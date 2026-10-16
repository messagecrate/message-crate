/**
 * Conversations as `/v1/conversations` lists them, shaped into the group
 * chat rows web-next's screens draw. Replaces `groupChatsRead.ts` for reads.
 *
 * The list is fetched whole (paged by 500) and cached for a few seconds,
 * because most screens need every row: the group list buckets by year, the
 * contact list needs per-contact message totals, and home stats need both.
 */
import type { GroupChatThread, GroupParticipant, GroupYearRow } from "@/lib/types";

import { loadProfile } from "./account";
import {
  dayOf,
  mapPool,
  memo,
  vaultAll,
  vaultCount,
  vaultJsonOrNull,
  yearIn,
  yearOf,
  type Schemas,
} from "./client";
import {
  formatPeopleTitle,
  handleTypeOf,
  isGenericGroupTitle,
  preferredNameOf,
} from "./names";

export type Conversation = Schemas["ConversationSummary"];
export type Participant = Schemas["Participant"];
export type ConversationSource = Schemas["ConversationSource"];

const LIST_TTL_MS = 5_000;
/** Concurrent per-year count calls when splitting a conversation by year. */
const COUNT_POOL = 8;

export type ConversationSection = "active" | "trash";

/** Every conversation of the account, active or trashed. */
export async function allConversations(
  section: ConversationSection = "active",
): Promise<Conversation[]> {
  return memo(`conversations:${section}`, LIST_TTL_MS, () =>
    vaultAll<Conversation>("/v1/conversations", {
      q: section === "trash" ? "trashed:yes" : "",
    }),
  );
}

export async function getConversation(id: number): Promise<Conversation | null> {
  return vaultJsonOrNull<Conversation>(`/v1/conversations/${id}`);
}

export async function conversationSources(
  id: number,
): Promise<ConversationSource[]> {
  return vaultAll<ConversationSource>(`/v1/conversations/${id}/sources`);
}

/** Contact ids of every participant that has one. */
export function participantContactIds(c: Conversation): number[] {
  const ids = new Set<number>();
  for (const p of c.participants) {
    if (p.contact_id != null) ids.add(p.contact_id);
  }
  return [...ids];
}

/** The one contact a direct conversation is with, when the handle has one. */
export function directContactId(c: Conversation): number | null {
  if (c.is_group) return null;
  return c.participants[0]?.contact_id ?? null;
}

export function toGroupParticipant(p: Participant): GroupParticipant {
  const handle = p.identity ?? "";
  return {
    name: p.name,
    handle,
    handleType: handleTypeOf(handle),
    contactId: p.contact_id ?? null,
  };
}

/** Title the group list shows: the export's label, else the people in it. */
export function groupTitle(c: Conversation): {
  title: string;
  titleFull: string;
  namedTitle: string | null;
} {
  const label = c.shown_title?.trim() || null;
  const people = formatPeopleTitle(c.participants.map((p) => p.name));
  if (label && !isGenericGroupTitle(label)) {
    return { title: label, titleFull: people.full || label, namedTitle: label };
  }
  const fallback = people.short || `Conversation #${c.id}`;
  return { title: fallback, titleFull: people.full || fallback, namedTitle: null };
}

/**
 * Calendar years a conversation's messages span, oldest first, in the
 * account's time zone: the zone the server reads `date:YYYY` in.
 */
export function conversationYears(c: Conversation, timeZone: string): number[] {
  const start =
    yearIn(c.first_message_at, timeZone) ?? yearIn(c.last_message_at, timeZone);
  const end = yearIn(c.last_message_at, timeZone);
  if (start == null || end == null) return [];
  const years: number[] = [];
  for (let y = Math.min(start, end); y <= Math.max(start, end); y += 1) {
    years.push(y);
  }
  return years;
}

/**
 * The search for one conversation's messages in one year. A search leaves
 * the trash out unless asked, and a trashed conversation can still be
 * opened, so the query asks (`trashed:any`), as web/ does.
 */
export function conversationYearQuery(conversationId: number, year: number): string {
  return `in:#${conversationId} trashed:any date:${year}`;
}

/**
 * Messages of one conversation in one year that also match `extra`: the
 * `total` of `GET /v1/messages` for {@link conversationYearQuery}.
 */
export async function countConversationYear(
  conversationId: number,
  year: number,
  extra = "",
): Promise<number> {
  return vaultCount("/v1/messages", {
    q: `${conversationYearQuery(conversationId, year)} ${extra}`.trim(),
  });
}

/**
 * Per-year message counts for a conversation. A conversation inside one
 * calendar year costs nothing extra; one that spans years costs one count
 * call per year.
 */
export async function yearCounts(
  c: Conversation,
  timeZone: string,
): Promise<Array<{ year: number; messages: number }>> {
  const years = conversationYears(c, timeZone);
  if (years.length === 0) return [];
  if (years.length === 1) {
    return [{ year: years[0]!, messages: c.message_count }];
  }
  const counts = await mapPool(years, COUNT_POOL, (year) =>
    countConversationYear(c.id, year),
  );
  return years
    .map((year, i) => ({ year, messages: counts[i]! }))
    .filter((row) => row.messages > 0);
}

/** The day of a conversation's first message, else of its last. */
export function conversationStartDay(c: Conversation): string {
  return dayOf(c.first_message_at) || dayOf(c.last_message_at);
}

/** Clip a conversation's date range to one calendar year. */
export function yearRange(
  c: Conversation,
  year: number,
): { dateStart: string; dateEnd: string } {
  const start = conversationStartDay(c);
  const end = dayOf(c.last_message_at);
  const first = `${year}-01-01`;
  const last = `${year}-12-31`;
  return {
    dateStart: start > first ? start : first,
    dateEnd: end < last ? end : last,
  };
}

function groupYearRow(
  c: Conversation,
  year: number,
  messages: number,
  years: number[],
): GroupYearRow {
  const { title, titleFull, namedTitle } = groupTitle(c);
  const participants = c.participants.map(toGroupParticipant);
  const { dateStart, dateEnd } = yearRange(c, year);
  return {
    id: c.id,
    year,
    title,
    titleFull,
    namedTitle,
    participantCount: participants.length,
    participantNames: participants.map((p) => p.name),
    participantHandles: participants.map((p) => p.handle),
    participants,
    messageCount: messages,
    dateStart,
    dateEnd,
    conversationDateStart: dayOf(c.first_message_at) || dateStart,
    conversationDateEnd: dayOf(c.last_message_at) || dateEnd,
    spansMultipleYears: years.length > 1,
  };
}

/** Group chats split by calendar year for the Group Messages page. */
export async function listGroupYearRows(
  section: ConversationSection = "active",
): Promise<GroupYearRow[]> {
  const [conversations, profile] = await Promise.all([
    allConversations(section),
    loadProfile(),
  ]);
  const groups = conversations.filter((c) => c.is_group);
  const rows = await mapPool(groups, COUNT_POOL, async (c) => {
    const years = conversationYears(c, profile.time_zone);
    const counts = await yearCounts(c, profile.time_zone);
    return counts.map((count) =>
      groupYearRow(c, count.year, count.messages, years),
    );
  });
  return rows
    .flat()
    .sort((a, b) => b.dateEnd.localeCompare(a.dateEnd) || b.id - a.id);
}

/** One group conversation as the contact page lists it (newest year). */
export function groupChatThread(c: Conversation): GroupChatThread {
  const { title, titleFull, namedTitle } = groupTitle(c);
  const participants = c.participants.map(toGroupParticipant);
  const year = yearOf(c.last_message_at) ?? new Date().getFullYear();
  return {
    conversationId: c.id,
    conversationIds: [c.id],
    title,
    titleFull,
    namedTitle,
    participantCount: participants.length,
    participantNames: participants.map((p) => p.name),
    participantHandles: participants.map((p) => p.handle),
    participants,
    year,
    messageCount: c.message_count,
    dateStart: conversationStartDay(c),
    dateEnd: dayOf(c.last_message_at),
  };
}

/** Group chats that include every listed contact (extra participants allowed). */
export async function groupChatsContainingContacts(
  contactIds: number[],
): Promise<GroupChatThread[]> {
  const wanted = [...new Set(contactIds.filter((id) => Number.isFinite(id)))];
  if (!wanted.length) return [];
  const groups = (await allConversations()).filter((c) => c.is_group);
  return groups
    .filter((c) => {
      const present = new Set(participantContactIds(c));
      return wanted.every((id) => present.has(id));
    })
    .map(groupChatThread)
    .sort((a, b) => b.dateEnd.localeCompare(a.dateEnd));
}

export type ContactStats = {
  /** 1:1 message total across the contact's direct conversations. */
  messageCount: number;
  /** Distinct group conversations the contact takes part in. */
  groupCount: number;
  dateStart: string | null;
  dateEnd: string | null;
  directConversationIds: number[];
};

/** Per-contact totals derived from the conversation list, one pass. */
export async function contactStats(): Promise<Map<number, ContactStats>> {
  return memo("contact-stats", LIST_TTL_MS, async () => {
    const out = new Map<number, ContactStats>();
    const entry = (id: number): ContactStats => {
      let s = out.get(id);
      if (!s) {
        s = {
          messageCount: 0,
          groupCount: 0,
          dateStart: null,
          dateEnd: null,
          directConversationIds: [],
        };
        out.set(id, s);
      }
      return s;
    };
    for (const c of await allConversations()) {
      if (c.is_group) {
        for (const id of participantContactIds(c)) entry(id).groupCount += 1;
        continue;
      }
      const id = directContactId(c);
      if (id == null) continue;
      const s = entry(id);
      s.messageCount += c.message_count;
      s.directConversationIds.push(c.id);
      const start = dayOf(c.first_message_at);
      const end = dayOf(c.last_message_at);
      if (start && (!s.dateStart || start < s.dateStart)) s.dateStart = start;
      if (end && (!s.dateEnd || end > s.dateEnd)) s.dateEnd = end;
    }
    return out;
  });
}

/** The vault names nameless contacts "(unknown)"; treat that as no name. */
export function participantPreferredName(p: Participant): string | null {
  return preferredNameOf(p.name);
}
