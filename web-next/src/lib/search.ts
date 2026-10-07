import Database from "better-sqlite3";

import { currentAccountId } from "./accountScope";
import { listContactsByIds, listLabelMemberContactIds } from "./contactsRead";
import {
  combinedDedupeSql,
  getDb,
  hasDuplicateOfColumn,
  resetDb,
  splitNameParts,
} from "./dbCore";
import { ownerHandleMatcher } from "./owner";
import { sanitizePhoneDigits, toPhoneE164 } from "./phoneE164";
import {
  hasDateBounds,
  hasSearchCriteria,
  parseSearchQuery,
  toFtsMatch,
  type DateBounds,
  type ParsedSearchQuery,
} from "./searchQuery";
import type { ContactListItem } from "./types";
import { openWritableVaultDb } from "./vaultSchema";

/** Query-time phone variants for LIKE matching (raw, E.164, digits). */
function personMatchNeedles(raw: string): string[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  const out = new Set<string>([trimmed]);
  const e164 = toPhoneE164(trimmed);
  if (e164) out.add(e164);
  const digits = sanitizePhoneDigits(trimmed);
  if (digits.length >= 7) out.add(digits);
  return [...out];
}

/** Local calendar day start as UTC ISO (viewer TZ = process local). */
function localDayStartUtc(yyyyMmDd: string): string {
  const [y, m, d] = yyyyMmDd.split("-").map((p) => Number(p));
  return new Date(y!, m! - 1, d!, 0, 0, 0, 0).toISOString();
}

/** Exclusive end of local calendar day as UTC ISO. */
function localDayEndExclusiveUtc(yyyyMmDd: string): string {
  const [y, m, d] = yyyyMmDd.split("-").map((p) => Number(p));
  return new Date(y!, m! - 1, d! + 1, 0, 0, 0, 0).toISOString();
}

/** A message's instant (`messages.timestamp`, UTC). */
const MSG_TS = `m.timestamp`;

/** SQL CASE mapping attachment MIME → filetype category. */
const ATTACHMENT_CATEGORY_SQL = `
  CASE
    WHEN lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE 'image/%' THEN 'image'
    WHEN lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE 'video/%' THEN 'video'
    WHEN lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE 'audio/%' THEN 'audio'
    WHEN lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE '%vcard%'
      OR lower(coalesce(a.mime_type, a.derived_mime_type, '')) = 'text/x-vcard'
      THEN 'contact'
    WHEN lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE '%pdf%'
      OR lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE '%word%'
      OR lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE '%document%'
      OR lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE '%sheet%'
      OR lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE '%presentation%'
      OR lower(coalesce(a.mime_type, a.derived_mime_type, '')) LIKE 'text/%'
      THEN 'document'
    ELSE 'other'
  END
`;

export type SearchHitMessage = {
  id: number;
  timestamp: string;
  snippet: string;
  isFromMe: boolean;
  sender: string | null;
};

export type SearchAttachmentSummary = {
  name: string | null;
  mimeType: string | null;
  sizeBytes: number | null;
};

/** One matching message when `group:none`. */
export type SearchMessageHit = {
  messageId: number;
  conversationId: number;
  conversationType: "group" | "individual";
  contactId: number | null;
  title: string;
  chatIdentifier: string;
  timestamp: string;
  snippet: string;
  isFromMe: boolean;
  sender: string | null;
  attachments: SearchAttachmentSummary[];
  /** Messages before the hit when `context:N` is set. */
  contextBefore: SearchHitMessage[];
  /** Messages after the hit when `context:N` is set. */
  contextAfter: SearchHitMessage[];
};

export type SearchConversationHit = {
  conversationId: number;
  conversationType: "group" | "individual";
  /** Contact represented by a direct conversation, when its handle is assigned. */
  contactId: number | null;
  title: string;
  chatIdentifier: string;
  matchCount: number;
  dateStart: string | null;
  dateEnd: string | null;
  /** Best matching message for preview / scroll-to. */
  topMatch: SearchHitMessage | null;
};

export type SearchResult = {
  query: string;
  parsed: ParsedSearchQuery;
  totalConversations: number;
  /** Every distinct contact represented by matching direct conversations. */
  contactIds: number[];
  hits: SearchConversationHit[];
  /** Flat message rows when `group:none`. */
  messageHits?: SearchMessageHit[];
  totalMessages?: number;
  /** Present when `show:contact` groups results under each contact. */
  contacts?: SearchContactHit[];
  totalContacts?: number;
};

/** One contact plus the conversations of theirs that matched. */
export type SearchContactHit = {
  contact: ContactListItem;
  /** Matching conversations this contact takes part in. */
  hits: SearchConversationHit[];
  /** Total matching messages across those conversations. */
  matchCount: number;
};

const DEFAULT_LIMIT = 100;
const MAX_LIMIT = 100;

function openWritable(): Database.Database {
  return openWritableVaultDb();
}

function snippetFromBody(body: string | null, terms: string[]): string {
  const text = (body ?? "").replace(/\s+/g, " ").trim();
  if (!text) return "";
  const lower = text.toLowerCase();
  let idx = -1;
  for (const t of terms) {
    const i = lower.indexOf(t.toLowerCase());
    if (i >= 0 && (idx < 0 || i < idx)) idx = i;
  }
  if (idx < 0) {
    return text.length > 140 ? `${text.slice(0, 140)}…` : text;
  }
  const start = Math.max(0, idx - 40);
  const end = Math.min(text.length, idx + 100);
  const slice = text.slice(start, end);
  return `${start > 0 ? "…" : ""}${slice}${end < text.length ? "…" : ""}`;
}

function conversationTitle(
  conversationType: string,
  groupTitle: string | null,
  chatIdentifier: string,
  participantNames: string[],
): string {
  if (conversationType === "group") {
    if (groupTitle?.trim()) return groupTitle.trim();
    if (participantNames.length) return participantNames.slice(0, 4).join(", ");
    return chatIdentifier || "Group";
  }
  if (participantNames[0]) return participantNames[0];
  return chatIdentifier || "Conversation";
}

/**
 * A contact is linked to a conversation by being its 1:1 handle or one of its
 * participants, so group filters reach every member. Handles are compared by
 * handle_id — the handles table's identity key.
 */
const CONTACT_LINKED_SQL = `(
  ch.handle_id = c.chat_handle_id
  OR EXISTS (
    SELECT 1 FROM participants p_link
    WHERE p_link.conversation_id = c.id AND p_link.handle_id = ch.handle_id
  )
)`;

/**
 * Contacts whose overall first / last 1:1 message day falls within bounds,
 * independent of the query's other filters. Mirrors the date range shown in the
 * contact list. Resolved in one pass rather than per candidate conversation.
 */
function contactIdsWithinDayBounds(
  db: Database.Database,
  bound: "first" | "last",
  bounds: DateBounds,
): number[] {
  const accountId = currentAccountId();
  const hideDupes = hasDuplicateOfColumn() ? " AND m.duplicate_of IS NULL" : "";
  const day = bound === "first" ? "MIN" : "MAX";
  const having: string[] = [];
  const params: unknown[] = [accountId];
  if (bounds.from) having.push(`${day}(substr(m.timestamp, 1, 10)) >= ?`);
  if (bounds.to) having.push(`${day}(substr(m.timestamp, 1, 10)) < ?`);
  if (bounds.from) params.push(bounds.from);
  if (bounds.to) params.push(bounds.to);
  if (having.length === 0) return [];

  const rows = db
    .prepare(
      `SELECT cp.contact_id AS contact_id
       FROM contact_handles cp
       JOIN conversations cv
         ON cv.chat_handle_id = cp.handle_id
        AND cv.conversation_type = 'individual'
        AND cv.account_id = cp.account_id
       JOIN messages m ON m.conversation_id = cv.id
       WHERE cp.account_id = ?${hideDupes}
       GROUP BY cp.contact_id
       HAVING ${having.join(" AND ")}`,
    )
    .all(...params) as Array<{ contact_id: number }>;
  return rows.map((row) => row.contact_id);
}

/** Restrict conversations to those involving one of these contacts. */
function involvesContactsSql(contactIds: number[]): string {
  const ids = contactIds.filter((id) => Number.isInteger(id));
  if (ids.length === 0) return "1=0";
  return `EXISTS (
    SELECT 1 FROM contact_handles ch
    WHERE ch.account_id = c.account_id
      AND ch.contact_id IN (${ids.join(",")})
      AND ${CONTACT_LINKED_SQL}
  )`;
}

/** Whether the query uses first/last/phone “with person” contact filters. */
function hasPersonNameFilters(parsed: ParsedSearchQuery): boolean {
  return (
    parsed.noFirstName ||
    parsed.noLastName ||
    !!(parsed.firstName?.trim() || parsed.lastName?.trim() || parsed.phone?.trim())
  );
}

/**
 * Contact ids matching first:/last:/phone:/is:nofirst/is:nolast for Messages
 * “with person” expand (same matching rules as Contacts search).
 */
function contactIdsMatchingPersonFilters(parsed: ParsedSearchQuery): number[] {
  const db = getDb();
  const accountId = currentAccountId();
  const firstNeedle = parsed.firstName?.trim().toLocaleLowerCase() ?? "";
  const lastNeedle = parsed.lastName?.trim().toLocaleLowerCase() ?? "";
  const phoneNeedles = parsed.phone?.trim()
    ? personMatchNeedles(parsed.phone).map((n) => n.toLocaleLowerCase())
    : [];

  const rows = db
    .prepare(
      `SELECT c.id AS id,
              c.preferred_name AS preferred_name
       FROM contacts c
       WHERE c.account_id = ?
         AND NOT EXISTS (
           SELECT 1 FROM trashed_contacts tc
           WHERE tc.account_id = c.account_id AND tc.contact_id = c.id
         )`,
    )
    .all(accountId) as Array<{
    id: number;
    preferred_name: string | null;
  }>;

  const handleRows = db
    .prepare(
      `SELECT cp.contact_id AS contact_id, h.raw AS handle
       FROM contact_handles cp
       JOIN handles h ON h.id = cp.handle_id
       WHERE cp.account_id = ?`,
    )
    .all(accountId) as Array<{ contact_id: number; handle: string }>;
  const handlesByContact = new Map<number, string[]>();
  for (const row of handleRows) {
    const handles = handlesByContact.get(row.contact_id);
    if (handles) handles.push(row.handle);
    else handlesByContact.set(row.contact_id, [row.handle]);
  }

  return rows
    .filter((contact) => {
      const preferred = (contact.preferred_name ?? "").trim();
      const parts = splitNameParts(preferred || null);
      const first = preferred ? parts.first : "";
      const last = preferred.includes(" ") ? parts.last : "";
      const noFirst = !first.trim();
      const noLast = !last.trim();
      if (parsed.noFirstName && !noFirst) return false;
      if (parsed.noLastName && !noLast) return false;
      if (
        !parsed.noFirstName &&
        firstNeedle &&
        !first.toLocaleLowerCase().includes(firstNeedle)
      ) {
        return false;
      }
      if (
        !parsed.noLastName &&
        lastNeedle &&
        !last.toLocaleLowerCase().includes(lastNeedle)
      ) {
        return false;
      }
      if (phoneNeedles.length) {
        const phoneValues = (handlesByContact.get(contact.id) ?? []).map(
          (v) => v.toLocaleLowerCase(),
        );
        if (
          !phoneNeedles.some((needle) =>
            phoneValues.some((value) => value.includes(needle)),
          )
        ) {
          return false;
        }
      }
      return true;
    })
    .map((contact) => contact.id);
}

type SearchFilters = {
  fts: string | null;
  /** FROM clause fragment starting at `messages m`. */
  fromSql: string;
  whereSql: string;
  params: unknown[];
  dedupe: string;
  sourceFilter: string | null;
  /**
   * Contact id sets from contact-level filters (`within:`, first/last contact).
   * Conversations only need to involve one such contact, but contact-grouped
   * results must also drop the contacts that failed the filter themselves.
   */
  contactScopes: number[][];
};

/** Shared WHERE clause for both the flat and contact-grouped queries. */
function buildSearchFilters(
  parsed: ParsedSearchQuery,
  sourceOverride?: string | null,
): SearchFilters {
  const accountId = currentAccountId();
  const fts = toFtsMatch(parsed);
  const params: unknown[] = [accountId];
  const where: string[] = ["c.account_id = ?"];

  // JOIN FTS when present so ORDER BY bm25(messages_fts) works for relevance.
  // Handles are joined up front: c_h.raw is the conversation chat identifier,
  // m_h.raw the sender — the old TEXT columns live in the handles table now.
  let fromSql = `messages m
    JOIN conversations c ON c.id = m.conversation_id
    JOIN handles c_h ON c_h.id = c.chat_handle_id
    LEFT JOIN handles m_h ON m_h.id = m.sender_handle_id`;
  if (fts) {
    fromSql += " JOIN messages_fts ON messages_fts.rowid = m.id";
    where.push(`messages_fts MATCH ?`);
    params.push(fts);
  }

  if (parsed.from) {
    if (parsed.from.trim().toLowerCase() === "me") {
      where.push(`m.is_from_me = 1`);
    } else {
      const needles = personMatchNeedles(parsed.from);
      const parts: string[] = [];
      for (const n of needles) {
        const like = `%${n}%`;
        parts.push(
          `(m.is_from_me = 0 AND (m_h.raw LIKE ? OR EXISTS (
             SELECT 1 FROM participants p
             JOIN handles p_h ON p_h.id = p.handle_id
             WHERE p.conversation_id = c.id
               AND (p_h.raw LIKE ? OR coalesce(p.name_alias, '') LIKE ?)
           )))`,
        );
        params.push(like, like, like);
      }
      if (parts.length) where.push(`(${parts.join(" OR ")})`);
    }
  }

  // to:me = received; to:person = I sent in a conversation involving them.
  if (parsed.to) {
    if (parsed.to.trim().toLowerCase() === "me") {
      where.push(`m.is_from_me = 0`);
    } else {
      const needles = personMatchNeedles(parsed.to);
      const parts: string[] = [];
      for (const n of needles) {
        const like = `%${n}%`;
        parts.push(
          `(m.is_from_me = 1 AND (
             c_h.raw LIKE ?
             OR EXISTS (
               SELECT 1 FROM participants p
               JOIN handles p_h ON p_h.id = p.handle_id
               WHERE p.conversation_id = c.id
                 AND (p_h.raw LIKE ? OR coalesce(p.name_alias, '') LIKE ?)
             )
           ))`,
        );
        params.push(like, like, like);
      }
      if (parts.length) where.push(`(${parts.join(" OR ")})`);
    }
  }

  // with: = conversation involves person (any role).
  if (parsed.with) {
    const needles = personMatchNeedles(parsed.with);
    const parts: string[] = [];
    for (const n of needles) {
      const like = `%${n}%`;
      parts.push(
        `(c_h.raw LIKE ?
          OR EXISTS (
            SELECT 1 FROM participants p
            JOIN handles p_h ON p_h.id = p.handle_id
            WHERE p.conversation_id = c.id
              AND (p_h.raw LIKE ? OR coalesce(p.name_alias, '') LIKE ?)
          ))`,
      );
      params.push(like, like, like);
    }
    if (parts.length) where.push(`(${parts.join(" OR ")})`);
  }

  if (parsed.after) {
    const bound =
      parsed.after.length === 10
        ? localDayStartUtc(parsed.after)
        : parsed.after;
    where.push(`${MSG_TS} >= ?`);
    params.push(bound);
  }
  if (parsed.before) {
    const bound =
      parsed.before.length === 10
        ? localDayEndExclusiveUtc(parsed.before)
        : parsed.before;
    where.push(`${MSG_TS} < ?`);
    params.push(bound);
  }

  const sourceFilter = sourceOverride ?? parsed.source;
  if (sourceFilter) {
    where.push(`m.source = ?`);
    params.push(sourceFilter);
  }

  if (parsed.conversationType) {
    where.push(`c.conversation_type = ?`);
    params.push(parsed.conversationType);
  }

  if (parsed.hasAttachment === true) {
    where.push(
      `EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id)`,
    );
  } else if (parsed.hasAttachment === false) {
    where.push(
      `NOT EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id)`,
    );
  }

  if (parsed.filename?.trim()) {
    where.push(
      `EXISTS (
         SELECT 1 FROM attachments a
         WHERE a.message_id = m.id
           AND lower(coalesce(a.original_name, '')) LIKE ?
       )`,
    );
    params.push(`%${parsed.filename.trim().toLowerCase()}%`);
  }

  if (parsed.filetype?.trim()) {
    where.push(
      `EXISTS (
         SELECT 1 FROM attachments a
         WHERE a.message_id = m.id
           AND (${ATTACHMENT_CATEGORY_SQL}) = ?
       )`,
    );
    params.push(parsed.filetype.trim().toLowerCase());
  }

  if (parsed.largerBytes != null) {
    where.push(
      `EXISTS (
         SELECT 1 FROM attachments a
         WHERE a.message_id = m.id
           AND a.size_bytes IS NOT NULL
           AND a.size_bytes > ?
       )`,
    );
    params.push(parsed.largerBytes);
  }

  if (parsed.smallerBytes != null) {
    where.push(
      `EXISTS (
         SELECT 1 FROM attachments a
         WHERE a.message_id = m.id
           AND a.size_bytes IS NOT NULL
           AND a.size_bytes < ?
       )`,
    );
    params.push(parsed.smallerBytes);
  }

  if (parsed.inConversation?.trim()) {
    const like = `%${parsed.inConversation.trim()}%`;
    where.push(
      `(coalesce(c.group_title, '') LIKE ?
        OR c_h.raw LIKE ?
        OR EXISTS (
          SELECT 1 FROM participants p
          JOIN handles p_h ON p_h.id = p.handle_id
          WHERE p.conversation_id = c.id
            AND (p_h.raw LIKE ? OR coalesce(p.name_alias, '') LIKE ?)
        ))`,
    );
    params.push(like, like, like, like);
  }

  const contactScopes: number[][] = [];
  const scopeToContacts = (contactIds: number[]) => {
    contactScopes.push(contactIds);
    where.push(involvesContactsSql(contactIds));
  };

  // Scoped to a label's member contacts.
  if (parsed.within) {
    scopeToContacts(listLabelMemberContactIds(parsed.within));
  }

  if (hasPersonNameFilters(parsed)) {
    scopeToContacts(contactIdsMatchingPersonFilters(parsed));
  }

  const db = getDb();
  if (hasDateBounds(parsed.firstContact)) {
    scopeToContacts(contactIdsWithinDayBounds(db, "first", parsed.firstContact));
  }
  if (hasDateBounds(parsed.lastContact)) {
    scopeToContacts(contactIdsWithinDayBounds(db, "last", parsed.lastContact));
  }

  where.push(
    `NOT EXISTS (
       SELECT 1 FROM trashed_conversations tc
       WHERE tc.account_id = c.account_id AND tc.conversation_id = c.id
     )`,
  );

  where.push("1=1"); // keep join shape stable

  return {
    fts,
    fromSql,
    whereSql: where.join(" AND "),
    params,
    dedupe: combinedDedupeSql(sourceFilter, "m"),
    sourceFilter: sourceFilter ?? null,
    contactScopes,
  };
}

function conversationOrderSql(
  sort: ParsedSearchQuery["sort"],
  fts: string | null,
): string {
  if (sort === "relevance" && fts) {
    return "MIN(bm25(messages_fts)) ASC, MAX(m.timestamp) DESC";
  }
  if (sort === "date-asc") {
    return "MAX(m.timestamp) ASC, c.id ASC";
  }
  return "MAX(m.timestamp) DESC, c.id DESC";
}

function messageOrderSql(
  sort: ParsedSearchQuery["sort"],
  fts: string | null,
): string {
  if (sort === "relevance" && fts) {
    return `bm25(messages_fts) ASC, ${MSG_TS} DESC, m.id DESC`;
  }
  if (sort === "date-asc") {
    return `${MSG_TS} ASC, m.id ASC`;
  }
  return `${MSG_TS} DESC, m.id DESC`;
}

/** Initialize FTS if a readonly connection opened before first-use setup. */
function ensureFtsReady(): void {
  const writeDb = openWritable();
  writeDb.close();
  resetDb();
}

export function searchVault(
  rawQuery: string,
  opts: { limit?: number; offset?: number; source?: string | null } = {},
): SearchResult {
  const parsed = parseSearchQuery(rawQuery);
  if (parsed.mode !== "contacts" && parsed.groupBy === "none") {
    return searchVaultMessages(rawQuery, opts);
  }

  const limit = Math.min(
    Math.max(1, opts.limit ?? DEFAULT_LIMIT),
    MAX_LIMIT,
  );
  const offset = Math.max(0, opts.offset ?? 0);

  // Presentation-only operators (`group:`, `sort:`, `context:`) do not count.
  if (!hasSearchCriteria(parsed)) {
    return {
      query: rawQuery,
      parsed,
      totalConversations: 0,
      contactIds: [],
      hits: [],
    };
  }

  ensureFtsReady();

  const db = getDb();
  const { fts, fromSql, whereSql, params, dedupe } = buildSearchFilters(
    parsed,
    opts.source,
  );
  const orderSql = conversationOrderSql(parsed.sort, fts);
  const countRow = db
    .prepare(
      `SELECT COUNT(*) AS n FROM (
         SELECT c.id
         FROM ${fromSql}
         WHERE ${whereSql}${dedupe}
         GROUP BY c.id
       )`,
    )
    .get(...params) as { n: number };

  const convRows = db
    .prepare(
      `SELECT ${CONVERSATION_HIT_COLUMNS}
       FROM ${fromSql}
       WHERE ${whereSql}${dedupe}
       GROUP BY c.id
       ORDER BY ${orderSql}
       LIMIT ? OFFSET ?`,
    )
    .all(...params, limit, offset) as ConversationRow[];

  const contactRows = db
    .prepare(
      `SELECT DISTINCT contact_id
       FROM (
         SELECT ch.contact_id AS contact_id
         FROM ${fromSql}
         JOIN contact_handles ch
           ON ch.account_id = c.account_id
          AND ch.handle_id = c.chat_handle_id
         WHERE ${whereSql}${dedupe}
           AND c.conversation_type = 'individual'
         GROUP BY c.id, ch.contact_id
       )
       ORDER BY contact_id`,
    )
    .all(...params) as Array<{ contact_id: number }>;
  const contactIds = contactRows.map((row) => row.contact_id);

  const hits = buildHits(db, convRows, parsed, fts);

  return {
    query: rawQuery,
    parsed,
    totalConversations: countRow.n,
    contactIds,
    hits,
  };
}

/** Flat message results for `group:none`. */
function searchVaultMessages(
  rawQuery: string,
  opts: { limit?: number; offset?: number; source?: string | null } = {},
): SearchResult {
  const parsed = parseSearchQuery(rawQuery);
  const limit = Math.min(
    Math.max(1, opts.limit ?? DEFAULT_LIMIT),
    MAX_LIMIT,
  );
  const offset = Math.max(0, opts.offset ?? 0);

  if (!hasSearchCriteria(parsed)) {
    return {
      query: rawQuery,
      parsed,
      totalConversations: 0,
      contactIds: [],
      hits: [],
      messageHits: [],
      totalMessages: 0,
    };
  }

  ensureFtsReady();

  const db = getDb();
  const { fts, fromSql, whereSql, params, dedupe } = buildSearchFilters(
    parsed,
    opts.source,
  );
  const orderSql = messageOrderSql(parsed.sort, fts);

  const countRow = db
    .prepare(
      `SELECT COUNT(DISTINCT m.id) AS n
       FROM ${fromSql}
       WHERE ${whereSql}${dedupe}`,
    )
    .get(...params) as { n: number };

  const msgRows = db
    .prepare(
      `SELECT DISTINCT
         m.id AS message_id,
         m.timestamp AS timestamp,
         m.body AS body,
         m.is_from_me AS is_from_me,
         m_h.raw AS sender,
         c.id AS conversation_id,
         c.conversation_type AS conversation_type,
         c.group_title AS group_title,
         c_h.raw AS chat_identifier,
         (
           SELECT ch.contact_id
           FROM contact_handles ch
           WHERE ch.account_id = c.account_id
             AND ch.handle_id = c.chat_handle_id
             AND c.conversation_type = 'individual'
           LIMIT 1
         ) AS contact_id
       FROM ${fromSql}
       WHERE ${whereSql}${dedupe}
       ORDER BY ${orderSql}
       LIMIT ? OFFSET ?`,
    )
    .all(...params, limit, offset) as Array<{
    message_id: number;
    timestamp: string;
    body: string | null;
    is_from_me: number;
    sender: string | null;
    conversation_id: number;
    conversation_type: string;
    group_title: string | null;
    chat_identifier: string;
    contact_id: number | null;
  }>;

  const highlightTerms = [
    ...parsed.terms,
    ...parsed.phrases,
    ...(parsed.subject ? [parsed.subject] : []),
  ];

  const messageHits = msgRows.map((row) => {
    const participants = db
      .prepare(
        `SELECT h.raw AS handle, p.name_alias
         FROM participants p
         JOIN handles h ON h.id = p.handle_id
         WHERE p.conversation_id = ?
         ORDER BY p.id`,
      )
      .all(row.conversation_id) as Array<{
      handle: string;
      name_alias: string | null;
    }>;
    const names = participants.map((p) => p.name_alias?.trim() || p.handle);
    const attachments = listAttachmentSummaries(db, row.message_id);
    const { before, after } =
      parsed.context > 0
        ? messageContext(db, row.message_id, parsed.context)
        : { before: [], after: [] };

    return {
      messageId: row.message_id,
      conversationId: row.conversation_id,
      conversationType:
        row.conversation_type === "group" ? ("group" as const) : ("individual" as const),
      contactId: row.contact_id,
      title: conversationTitle(
        row.conversation_type,
        row.group_title,
        row.chat_identifier,
        names,
      ),
      chatIdentifier: row.chat_identifier,
      timestamp: row.timestamp,
      snippet: snippetFromBody(row.body, highlightTerms),
      isFromMe: row.is_from_me === 1,
      sender: row.sender,
      attachments,
      contextBefore: before,
      contextAfter: after,
    };
  });

  const contactIds = [
    ...new Set(
      messageHits
        .map((h) => h.contactId)
        .filter((id): id is number => id != null),
    ),
  ].sort((a, b) => a - b);

  return {
    query: rawQuery,
    parsed,
    totalConversations: new Set(messageHits.map((h) => h.conversationId)).size,
    contactIds,
    hits: [],
    messageHits,
    totalMessages: countRow.n,
  };
}

function listAttachmentSummaries(
  db: Database.Database,
  messageId: number,
): SearchAttachmentSummary[] {
  const rows = db
    .prepare(
      `SELECT original_name, mime_type, size_bytes
       FROM attachments
       WHERE message_id = ?
       ORDER BY id`,
    )
    .all(messageId) as Array<{
    original_name: string | null;
    mime_type: string | null;
    size_bytes: number | null;
  }>;
  return rows.map((row) => ({
    name: row.original_name,
    mimeType: row.mime_type,
    sizeBytes: row.size_bytes,
  }));
}

/** Neighboring messages in the same conversation (by timestamp/id). */
export function messageContext(
  db: Database.Database,
  messageId: number,
  n: number,
): { before: SearchHitMessage[]; after: SearchHitMessage[] } {
  if (n <= 0) return { before: [], after: [] };
  const anchor = db
    .prepare(
      `SELECT m.id, m.conversation_id, m.timestamp, m.body, m.is_from_me,
              h.raw AS sender
       FROM messages m
       LEFT JOIN handles h ON h.id = m.sender_handle_id
       WHERE m.id = ?`,
    )
    .get(messageId) as
    | {
        id: number;
        conversation_id: number;
        timestamp: string;
        body: string | null;
        is_from_me: number;
        sender: string | null;
      }
    | undefined;
  if (!anchor) return { before: [], after: [] };

  const toHit = (row: {
    id: number;
    timestamp: string;
    body: string | null;
    is_from_me: number;
    sender: string | null;
  }): SearchHitMessage => ({
    id: row.id,
    timestamp: row.timestamp,
    snippet: snippetFromBody(row.body, []),
    isFromMe: row.is_from_me === 1,
    sender: row.sender,
  });

  const before = db
    .prepare(
      `SELECT m.id, m.timestamp, m.body, m.is_from_me, h.raw AS sender
       FROM messages m
       LEFT JOIN handles h ON h.id = m.sender_handle_id
       WHERE m.conversation_id = ?
         AND (m.timestamp < ? OR (m.timestamp = ? AND m.id < ?))
       ORDER BY m.timestamp DESC, m.id DESC
       LIMIT ?`,
    )
    .all(
      anchor.conversation_id,
      anchor.timestamp,
      anchor.timestamp,
      anchor.id,
      n,
    ) as Array<{
    id: number;
    timestamp: string;
    body: string | null;
    is_from_me: number;
    sender: string | null;
  }>;

  const after = db
    .prepare(
      `SELECT m.id, m.timestamp, m.body, m.is_from_me, h.raw AS sender
       FROM messages m
       LEFT JOIN handles h ON h.id = m.sender_handle_id
       WHERE m.conversation_id = ?
         AND (m.timestamp > ? OR (m.timestamp = ? AND m.id > ?))
       ORDER BY m.timestamp ASC, m.id ASC
       LIMIT ?`,
    )
    .all(
      anchor.conversation_id,
      anchor.timestamp,
      anchor.timestamp,
      anchor.id,
      n,
    ) as Array<{
    id: number;
    timestamp: string;
    body: string | null;
    is_from_me: number;
    sender: string | null;
  }>;

  return {
    before: before.reverse().map(toHit),
    after: after.map(toHit),
  };
}

/**
 * Message ids around a hit for thread loading when `context:N` is set.
 * Includes the anchor itself.
 */
export function searchMessageContextIds(
  messageId: number,
  n: number,
): number[] {
  if (!Number.isInteger(messageId) || messageId <= 0) return [];
  ensureFtsReady();
  const db = getDb();
  const { before, after } = messageContext(db, messageId, Math.max(0, n));
  return [...before.map((m) => m.id), messageId, ...after.map((m) => m.id)];
}

type ConversationRow = {
  conversation_id: number;
  conversation_type: string;
  group_title: string | null;
  chat_identifier: string;
  contact_id: number | null;
  match_count: number;
  date_start: string | null;
  date_end: string | null;
  sample_message_id: number;
};

const CONVERSATION_HIT_COLUMNS = `
  c.id AS conversation_id,
  c.conversation_type AS conversation_type,
  c.group_title AS group_title,
  c_h.raw AS chat_identifier,
  (
    SELECT ch.contact_id
    FROM contact_handles ch
    WHERE ch.account_id = c.account_id
      AND ch.handle_id = c.chat_handle_id
      AND c.conversation_type = 'individual'
    LIMIT 1
  ) AS contact_id,
  COUNT(DISTINCT m.id) AS match_count,
  MIN(m.timestamp) AS date_start,
  MAX(m.timestamp) AS date_end,
  MAX(m.id) AS sample_message_id`;

function buildHits(
  db: Database.Database,
  convRows: ConversationRow[],
  parsed: ParsedSearchQuery,
  fts: string | null,
): SearchConversationHit[] {
  const highlightTerms = [
    ...parsed.terms,
    ...parsed.phrases,
    ...(parsed.subject ? [parsed.subject] : []),
  ];

  return convRows.map((row) => {
    const participants = db
      .prepare(
        `SELECT h.raw AS handle, p.name_alias
         FROM participants p
         JOIN handles h ON h.id = p.handle_id
         WHERE p.conversation_id = ?
         ORDER BY p.id`,
      )
      .all(row.conversation_id) as Array<{
      handle: string;
      name_alias: string | null;
    }>;
    const names = participants.map((p) => p.name_alias?.trim() || p.handle);

    let topMatch: SearchHitMessage | null = null;
    const msg = db
      .prepare(
        `SELECT m.id, m.timestamp, m.body, m.is_from_me, h.raw AS sender
         FROM messages m
         LEFT JOIN handles h ON h.id = m.sender_handle_id
         WHERE m.id = ?`,
      )
      .get(row.sample_message_id) as
      | {
          id: number;
          timestamp: string;
          body: string | null;
          is_from_me: number;
          sender: string | null;
        }
      | undefined;

    // Prefer a message that actually contains a highlight term when FTS was used.
    let best = msg;
    if (fts && highlightTerms.length > 0) {
      const candidates = db
        .prepare(
          `SELECT m.id, m.timestamp, m.body, m.is_from_me, h.raw AS sender
           FROM messages m
           LEFT JOIN handles h ON h.id = m.sender_handle_id
           WHERE m.conversation_id = ?
             AND m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?)
           ORDER BY m.timestamp DESC
           LIMIT 5`,
        )
        .all(row.conversation_id, fts) as Array<{
        id: number;
        timestamp: string;
        body: string | null;
        is_from_me: number;
        sender: string | null;
      }>;
      if (candidates.length > 0) best = candidates[0];
    }

    if (best) {
      topMatch = {
        id: best.id,
        timestamp: best.timestamp,
        snippet: snippetFromBody(best.body, highlightTerms),
        isFromMe: best.is_from_me === 1,
        sender: best.sender,
      };
    }

    return {
      conversationId: row.conversation_id,
      conversationType:
        row.conversation_type === "group" ? "group" : "individual",
      contactId: row.contact_id,
      title: conversationTitle(
        row.conversation_type,
        row.group_title,
        row.chat_identifier,
        names,
      ),
      chatIdentifier: row.chat_identifier,
      matchCount: row.match_count,
      dateStart: row.date_start,
      dateEnd: row.date_end,
      topMatch,
    };
  });
}

export type ConversationMatch = {
  id: number;
  timestamp: string;
};

export type ConversationMatchResult = {
  query: string;
  parsed: ParsedSearchQuery;
  conversationIds: number[];
  matches: ConversationMatch[];
};

/**
 * Every matching message in the given conversations, oldest first, so an
 * in-thread find bar can step through them. A direct thread may span several
 * conversation rows (one per source). Applies the same filters as
 * {@link searchVault}.
 */
export function searchConversationMatches(
  rawQuery: string,
  conversationIds: number[],
  opts: { source?: string | null } = {},
): ConversationMatchResult {
  const parsed = parseSearchQuery(rawQuery);
  const ids = conversationIds.filter((id) => Number.isInteger(id) && id > 0);
  if (ids.length === 0 || (!hasSearchCriteria(parsed) && !rawQuery.trim())) {
    return { query: rawQuery, parsed, conversationIds: ids, matches: [] };
  }

  ensureFtsReady();

  const db = getDb();
  const { fromSql, whereSql, params, dedupe } = buildSearchFilters(
    parsed,
    opts.source,
  );
  const matches = db
    .prepare(
      `SELECT DISTINCT m.id AS id, m.timestamp AS timestamp
       FROM ${fromSql}
       WHERE ${whereSql}${dedupe} AND c.id IN (${ids.map(() => "?").join(",")})
       ORDER BY m.timestamp ASC, m.id ASC`,
    )
    .all(...params, ...ids) as ConversationMatch[];

  return { query: rawQuery, parsed, conversationIds: ids, matches };
}

/** Contacts allowed to head a result group; null when unfiltered. */
function intersectContactScopes(scopes: number[][]): Set<number> | null {
  const [first, ...rest] = scopes;
  if (!first) return null;
  let allowed = new Set<number>(first);
  for (const scope of rest) {
    const scopeSet = new Set<number>(scope);
    allowed = new Set([...allowed].filter((id) => scopeSet.has(id)));
  }
  return allowed;
}

/** Contacts holding a vault-owner handle, so search can skip them. */
function ownerContactIds(db: Database.Database): Set<number> {
  const accountId = currentAccountId();
  const rows = db
    .prepare(
      `SELECT h.raw AS handle, cp.contact_id AS contact_id
       FROM contact_handles cp
       JOIN handles h ON h.id = cp.handle_id
       WHERE cp.account_id = ?`,
    )
    .all(accountId) as Array<{ handle: string; contact_id: number }>;
  const isOwner = ownerHandleMatcher();
  const ids = new Set<number>();
  for (const row of rows) {
    if (isOwner(row.handle)) ids.add(row.contact_id);
  }
  return ids;
}

function countMatches(
  actual: number,
  comparison: ParsedSearchQuery["messageCount"],
): boolean {
  if (!comparison) return true;
  switch (comparison.comparator) {
    case "=":
      return actual === comparison.value;
    case ">":
      return actual > comparison.value;
    case ">=":
      return actual >= comparison.value;
    case "<":
      return actual < comparison.value;
    case "<=":
      return actual <= comparison.value;
  }
}

function dateMatches(value: string | null, bounds: DateBounds): boolean {
  if (!hasDateBounds(bounds)) return true;
  if (!value) return false;
  const day = value.slice(0, 10);
  return (!bounds.from || day >= bounds.from) && (!bounds.to || day < bounds.to);
}

/**
 * Contact-primary search. Unlike message search, this includes contacts with no
 * messages and applies message/group counts to the contact as a whole.
 */
export function searchVaultContacts(
  rawQuery: string,
  opts: { limit?: number; offset?: number } = {},
): SearchResult {
  const parsed = parseSearchQuery(rawQuery);
  const limit = Math.min(Math.max(1, opts.limit ?? DEFAULT_LIMIT), MAX_LIMIT);
  const offset = Math.max(0, opts.offset ?? 0);
  const empty: SearchResult = {
    query: rawQuery,
    parsed,
    totalConversations: 0,
    contactIds: [],
    hits: [],
    contacts: [],
    totalContacts: 0,
  };
  if (parsed.mode !== "contacts") return empty;

  const db = getDb();
  const accountId = currentAccountId();
  const candidateRows = db
    .prepare(
      `SELECT c.id
       FROM contacts c
       WHERE c.account_id = ?
         AND NOT EXISTS (
           SELECT 1 FROM trashed_contacts tc
           WHERE tc.account_id = c.account_id AND tc.contact_id = c.id
         )
       ORDER BY c.id`,
    )
    .all(accountId) as Array<{ id: number }>;
  const skipIds = ownerContactIds(db);
  const candidateIds = candidateRows
    .map((row) => row.id)
    .filter((id) => !skipIds.has(id));
  const allItems = listContactsByIds(candidateIds);

  const handleRows = db
    .prepare(
      `SELECT cp.contact_id AS contact_id, h.raw AS handle
       FROM contact_handles cp
       JOIN handles h ON h.id = cp.handle_id
       WHERE cp.account_id = ?
       ORDER BY cp.contact_id, h.raw`,
    )
    .all(accountId) as Array<{ contact_id: number; handle: string }>;
  const handlesByContact = new Map<number, string[]>();
  for (const row of handleRows) {
    const handles = handlesByContact.get(row.contact_id);
    if (handles) handles.push(row.handle);
    else handlesByContact.set(row.contact_id, [row.handle]);
  }
  const withinIds = parsed.within
    ? new Set(listLabelMemberContactIds(parsed.within))
    : null;
  const handleNeedle = parsed.handle?.trim().toLocaleLowerCase() ?? "";
  const firstNeedle = parsed.firstName?.trim().toLocaleLowerCase() ?? "";
  const lastNeedle = parsed.lastName?.trim().toLocaleLowerCase() ?? "";
  const phoneNeedles = parsed.phone?.trim()
    ? personMatchNeedles(parsed.phone).map((n) => n.toLocaleLowerCase())
    : [];
  const matchingItems = allItems
    .filter((contact) => {
      if (withinIds && !withinIds.has(contact.id)) return false;
      const preferred = (contact.preferredName ?? "").trim();
      const parts = splitNameParts(preferred || null);
      const first = preferred ? parts.first : "";
      const last = preferred.includes(" ") ? parts.last : "";
      const noFirst = !first.trim();
      const noLast = !last.trim();
      if (parsed.noFirstName && !noFirst) return false;
      if (parsed.noLastName && !noLast) return false;
      if (
        !parsed.noFirstName &&
        firstNeedle &&
        !first.toLocaleLowerCase().includes(firstNeedle)
      ) {
        return false;
      }
      if (
        !parsed.noLastName &&
        lastNeedle &&
        !last.toLocaleLowerCase().includes(lastNeedle)
      ) {
        return false;
      }
      const phoneValues = [
        contact.preferredHandle ?? "",
        ...(handlesByContact.get(contact.id) ?? []),
      ].map((v) => v.toLocaleLowerCase());
      if (
        phoneNeedles.length &&
        !phoneNeedles.some((needle) =>
          phoneValues.some((value) => value.includes(needle)),
        )
      ) {
        return false;
      }
      // Legacy combined name/number filter.
      if (
        handleNeedle &&
        ![
          contact.displayName,
          contact.preferredName ?? "",
          first,
          last,
          ...phoneValues,
        ].some((value) => value.toLocaleLowerCase().includes(handleNeedle))
      ) {
        return false;
      }
      return (
        dateMatches(contact.dateStart, parsed.firstContact) &&
        dateMatches(contact.dateEnd, parsed.lastContact) &&
        countMatches(contact.groupMessageCount, parsed.groupCount) &&
        countMatches(contact.messageCount, parsed.messageCount)
      );
    })
    .sort(
      (a, b) =>
        (b.dateEnd ?? "").localeCompare(a.dateEnd ?? "") ||
        a.sortLast.localeCompare(b.sortLast, undefined, {
          sensitivity: "base",
        }) ||
        a.id - b.id,
    );
  const pageItems = matchingItems.slice(offset, offset + limit);
  const pageIds = pageItems.map((contact) => contact.id);
  if (pageIds.length === 0) {
    return {
      ...empty,
      contactIds: matchingItems.map((contact) => contact.id),
      totalContacts: matchingItems.length,
    };
  }

  const placeholders = pageIds.map(() => "?").join(",");
  const pairRows = db
    .prepare(
      `SELECT DISTINCT ch.contact_id, c.id AS conversation_id
       FROM contact_handles ch
       JOIN conversations c
         ON c.account_id = ch.account_id
        AND (
          (c.conversation_type = 'individual' AND c.chat_handle_id = ch.handle_id)
          OR (
            c.conversation_type = 'group'
            AND EXISTS (
              SELECT 1 FROM participants p
              WHERE p.conversation_id = c.id AND p.handle_id = ch.handle_id
            )
          )
        )
       WHERE ch.account_id = ?
         AND ch.contact_id IN (${placeholders})
         AND NOT EXISTS (
           SELECT 1 FROM trashed_conversations tc
           WHERE tc.account_id = c.account_id AND tc.conversation_id = c.id
         )`,
    )
    .all(accountId, ...pageIds) as Array<{
    contact_id: number;
    conversation_id: number;
  }>;
  const conversationIds = [
    ...new Set(pairRows.map((row) => row.conversation_id)),
  ];
  const convRows =
    conversationIds.length === 0
      ? []
      : (db
          .prepare(
            `SELECT ${CONVERSATION_HIT_COLUMNS}
             FROM messages m
             JOIN conversations c ON c.id = m.conversation_id
             JOIN handles c_h ON c_h.id = c.chat_handle_id
             WHERE c.account_id = ?
               AND c.id IN (${conversationIds.map(() => "?").join(",")})
               ${combinedDedupeSql(null, "m")}
             GROUP BY c.id
             ORDER BY MAX(m.timestamp) DESC`,
          )
          .all(accountId, ...conversationIds) as ConversationRow[]);
  const hits = buildHits(db, convRows, parsed, null);
  const hitsById = new Map(hits.map((hit) => [hit.conversationId, hit]));
  const hitIdsByContact = new Map<number, number[]>();
  for (const row of pairRows) {
    const ids = hitIdsByContact.get(row.contact_id);
    if (ids) ids.push(row.conversation_id);
    else hitIdsByContact.set(row.contact_id, [row.conversation_id]);
  }
  const contacts: SearchContactHit[] = pageItems.map((contact) => {
    const contactHits = (hitIdsByContact.get(contact.id) ?? [])
      .map((id) => hitsById.get(id))
      .filter((hit): hit is SearchConversationHit => hit != null)
      .sort((a, b) => (b.dateEnd ?? "").localeCompare(a.dateEnd ?? ""));
    return {
      contact,
      hits: contactHits,
      matchCount: contactHits.reduce((sum, hit) => sum + hit.matchCount, 0),
    };
  });

  return {
    query: rawQuery,
    parsed,
    totalConversations: conversationIds.length,
    contactIds: matchingItems.map((contact) => contact.id),
    hits,
    contacts,
    totalContacts: matchingItems.length,
  };
}

/**
 * Same filters as {@link searchVault}, but grouped under each contact who takes
 * part in a matching conversation. A group conversation appears under every
 * participating contact. Contacts are paginated, not conversations.
 */
export function searchVaultByContact(
  rawQuery: string,
  opts: { limit?: number; offset?: number; source?: string | null } = {},
): SearchResult {
  const parsed = parseSearchQuery(rawQuery);
  const limit = Math.min(Math.max(1, opts.limit ?? DEFAULT_LIMIT), MAX_LIMIT);
  const offset = Math.max(0, opts.offset ?? 0);

  const empty: SearchResult = {
    query: rawQuery,
    parsed,
    totalConversations: 0,
    contactIds: [],
    hits: [],
    contacts: [],
    totalContacts: 0,
  };
  if (!hasSearchCriteria(parsed) && !rawQuery.trim()) return empty;

  ensureFtsReady();

  const db = getDb();
  const { fts, fromSql, whereSql, params, dedupe, contactScopes } =
    buildSearchFilters(parsed, opts.source);

  // Every (contact, conversation) pair that matched, newest conversation first.
  // Conversations are collapsed before fanning out to contacts, so the handle
  // lookups run per conversation rather than per matching message.
  const accountId = currentAccountId();
  const pairRows = db
    .prepare(
      `WITH matched AS (
         SELECT c.id AS conversation_id,
                c.chat_handle_id AS chat_handle_id,
                MAX(m.timestamp) AS last_ts
         FROM ${fromSql}
         WHERE ${whereSql}${dedupe}
         GROUP BY c.id
       )
       SELECT ch.contact_id AS contact_id, mt.conversation_id, mt.last_ts
       FROM matched mt
       JOIN contact_handles ch
         ON ch.account_id = ? AND ch.handle_id = mt.chat_handle_id
       UNION
       SELECT ch.contact_id AS contact_id, mt.conversation_id, mt.last_ts
       FROM matched mt
       JOIN participants p ON p.conversation_id = mt.conversation_id
       JOIN contact_handles ch
         ON ch.account_id = ? AND ch.handle_id = p.handle_id
       ORDER BY last_ts DESC`,
    )
    .all(...params, accountId, accountId) as Array<{
    contact_id: number;
    conversation_id: number;
    last_ts: string | null;
  }>;

  if (pairRows.length === 0) return empty;

  // Older vaults may have a contact holding an owner handle; the owner is a
  // participant in their own groups and should not be listed as a match.
  const skipContactIds = ownerContactIds(db);
  // A contact-level filter also decides who may head a result group: sharing a
  // group chat with a match is not the same as being one.
  const allowedContactIds = intersectContactScopes(contactScopes);

  const conversationIdsByContact = new Map<number, number[]>();
  for (const row of pairRows) {
    if (skipContactIds.has(row.contact_id)) continue;
    if (allowedContactIds && !allowedContactIds.has(row.contact_id)) continue;
    const list = conversationIdsByContact.get(row.contact_id);
    if (list) list.push(row.conversation_id);
    else conversationIdsByContact.set(row.contact_id, [row.conversation_id]);
  }
  if (conversationIdsByContact.size === 0) return empty;

  // Contacts ordered by their most recent matching message.
  const orderedContactIds = [...conversationIdsByContact.keys()];
  const pageContactIds = orderedContactIds.slice(offset, offset + limit);
  const conversationIds = [
    ...new Set(pageContactIds.flatMap((id) => conversationIdsByContact.get(id) ?? [])),
  ];

  const convRows =
    conversationIds.length === 0
      ? []
      : (db
          .prepare(
            `SELECT ${CONVERSATION_HIT_COLUMNS}
             FROM ${fromSql}
             WHERE ${whereSql}${dedupe}
               AND c.id IN (${conversationIds.map(() => "?").join(",")})
             GROUP BY c.id
             ORDER BY MAX(m.timestamp) DESC`,
          )
          .all(...params, ...conversationIds) as ConversationRow[]);

  const hits = buildHits(db, convRows, parsed, fts);
  const hitsById = new Map(hits.map((hit) => [hit.conversationId, hit]));

  const contactItems = listContactsByIds(pageContactIds);
  const contacts: SearchContactHit[] = [];
  for (const contact of contactItems) {
    const contactHits = (conversationIdsByContact.get(contact.id) ?? [])
      .map((id) => hitsById.get(id))
      .filter((hit): hit is SearchConversationHit => hit != null);
    if (contactHits.length === 0) continue;
    contacts.push({
      contact,
      hits: contactHits,
      matchCount: contactHits.reduce((n, hit) => n + hit.matchCount, 0),
    });
  }

  return {
    query: rawQuery,
    parsed,
    totalConversations: new Set(pairRows.map((r) => r.conversation_id)).size,
    contactIds: orderedContactIds,
    hits,
    contacts,
    totalContacts: orderedContactIds.length,
  };
}
