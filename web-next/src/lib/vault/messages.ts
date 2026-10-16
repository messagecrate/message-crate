/**
 * Messages of one or more conversations, read from
 * `GET /v1/conversations/{id}/messages`. Replaces `messagesRead.ts`.
 *
 * The vault pages ascending by offset. web-next's thread view wants the
 * newest page first and an opaque cursor for older pages, so the cursor here
 * records, per conversation, how many messages counted from the newest end
 * have already been handed out.
 */
import {
  DEFAULT_MESSAGE_PAGE_SIZE,
  MAX_MESSAGE_PAGE_SIZE,
} from "@/lib/messagePageSize";
import type { AttachmentRow, MessageRow } from "@/lib/types";

import { loadProfile, ownerDisplayName } from "./account";
import {
  MAX_PAGE,
  mapPool,
  vaultAll,
  vaultPage,
  type Schemas,
} from "./client";
import { getConversation, type Participant } from "./conversations";
import { displayName, preferredNameOf } from "./names";

export { DEFAULT_MESSAGE_PAGE_SIZE, MAX_MESSAGE_PAGE_SIZE };

export type Message = Schemas["Message"];

export type MessagePage = {
  messages: MessageRow[];
  /** Cursor for the next older page; null when the oldest page is loaded. */
  nextOlderCursor: string | null;
  hasOlder: boolean;
};

const POOL = 6;

function toAttachment(a: Schemas["Attachment"], index: number): AttachmentRow {
  // `assetsPath` carries the sha256 so `/api/assets/{source}/{sha256}` can
  // proxy `GET /v1/assets/{sha256}`. The vault has no transcoded variant.
  return {
    id: index,
    mimeType: a.mime_type ?? null,
    originalName: a.original_name ?? null,
    assetsPath: a.sha256 ?? null,
    sha256: a.sha256 ?? null,
    previewMimeType: null,
    previewAssetsPath: null,
    previewSha256: null,
  };
}

function senderName(
  m: Message,
  owner: string,
  participants: Participant[],
): string {
  if (m.is_from_me) return owner;
  const sender = m.sender ?? null;
  const participant = participants.find((p) => p.identity === sender);
  if (participant) {
    return displayName(preferredNameOf(participant.name), sender) === "Unknown"
      ? participant.name
      : displayName(preferredNameOf(participant.name), sender);
  }
  return displayName(null, sender);
}

export function toMessageRow(m: Message, owner: string): MessageRow {
  return {
    id: m.id,
    conversationId: m.conversation.id,
    source: m.source,
    timestamp: m.timestamp,
    isFromMe: m.is_from_me,
    sender: m.sender ?? null,
    senderName: senderName(m, owner, m.conversation.participants),
    body: m.text ?? null,
    isAnnouncement: m.is_announcement,
    attachments: m.attachments.map(toAttachment),
  };
}

function byTimeAsc(a: MessageRow, b: MessageRow): number {
  return a.timestamp.localeCompare(b.timestamp) || a.id - b.id;
}

function idList(conversationIds: number | number[]): number[] {
  return (Array.isArray(conversationIds) ? conversationIds : [conversationIds])
    .filter((id) => Number.isFinite(id));
}

function keepSource(rows: MessageRow[], source?: string | null): MessageRow[] {
  return source ? rows.filter((r) => r.source === source) : rows;
}

/**
 * Every message of the conversations in one calendar year, oldest first. A
 * conversation's message list takes no filter, so a year inside one is the
 * search `in:#id date:YYYY` on `GET /v1/messages`.
 */
export async function messagesForConversationYear(
  conversationIds: number | number[],
  year: number,
  source?: string | null,
): Promise<MessageRow[]> {
  const owner = ownerDisplayName(await loadProfile());
  const ids = idList(conversationIds);
  const pages = await mapPool(ids, POOL, (id) =>
    vaultAll<Message>("/v1/messages", {
      q: `in:#${id} date:${year}`,
      sort: "date",
    }),
  );
  const rows = pages.flat().map((m) => toMessageRow(m, owner));
  return keepSource(rows, source).sort(byTimeAsc);
}

/** Every message of the conversations, newest first. */
export async function messagesForConversations(
  conversationIds: number | number[],
  source?: string | null,
): Promise<MessageRow[]> {
  const owner = ownerDisplayName(await loadProfile());
  const ids = idList(conversationIds);
  const pages = await mapPool(ids, POOL, (id) =>
    vaultAll<Message>(`/v1/conversations/${id}/messages`),
  );
  const rows = pages.flat().map((m) => toMessageRow(m, owner));
  return keepSource(rows, source).sort(byTimeAsc).reverse();
}

/** Per conversation: how many messages from the newest end are already out. */
type Cursor = Record<string, number>;

export function encodeCursor(cursor: Cursor): string {
  return Buffer.from(JSON.stringify(cursor), "utf8").toString("base64url");
}

export function decodeCursor(raw: string): Cursor | null {
  try {
    const parsed = JSON.parse(
      Buffer.from(raw, "base64url").toString("utf8"),
    ) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return null;
    }
    const out: Cursor = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v !== "number" || !Number.isFinite(v) || v < 0) return null;
      out[k] = v;
    }
    return out;
  } catch {
    return null;
  }
}

/**
 * Newest page of messages in chronological order, with a cursor for older
 * pages. Pass `before` (a previous `nextOlderCursor`) to continue.
 */
export async function messagesPageForConversations(
  conversationIds: number | number[],
  opts?: { source?: string | null; before?: string | null; limit?: number },
): Promise<MessagePage> {
  const limit = Math.min(
    Math.max(1, opts?.limit ?? DEFAULT_MESSAGE_PAGE_SIZE),
    MAX_MESSAGE_PAGE_SIZE,
    MAX_PAGE,
  );
  const ids = idList(conversationIds);
  const cursor: Cursor = (opts?.before && decodeCursor(opts.before)) || {};
  const owner = ownerDisplayName(await loadProfile());

  // One window of `limit` messages from the newest unread end of each
  // conversation; the merged newest `limit` become the page.
  const windows = await mapPool(ids, POOL, async (id) => {
    const summary = await getConversation(id);
    const total = summary?.message_count ?? 0;
    const taken = cursor[String(id)] ?? 0;
    const remaining = Math.max(0, total - taken);
    if (remaining === 0) return { id, total, taken, rows: [] as MessageRow[] };
    const size = Math.min(limit, remaining);
    const page = await vaultPage<Message>(`/v1/conversations/${id}/messages`, {
      limit: size,
      offset: remaining - size,
    });
    return {
      id,
      total,
      taken,
      rows: page.items.map((m) => toMessageRow(m, owner)),
    };
  });

  const merged = windows
    .flatMap((w) => w.rows)
    .sort(byTimeAsc)
    .reverse();
  const page = merged.slice(0, limit);
  const next: Cursor = { ...cursor };
  for (const w of windows) {
    const consumed = page.filter((r) => r.conversationId === w.id).length;
    next[String(w.id)] = w.taken + consumed;
  }
  const hasOlder = windows.some((w) => w.total > (next[String(w.id)] ?? 0));
  return {
    messages: keepSource([...page].sort(byTimeAsc), opts?.source),
    nextOlderCursor: hasOlder ? encodeCursor(next) : null,
    hasOlder,
  };
}
