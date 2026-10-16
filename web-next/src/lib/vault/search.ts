/**
 * Search through the vault's one search language. web-next keeps its own
 * parser (`searchQuery.ts`) for the presentation words it understands
 * (`group:none`, `sort:`, `context:`, `search:contacts`); the filters it
 * parsed are re-spelled in the vault's words and sent to
 * `GET /v1/conversations`, `GET /v1/export/messages`, or `GET /v1/contacts`.
 *
 * Replaces the reads in `search.ts`. The result types are re-exported from
 * there unchanged so the components keep compiling.
 */
import {
  hasSearchCriteria,
  parseSearchQuery,
  type DateBounds,
  type ParsedSearchQuery,
} from "@/lib/searchQuery";
import type {
  ConversationMatchResult,
  SearchContactHit,
  SearchConversationHit,
  SearchHitMessage,
  SearchMessageHit,
  SearchResult,
} from "@/lib/search";

import { loadProfile, ownerDisplayName } from "./account";
import {
  mapPool,
  qs,
  vaultAll,
  vaultPage,
  type Schemas,
} from "./client";
import { listContactsByIds } from "./contacts";
import { groupTitle, type Conversation } from "./conversations";
import type { Message } from "./messages";
import { formatPeopleTitle } from "./names";

export type {
  ConversationMatch,
  ConversationMatchResult,
  SearchAttachmentSummary,
  SearchContactHit,
  SearchConversationHit,
  SearchHitMessage,
  SearchMessageHit,
  SearchResult,
} from "@/lib/search";

const DEFAULT_LIMIT = 100;
const MAX_LIMIT = 100;
/** Messages read when grouping a content search by conversation. */
const MAX_PAGE_MESSAGES = 500;
const SNIPPET_CHARS = 200;
const TOP_MATCH_POOL = 6;

type List = "contacts" | "conversations" | "messages";

/** The vault's `source:` word takes a backup family, not an import source id. */
const SOURCE_FAMILIES = new Set(["imessage", "whatsapp", "sms"]);

function quote(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  return /[\s"()]/.test(trimmed) ? `"${trimmed.replace(/"/g, "")}"` : trimmed;
}

function dateWord(word: string, bounds: DateBounds): string | null {
  if (bounds.from && bounds.to) return `${word}:${bounds.from}..${bounds.to}`;
  if (bounds.from) return `${word}:>=${bounds.from}`;
  if (bounds.to) return `${word}:<${bounds.to}`;
  return null;
}

function countWord(
  word: string,
  cmp: ParsedSearchQuery["messageCount"],
): string | null {
  if (!cmp) return null;
  const op = cmp.comparator === "=" ? "" : cmp.comparator;
  return `${word}:${op}${cmp.value}`;
}

/**
 * Re-spell a parsed web-next query in the vault's words for one list. Words
 * the vault has no counterpart for are dropped; the gap list records them.
 */
export function vaultQuery(parsed: ParsedSearchQuery, list: List): string {
  const words: string[] = [];
  const text: string[] = [];
  for (const term of parsed.terms) text.push(term);
  for (const phrase of parsed.phrases) text.push(`"${phrase.replace(/"/g, "")}"`);
  if (text.length && list !== "contacts") words.push(text.join(" "));

  if (list !== "contacts") {
    if (parsed.subject) words.push(`subject:${quote(parsed.subject)}`);
    if (parsed.text) words.push(`body:${quote(parsed.text)}`);
    if (parsed.with) words.push(`with:${quote(parsed.with)}`);
    if (parsed.hasAttachment === true) words.push("attachment:any");
    if (parsed.hasAttachment === false) words.push("attachment:none");
    if (parsed.filetype) words.push(`attachment:${parsed.filetype}`);
    if (parsed.filename) words.push(`filename:${quote(parsed.filename)}`);
    if (parsed.largerBytes != null) words.push(`size:>${parsed.largerBytes}`);
    if (parsed.smallerBytes != null) words.push(`size:<${parsed.smallerBytes}`);
    if (parsed.source && SOURCE_FAMILIES.has(parsed.source.toLowerCase())) {
      words.push(`source:${parsed.source.toLowerCase()}`);
    }
    if (parsed.conversationType === "group") words.push("kind:group");
    if (parsed.conversationType === "one-to-one") words.push("kind:direct");
    const date = dateWord("date", { from: parsed.after, to: parsed.before });
    if (date) words.push(date);
    if (parsed.inConversation) {
      words.push(
        list === "messages"
          ? `in:${quote(parsed.inConversation)}`
          : `title:${quote(parsed.inConversation)}`,
      );
    }
  }
  if (list === "messages") {
    if (parsed.from) words.push(`from:${quote(parsed.from)}`);
    if (parsed.to) words.push(`to:${quote(parsed.to)}`);
  }
  if (list === "contacts") {
    const name = [parsed.firstName, parsed.lastName].filter(Boolean).join(" ");
    if (name) words.push(`name:${quote(name)}`);
    if (parsed.handle) words.push(`handle:${quote(parsed.handle)}`);
    if (parsed.phone) words.push(`handle:${quote(parsed.phone)}`);
    if (text.length) words.push(`name:${quote(text.join(" "))}`);
    const groups = countWord("groups", parsed.groupCount);
    if (groups) words.push(groups);
    const messages = countWord("messages", parsed.messageCount);
    if (messages) words.push(messages);
    const first = dateWord("first-message", parsed.firstContact);
    if (first) words.push(first);
    const last = dateWord("last-message", parsed.lastContact);
    if (last) words.push(last);
  }
  if (parsed.within) words.push(`group:${quote(parsed.within)}`);
  return words.join(" ").trim();
}

function snippet(text: string | null | undefined): string {
  const t = (text ?? "").replace(/\s+/g, " ").trim();
  return t.length > SNIPPET_CHARS ? `${t.slice(0, SNIPPET_CHARS)}…` : t;
}

function hitMessage(m: Message): SearchHitMessage {
  return {
    id: m.id,
    timestamp: m.timestamp,
    snippet: snippet(m.text),
    isFromMe: m.is_from_me,
    sender: m.sender ?? null,
  };
}

function conversationHit(
  c: Conversation,
  topMatch: SearchHitMessage | null,
): SearchConversationHit {
  const { title } = groupTitle(c);
  const first = c.participants[0];
  return {
    conversationId: c.id,
    conversationType: c.is_group ? "group" : "one-to-one",
    contactId: c.is_group ? null : (first?.contact_id ?? null),
    title: c.is_group ? title : (first?.name ?? title),
    chatIdentifier: first?.handle ?? String(c.id),
    matchCount: c.message_count,
    dateStart: c.date_range_start ?? null,
    dateEnd: c.date_range_end ?? c.last_message_at,
    topMatch,
  };
}

function messageHit(m: Message, owner: string): SearchMessageHit {
  const conv = m.conversation;
  const isGroup = conv.conversation_type === "group";
  const first = conv.participants[0];
  const people = formatPeopleTitle(conv.participants.map((p) => p.name));
  const title = isGroup
    ? conv.group_title?.trim() || people.short || conv.chat_identifier
    : (first?.name ?? conv.chat_identifier);
  return {
    messageId: m.id,
    conversationId: conv.id,
    conversationType: isGroup ? "group" : "one-to-one",
    contactId: isGroup ? null : (first?.contact_id ?? null),
    title,
    chatIdentifier: conv.chat_identifier,
    timestamp: m.timestamp,
    snippet: snippet(m.text),
    isFromMe: m.is_from_me,
    sender: m.is_from_me ? owner : (m.sender ?? null),
    attachments: m.attachments.map((a) => ({
      name: a.original_name ?? null,
      mimeType: a.mime_type ?? null,
      sizeBytes: null,
    })),
    contextBefore: [],
    contextAfter: [],
  };
}

function pageBounds(opts: { limit?: number; offset?: number }) {
  return {
    limit: Math.min(Math.max(1, opts.limit ?? DEFAULT_LIMIT), MAX_LIMIT),
    offset: Math.max(0, opts.offset ?? 0),
  };
}

/** Best matching message of a conversation for preview and scroll-to. */
async function topMatchFor(
  q: string,
  conversationId: number,
): Promise<SearchHitMessage | null> {
  const page = await vaultPage<Message>("/v1/export/messages", {
    q: `${q} in:#${conversationId}`.trim(),
    limit: 1,
  });
  const m = page.items[0];
  return m ? hitMessage(m) : null;
}

async function searchMessages(
  rawQuery: string,
  parsed: ParsedSearchQuery,
  opts: { limit?: number; offset?: number; source?: string | null },
): Promise<SearchResult> {
  const { limit, offset } = pageBounds(opts);
  const owner = ownerDisplayName(await loadProfile());
  const page = await vaultPage<Message>("/v1/export/messages", {
    q: vaultQuery(parsed, "messages"),
    limit,
    offset,
  });
  const messageHits = page.items
    .filter((m) => !opts.source || m.source === opts.source)
    .map((m) => messageHit(m, owner));
  return {
    query: rawQuery,
    parsed,
    totalConversations: 0,
    contactIds: [
      ...new Set(
        messageHits
          .map((h) => h.contactId)
          .filter((id): id is number => id != null),
      ),
    ],
    hits: [],
    messageHits,
    totalMessages: page.total,
  };
}

/**
 * True when the query filters on message content. Free text on the
 * conversation list matches names and titles only, so these queries run on
 * the message list and are grouped by conversation here.
 */
function hasMessageCriteria(parsed: ParsedSearchQuery): boolean {
  return (
    parsed.terms.length > 0 ||
    parsed.phrases.length > 0 ||
    parsed.exclude.length > 0 ||
    !!parsed.subject ||
    !!parsed.text ||
    !!parsed.from ||
    !!parsed.to ||
    parsed.hasAttachment !== null ||
    !!parsed.filetype ||
    !!parsed.filename ||
    parsed.largerBytes != null ||
    parsed.smallerBytes != null
  );
}

/** One page of message hits grouped into conversation hits, newest first. */
async function searchConversationsByMessages(
  rawQuery: string,
  parsed: ParsedSearchQuery,
  opts: { limit?: number; offset?: number; source?: string | null },
): Promise<SearchResult> {
  const { limit, offset } = pageBounds(opts);
  const owner = ownerDisplayName(await loadProfile());
  const page = await vaultPage<Message>("/v1/export/messages", {
    q: vaultQuery(parsed, "messages"),
    limit: MAX_PAGE_MESSAGES,
    offset: 0,
  });
  const grouped = new Map<number, SearchConversationHit>();
  for (const m of page.items) {
    if (opts.source && m.source !== opts.source) continue;
    const hit = messageHit(m, owner);
    const existing = grouped.get(hit.conversationId);
    if (existing) {
      existing.matchCount += 1;
      if (!existing.dateStart || hit.timestamp < existing.dateStart) {
        existing.dateStart = hit.timestamp;
      }
      if (!existing.dateEnd || hit.timestamp > existing.dateEnd) {
        existing.dateEnd = hit.timestamp;
      }
      continue;
    }
    grouped.set(hit.conversationId, {
      conversationId: hit.conversationId,
      conversationType: hit.conversationType,
      contactId: hit.contactId,
      title: hit.title,
      chatIdentifier: hit.chatIdentifier,
      matchCount: 1,
      dateStart: hit.timestamp,
      dateEnd: hit.timestamp,
      topMatch: hitMessage(m),
    });
  }
  const all = [...grouped.values()].sort((a, b) =>
    parsed.sort === "date-asc"
      ? (a.dateEnd ?? "").localeCompare(b.dateEnd ?? "")
      : (b.dateEnd ?? "").localeCompare(a.dateEnd ?? ""),
  );
  const hits = all.slice(offset, offset + limit);
  return {
    query: rawQuery,
    parsed,
    // Grouped from the first MAX_PAGE_MESSAGES matching messages only.
    totalConversations: all.length,
    contactIds: [
      ...new Set(
        hits.map((h) => h.contactId).filter((id): id is number => id != null),
      ),
    ],
    hits,
  };
}

/** Conversation-grouped (default) or per-message (`group:none`) search. */
export async function searchVault(
  rawQuery: string,
  opts: { limit?: number; offset?: number; source?: string | null } = {},
): Promise<SearchResult> {
  const parsed = parseSearchQuery(rawQuery);
  if (parsed.mode !== "contacts" && parsed.groupBy === "none") {
    return searchMessages(rawQuery, parsed, opts);
  }
  const { limit, offset } = pageBounds(opts);
  if (!hasSearchCriteria(parsed)) {
    return {
      query: rawQuery,
      parsed,
      totalConversations: 0,
      contactIds: [],
      hits: [],
    };
  }
  if (hasMessageCriteria(parsed)) {
    return searchConversationsByMessages(rawQuery, parsed, opts);
  }
  const q = vaultQuery(parsed, "conversations");
  const page = await vaultPage<Conversation>("/v1/conversations", {
    q,
    limit,
    offset,
    sort: parsed.sort === "relevance" ? undefined : "date",
    order: parsed.sort === "date-asc" ? "asc" : undefined,
  });
  const messageQuery = vaultQuery(parsed, "messages");
  const topMatches = await mapPool(page.items, TOP_MATCH_POOL, (c) =>
    topMatchFor(messageQuery, c.id).catch(() => null),
  );
  const hits = page.items.map((c, i) => conversationHit(c, topMatches[i] ?? null));
  return {
    query: rawQuery,
    parsed,
    totalConversations: page.total,
    contactIds: [
      ...new Set(
        hits.map((h) => h.contactId).filter((id): id is number => id != null),
      ),
    ],
    hits,
  };
}

/** Contact-primary search (`search:contacts`). */
export async function searchVaultContacts(
  rawQuery: string,
  opts: { limit?: number; offset?: number } = {},
): Promise<SearchResult> {
  const parsed = parseSearchQuery(rawQuery);
  const { limit, offset } = pageBounds(opts);
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
  const page = await vaultPage<Schemas["ContactSummary"]>("/v1/contacts", {
    q: vaultQuery(parsed, "contacts"),
    limit,
    offset,
  });
  const contacts = await listContactsByIds(page.items.map((c) => c.id));
  const contactHits: SearchContactHit[] = contacts.map((contact) => ({
    contact,
    hits: [],
    matchCount: contact.messageCount,
  }));
  return {
    ...empty,
    contactIds: contacts.map((c) => c.id),
    contacts: contactHits,
    totalContacts: page.total,
  };
}

/**
 * Every matching message in the given conversations, oldest first, for the
 * in-thread find bar.
 */
export async function searchConversationMatches(
  rawQuery: string,
  conversationIds: number[],
  opts: { source?: string | null } = {},
): Promise<ConversationMatchResult> {
  const parsed = parseSearchQuery(rawQuery);
  const ids = conversationIds.filter((id) => Number.isInteger(id) && id > 0);
  if (ids.length === 0 || (!hasSearchCriteria(parsed) && !rawQuery.trim())) {
    return { query: rawQuery, parsed, conversationIds: ids, matches: [] };
  }
  const q = vaultQuery(parsed, "messages");
  const pages = await mapPool(ids, TOP_MATCH_POOL, (id) =>
    vaultAll<Message>("/v1/export/messages", { q: `${q} in:#${id}`.trim() }),
  );
  const matches = pages
    .flat()
    .filter((m) => !opts.source || m.source === opts.source)
    .map((m) => ({ id: m.id, timestamp: m.timestamp }))
    .sort((a, b) => a.timestamp.localeCompare(b.timestamp) || a.id - b.id);
  return { query: rawQuery, parsed, conversationIds: ids, matches };
}

/**
 * Neighbouring message ids for `context:N`. The vault has no route for
 * messages around one message, so the hit stands alone.
 */
export async function searchMessageContextIds(
  messageId: number,
): Promise<number[]> {
  return Number.isInteger(messageId) && messageId > 0 ? [messageId] : [];
}

/** Query string a `/v1` list would receive for a raw web-next query. */
export function debugVaultQuery(rawQuery: string, list: List): string {
  return `${vaultQuery(parseSearchQuery(rawQuery), list)}${qs({})}`;
}
