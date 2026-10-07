import { currentAccountId } from "./accountScope";
import {
  getDb,
  hasDuplicateOfColumn,
  hasTrashedConversationsTable,
  notTrashedHandleSql,
  resetDb,
  usefulNameAlias,
} from "./dbCore";
import type { HandleType } from "./handleKind";
import { formatPhoneDisplay } from "./phoneE164";
import {
  contactMessageSourceCountsForConversations,
  contactYearlyThreadsForPhones,
  type ContactSourceCounts,
} from "./contactsRead";
import { contactGroupChatThreadsForPhones } from "./groupChatsRead";
import type { GroupChatThread, UnassignedHandle, YearThread } from "./types";

/** 1:1 conversations with messages whose handle is not on any contact. */
export function listUnassignedHandles(): UnassignedHandle[] {
  return listHandleSection("unassigned");
}

/** Unassigned handles that were moved to Trash. */
export function listTrashedHandles(): UnassignedHandle[] {
  resetDb();
  return listHandleSection("trash");
}

export type GroupParticipantHandle = {
  handle: string;
  handleType: HandleType | null;
  nameAlias: string | null;
};

/**
 * Group participants with no contact of their own.
 *
 * Kept separate from {@link listUnassignedHandles} because that drives the
 * Unassigned and Trash views, which only ever list 1:1 handles. These handles
 * have no 1:1 conversation at all, so they only surface through group
 * participation. A participant with `contact_id IS NULL` has no contact.
 */
export function listUnassignedGroupParticipantHandles(): GroupParticipantHandle[] {
  const accountId = currentAccountId();
  const db = getDb();
  const trashHandleFilter = notTrashedHandleSql("p.handle_id", "c.account_id");
  const trashConvFilter = hasTrashedConversationsTable(db)
    ? `AND NOT EXISTS (
         SELECT 1 FROM trashed_conversations tc
         WHERE tc.conversation_id = c.id AND tc.account_id = c.account_id
       )`
    : "";

  const rows = db
    .prepare(
      `SELECT h.raw AS handle, h.handle_type AS handle_type, MAX(p.name_alias) AS name_alias
       FROM participants p
       JOIN conversations c ON c.id = p.conversation_id
       JOIN handles h ON h.id = p.handle_id
       LEFT JOIN contact_handles cp
         ON cp.handle_id = p.handle_id AND cp.account_id = c.account_id
       WHERE c.account_id = ?
         AND c.conversation_type = 'group'
         AND trim(coalesce(h.raw, '')) <> ''
         AND cp.contact_id IS NULL
         AND EXISTS (
           SELECT 1 FROM messages m WHERE m.conversation_id = c.id
         )
         ${trashHandleFilter}
         ${trashConvFilter}
       GROUP BY p.handle_id
       ORDER BY h.raw COLLATE NOCASE`,
    )
    .all(accountId) as Array<{
    handle: string;
    handle_type: string | null;
    name_alias: string | null;
  }>;

  return rows.map((r) => ({
    handle: r.handle.trim(),
    handleType: (r.handle_type as HandleType | null) ?? null,
    nameAlias: usefulNameAlias(r.name_alias, r.handle),
  }));
}


function listHandleSection(section: "unassigned" | "trash"): UnassignedHandle[] {
  const accountId = currentAccountId();
  const db = getDb();
  const hideDupes = hasDuplicateOfColumn() ? " AND m.duplicate_of IS NULL" : "";
  // A handle has no trash of its own: its 1:1 conversation `c` does.
  const trashFilter =
    section === "trash"
      ? `AND EXISTS (
           SELECT 1 FROM trashed_conversations th
           WHERE th.conversation_id = c.id AND th.account_id = c.account_id
         )`
      : `AND NOT EXISTS (
           SELECT 1 FROM trashed_conversations th
           WHERE th.conversation_id = c.id AND th.account_id = c.account_id
         )`;

  const trashedAtSelect =
    section === "trash"
      ? `, (
           SELECT th.trashed_at FROM trashed_conversations th
           WHERE th.conversation_id = c.id AND th.account_id = c.account_id
           LIMIT 1
         ) AS trashed_at`
      : `, NULL AS trashed_at`;

  const rows = db
    .prepare(
      `SELECT h.raw AS handle,
              h.handle_type AS handle_type,
              MAX(p.name_alias) AS name_alias,
              COUNT(m.id) AS message_count,
              MIN(substr(m.timestamp, 1, 10)) AS date_start,
              MAX(substr(m.timestamp, 1, 10)) AS date_end
              ${trashedAtSelect}
       FROM conversations c
       JOIN handles h ON h.id = c.chat_handle_id
       JOIN messages m ON m.conversation_id = c.id
       LEFT JOIN participants p
         ON p.conversation_id = c.id AND p.handle_id = c.chat_handle_id
       WHERE c.account_id = ?
         AND c.conversation_type = 'individual'
         AND NOT EXISTS (
           SELECT 1 FROM contact_handles cp
           WHERE cp.handle_id = c.chat_handle_id AND cp.account_id = c.account_id
         )
         ${trashFilter}${hideDupes}
       GROUP BY c.id
       HAVING message_count > 0
       ORDER BY ${section === "trash" ? "trashed_at DESC, " : ""}handle COLLATE NOCASE`,
    )
    .all(accountId) as Array<{
    handle: string;
    handle_type: string | null;
    name_alias: string | null;
    message_count: number;
    date_start: string | null;
    date_end: string | null;
    trashed_at: string | null;
  }>;

  return rows
    .map((r) => {
      const hintUseful = usefulNameAlias(r.name_alias, r.handle);
      const displayName = hintUseful ?? formatPhoneDisplay(r.handle);
      const sortKey = hintUseful ? `${hintUseful}\0${r.handle}` : r.handle;
      const ch = (hintUseful ?? r.handle).charAt(0).toUpperCase();
      const letter = ch >= "A" && ch <= "Z" ? ch : "#";
      return {
        handle: r.handle,
        handleType: (r.handle_type as HandleType | null) ?? null,
        displayName,
        nameAlias: hintUseful,
        messageCount: r.message_count,
        dateStart: r.date_start,
        dateEnd: r.date_end,
        sortKey,
        letter,
        unverified: Boolean(hintUseful),
        ...(r.trashed_at ? { trashedAt: r.trashed_at } : {}),
      };
    })
    .sort((a, b) => {
      if (section === "trash") {
        const at = (b.trashedAt ?? "").localeCompare(a.trashedAt ?? "");
        if (at !== 0) return at;
      }
      return a.sortKey.localeCompare(b.sortKey, undefined, {
        sensitivity: "base",
      });
    });
}

export function unassignedThreadsBundle(
  handle: string,
  source?: string | null,
  opts?: { includeTrashed?: boolean; handleType?: HandleType | null },
): {
  handle: string;
  handleType: HandleType | null;
  yearly: YearThread[];
  groupChats: GroupChatThread[];
  messageSources: string[];
  sourceCounts: ContactSourceCounts;
} | null {
  const accountId = currentAccountId();
  const trimmed = handle.trim();
  if (!trimmed) return null;
  const db = getDb();
  // A raw can in theory exist under several handle types; the list views know
  // the type, so scope the conversation lookup to it when provided.
  const typeFilter = opts?.handleType ? ` AND h.handle_type = ?` : "";
  const params: unknown[] = [accountId, trimmed];
  if (opts?.handleType) params.push(opts.handleType);
  const conv = db
    .prepare(
      `SELECT c.id AS id, h.handle_type AS handle_type
       FROM conversations c
       JOIN handles h ON h.id = c.chat_handle_id
       WHERE c.account_id = ? AND c.conversation_type = 'individual' AND h.raw = ?${typeFilter}`,
    )
    .get(...params) as { id: number; handle_type: string } | undefined;
  if (!conv) return null;

  const owned = db
    .prepare(
      `SELECT 1 AS ok
       FROM contact_handles cp
       JOIN handles h ON h.id = cp.handle_id
       WHERE cp.account_id = ? AND h.raw = ?`,
    )
    .get(accountId, trimmed) as { ok: number } | undefined;
  if (!opts?.includeTrashed && owned) return null;

  const hasMsgs = db
    .prepare(
      `SELECT 1 AS ok FROM messages WHERE conversation_id = ? LIMIT 1`,
    )
    .get(conv.id) as { ok: number } | undefined;
  if (!hasMsgs) return null;

  const phones = [trimmed];
  const groupChats = contactGroupChatThreadsForPhones(phones, source);
  const individualIds = [conv.id];
  const groupConvIds = groupChats.flatMap((g) =>
    g.conversationIds?.length > 0 ? g.conversationIds : [g.conversationId],
  );
  const allConvIds = [...new Set([...individualIds, ...groupConvIds])];
  const sourceCounts =
    contactMessageSourceCountsForConversations(individualIds);
  const anySourceCounts =
    contactMessageSourceCountsForConversations(allConvIds);

  return {
    handle: trimmed,
    handleType: (conv.handle_type as HandleType | null) ?? null,
    yearly: contactYearlyThreadsForPhones(phones, source, {
      includeTrashed: opts?.includeTrashed,
    }),
    groupChats,
    messageSources: Object.keys(anySourceCounts.bySource).sort(),
    sourceCounts,
  };
}
