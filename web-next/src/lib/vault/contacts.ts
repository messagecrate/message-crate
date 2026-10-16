/**
 * Contacts as `/v1/contacts` lists them, shaped into web-next's list and
 * detail rows. Replaces `contactsRead.ts` for reads.
 */
import type {
  ContactDetail,
  ContactHandle,
  ContactListItem,
  ContactSection,
  GroupChatThread,
  TrashedContactItem,
  TrashedContactMessagesItem,
  YearThread,
} from "@/lib/types";

import { loadProfile } from "./account";
import {
  dayOf,
  mapPool,
  memo,
  vaultAll,
  vaultJsonOrNull,
  type Schemas,
} from "./client";
import {
  allConversations,
  contactStats,
  conversationSources,
  conversationYears,
  countConversationYear,
  directContactId,
  groupChatThread,
  participantContactIds,
  yearRange,
  type Conversation,
  type ContactStats,
} from "./conversations";
import {
  displayName,
  handleTypeOf,
  nameParts,
  preferredNameOf,
  sortFields,
} from "./names";

type ContactSummary = Schemas["ContactSummary"];
type Contact = Schemas["Contact"];

const LIST_TTL_MS = 5_000;
const COUNT_POOL = 8;

const EMPTY_STATS: ContactStats = {
  messageCount: 0,
  groupCount: 0,
  dateStart: null,
  dateEnd: null,
  directConversationIds: [],
};

async function allContacts(section: "active" | "trash" = "active"): Promise<ContactSummary[]> {
  return memo(`contacts:${section}`, LIST_TTL_MS, () =>
    vaultAll<ContactSummary>("/v1/contacts", {
      q: section === "trash" ? "trashed:yes" : "",
    }),
  );
}

function toListItem(row: ContactSummary, stats: ContactStats): ContactListItem {
  const preferredName = preferredNameOf(row.name);
  const preferredHandle = row.addresses[0] ?? null;
  const { firstName, lastName } = nameParts(preferredName);
  return {
    id: row.id,
    displayName: displayName(preferredName, preferredHandle),
    preferredName,
    preferredHandle,
    handleType: handleTypeOf(preferredHandle),
    firstName,
    lastName,
    ...sortFields(preferredName, preferredHandle),
    labels: row.groups ?? [],
    messageCount: stats.messageCount,
    groupMessageCount: stats.groupCount,
    dateStart: stats.dateStart,
    dateEnd: stats.dateEnd,
    lastModified: row.last_modified,
  };
}

function inSection(item: ContactListItem, section: ContactSection): boolean {
  if (section === "all") return true;
  if (section === "no-messages") {
    return item.messageCount === 0 && item.groupMessageCount === 0;
  }
  if (section === "no-label") return item.labels.length === 0;
  const wanted = section.label.trim().toLowerCase();
  return item.labels.some((l) => l.trim().toLowerCase() === wanted);
}

function byName(a: ContactListItem, b: ContactListItem): number {
  return (
    a.sortLast.localeCompare(b.sortLast, undefined, { sensitivity: "base" }) ||
    a.sortFirst.localeCompare(b.sortFirst, undefined, { sensitivity: "base" }) ||
    a.id - b.id
  );
}

/** Every non-trashed contact in a section, sorted by last name. */
export async function listContacts(section: ContactSection): Promise<ContactListItem[]> {
  const [rows, stats] = await Promise.all([allContacts(), contactStats()]);
  return rows
    .map((row) => toListItem(row, stats.get(row.id) ?? EMPTY_STATS))
    .filter((item) => inSection(item, section))
    .sort(byName);
}

export async function countContacts(section: ContactSection): Promise<number> {
  return (await listContacts(section)).length;
}

/** Contact list items for specific ids, in the given order. */
export async function listContactsByIds(contactIds: number[]): Promise<ContactListItem[]> {
  if (contactIds.length === 0) return [];
  const [rows, stats] = await Promise.all([allContacts(), contactStats()]);
  const byId = new Map(rows.map((row) => [row.id, row]));
  const out: ContactListItem[] = [];
  for (const id of contactIds) {
    const row = byId.get(id);
    if (row) out.push(toListItem(row, stats.get(id) ?? EMPTY_STATS));
  }
  return out;
}

function detailHandles(detail: Contact): ContactHandle[] {
  return detail.identities
    .map((h) => ({
      raw: h.address,
      handle_type: handleTypeOf(h.address) ?? "username",
      service: h.service ?? null,
      normalizedNote: null,
    }))
    .sort((a, b) =>
      a.handle_type === b.handle_type
        ? a.raw.localeCompare(b.raw)
        : a.handle_type === "phone"
          ? -1
          : b.handle_type === "phone"
            ? 1
            : 0,
    );
}

function toDetail(detail: Contact): ContactDetail {
  const preferredName = preferredNameOf(detail.name);
  const handles = detailHandles(detail);
  const preferredHandle = handles[0]?.raw ?? null;
  const { firstName, lastName } = nameParts(preferredName);
  let dateStart: string | null = null;
  let dateEnd: string | null = null;
  let messageCount = 0;
  for (const h of detail.identities) {
    messageCount += h.one_to_one_messages;
    const start = dayOf(h.start_date);
    const end = dayOf(h.end_date);
    if (start && (!dateStart || start < dateStart)) dateStart = start;
    if (end && (!dateEnd || end > dateEnd)) dateEnd = end;
  }
  return {
    id: detail.id,
    displayName: displayName(preferredName, preferredHandle),
    preferredName,
    preferredHandle,
    handleType: handles[0]?.handle_type ?? null,
    firstName,
    lastName,
    ...sortFields(preferredName, preferredHandle),
    labels: detail.groups ?? [],
    messageCount,
    groupMessageCount: detail.group_conversations,
    dateStart,
    dateEnd,
    lastModified: detail.last_modified,
    handles,
    phones: handles.map((h) => h.raw),
  };
}

export async function getContact(id: number): Promise<ContactDetail | null> {
  const detail = await vaultJsonOrNull<Contact>(`/v1/contacts/${id}`);
  return detail ? toDetail(detail) : null;
}

export type ContactSourceCounts = {
  /** Soft-deduped 1:1 total (Combined view). Group chats are listed separately. */
  all: number;
  /** Per-source 1:1 totals (single-source view). */
  bySource: Record<string, number>;
};

/**
 * Year buckets across a contact's direct conversations. One conversation per
 * source can carry the same person, so a year may hold several ids.
 */
async function yearThreads(
  directs: Conversation[],
  timeZone: string,
): Promise<YearThread[]> {
  // One pool across every (conversation, year) pair, so a contact with many
  // conversations still has at most COUNT_POOL pairs counting at once.
  const pairs = directs.flatMap((c) => {
    const years = conversationYears(c, timeZone);
    return years.map((year) => ({ c, year, onlyYear: years.length === 1 }));
  });
  const rows = await mapPool(pairs, COUNT_POOL, async ({ c, year, onlyYear }) => {
    const [messages, attachments] = await Promise.all([
      onlyYear ? c.message_count : countConversationYear(c.id, year),
      countConversationYear(c.id, year, "attachment:any"),
    ]);
    return { c, year, messages, attachments };
  });
  const byYear = new Map<number, YearThread>();
  for (const row of rows) {
    if (row.messages === 0) continue;
    const { dateStart, dateEnd } = yearRange(row.c, row.year);
    const bucket = byYear.get(row.year) ?? {
      year: row.year,
      messageCount: 0,
      attachmentCount: 0,
      dateStart,
      dateEnd,
      conversationIds: [],
    };
    bucket.messageCount += row.messages;
    bucket.attachmentCount += row.attachments;
    if (dateStart < bucket.dateStart) bucket.dateStart = dateStart;
    if (dateEnd > bucket.dateEnd) bucket.dateEnd = dateEnd;
    bucket.conversationIds.push(row.c.id);
    byYear.set(row.year, bucket);
  }
  return [...byYear.values()].sort((a, b) => b.year - a.year);
}

/**
 * Contact detail plus thread metadata in one pass: yearly 1:1 buckets, the
 * group chats the contact is in, and the backup sources behind them.
 *
 * `source` is an import source id (`GET /v1/session`); the vault's list
 * routes cannot filter by it, so the threads are the same for every source
 * and only the per-source counts change.
 */
export async function loadContactThreadsPage(
  contactId: number,
  source?: string | null,
  opts?: { includeTrashed?: boolean },
): Promise<{
  contact: ContactDetail;
  yearly: YearThread[];
  groupChats: GroupChatThread[];
  messageSources: string[];
  sourceCounts: ContactSourceCounts;
} | null> {
  const contact = await getContact(contactId);
  if (!contact) return null;
  void source; // accepted for the route's sake; see the doc comment
  const [conversations, profile] = await Promise.all([
    allConversations(opts?.includeTrashed ? "trash" : "active"),
    loadProfile(),
  ]);
  const directs = conversations.filter(
    (c) => !c.is_group && directContactId(c) === contactId,
  );
  const groups = conversations.filter(
    (c) => c.is_group && participantContactIds(c).includes(contactId),
  );
  const [yearly, sources] = await Promise.all([
    yearThreads(directs, profile.time_zone),
    mapPool(directs, COUNT_POOL, (c) => conversationSources(c.id)),
  ]);
  const bySource: Record<string, number> = {};
  for (const list of sources) {
    for (const info of list) {
      bySource[info.backup_name] =
        (bySource[info.backup_name] ?? 0) + info.message_count;
    }
  }
  return {
    contact,
    yearly,
    groupChats: groups
      .map(groupChatThread)
      .sort((a, b) => b.dateEnd.localeCompare(a.dateEnd)),
    messageSources: Object.keys(bySource).sort(),
    sourceCounts: {
      all: directs.reduce((n, c) => n + c.message_count, 0),
      bySource,
    },
  };
}

export async function contactThreadsBundle(
  contactId: number,
  source?: string | null,
  opts?: { includeTrashed?: boolean },
): Promise<{
  yearly: YearThread[];
  groupChats: GroupChatThread[];
  messageSources: string[];
  sourceCounts: ContactSourceCounts;
}> {
  const page = await loadContactThreadsPage(contactId, source, opts);
  if (!page) {
    return {
      yearly: [],
      groupChats: [],
      messageSources: [],
      sourceCounts: { all: 0, bySource: {} },
    };
  }
  return {
    yearly: page.yearly,
    groupChats: page.groupChats,
    messageSources: page.messageSources,
    sourceCounts: page.sourceCounts,
  };
}

/**
 * Contacts in the trash (`trashed:yes`). The vault does not report when a
 * contact was trashed, so `trashedAt` is empty.
 */
export async function listTrashedContacts(): Promise<TrashedContactItem[]> {
  const rows = await allContacts("trash");
  return rows
    .map((row) => {
      const preferredName = preferredNameOf(row.name);
      const preferredHandle = row.addresses[0] ?? null;
      const { firstName, lastName } = nameParts(preferredName);
      const sort = sortFields(preferredName, preferredHandle);
      return {
        kind: "contact" as const,
        contactId: row.id,
        displayName: displayName(preferredName, preferredHandle),
        preferredHandle,
        handleCount: row.identity_count,
        messageCount: 0,
        sortKey: `${sort.sortLast}\0${sort.sortFirst}`.toLowerCase(),
        letter: sort.letter,
        sortFirst: sort.sortFirst,
        sortLast: sort.sortLast,
        firstName,
        lastName,
        trashedAt: "",
      };
    })
    .sort((a, b) => a.sortKey.localeCompare(b.sortKey));
}

/**
 * "Delete messages only" no longer exists: the vault trashes conversations
 * and contacts, never a handle's messages on their own.
 */
export async function listTrashedContactMessages(): Promise<
  TrashedContactMessagesItem[]
> {
  return [];
}
