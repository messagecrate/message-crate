import {
  collapseGroupConversations,
  groupYearRowsToCollapseThreads,
  type CollapsedGroupConversation,
} from "./groupChatList";
import type { IdentityType } from "./handleKind";
import type {
  GroupYearRow,
  TrashedContactItem,
  TrashedContactMessagesItem,
  UnassignedHandle,
} from "./types";

export type TrashTab = "contacts" | "group-messages";

export type TrashCategory = "contacts" | "groupMessages";

export type TrashListItem =
  | {
      key: string;
      category: "contacts";
      trashKind: "contact" | "messages_only" | "unassigned";
      contactId?: number;
      handle: string;
      /** Handle identity type (null when unknown — e.g. contact rows). */
      handleType?: IdentityType | null;
      displayName: string;
      messageCount: number;
      trashedAt: string;
      unverified?: boolean;
    }
  | {
      key: string;
      category: "groupMessages";
      conversationId: number;
      displayName: string;
      messageCount: number;
      trashedAt: string;
      group: CollapsedGroupConversation;
    };

export const TRASH_CATEGORY_LABEL: Record<TrashCategory, string> = {
  contacts: "Contacts",
  groupMessages: "Group Messages",
};

/** @deprecated Prefer `useDateTimeFormat().formatDateTime` in UI. */
export function formatTrashedAt(trashedAt: string): string {
  const m = trashedAt.match(/^(\d{4}-\d{2}-\d{2})[ T](\d{2}:\d{2})/);
  if (m) return `${m[1]} ${m[2]}`;
  return trashedAt;
}

function byTrashedAtDesc(a: string, b: string): number {
  return b.localeCompare(a);
}

export function parseTrashTab(raw: string | null | undefined): TrashTab {
  return raw === "group-messages" ? "group-messages" : "contacts";
}

export function buildTrashListItems(input: {
  handles: UnassignedHandle[];
  contacts: TrashedContactItem[];
  messagesOnly: TrashedContactMessagesItem[];
  groupChats: GroupYearRow[];
}): TrashListItem[] {
  const items: TrashListItem[] = [];

  for (const c of input.contacts) {
    const handle = c.preferredHandle;
    if (!handle) continue;
    items.push({
      key: `c:${c.contactId}`,
      category: "contacts",
      trashKind: "contact",
      contactId: c.contactId,
      handle,
      displayName: c.displayName,
      messageCount: c.messageCount,
      trashedAt: c.trashedAt,
    });
  }

  for (const m of input.messagesOnly) {
    items.push({
      key: `m:${m.handle}`,
      category: "contacts",
      trashKind: "messages_only",
      contactId: m.contactId,
      handle: m.handle,
      displayName: m.displayName,
      messageCount: m.messageCount,
      trashedAt: m.trashedAt,
    });
  }

  for (const h of input.handles) {
    items.push({
      key: `u:${h.handle}`,
      category: "contacts",
      trashKind: "unassigned",
      handle: h.handle,
      handleType: h.handleType ?? null,
      displayName: h.displayName,
      messageCount: h.messageCount,
      trashedAt: h.trashedAt ?? "",
      unverified: h.unverified,
    });
  }

  const collapsed = collapseGroupConversations(
    groupYearRowsToCollapseThreads(input.groupChats),
  );
  const trashedAtByConv = new Map<number, string>();
  for (const row of input.groupChats) {
    if (!row.trashedAt) continue;
    const prev = trashedAtByConv.get(row.id);
    if (!prev || row.trashedAt > prev) trashedAtByConv.set(row.id, row.trashedAt);
  }
  for (const g of collapsed) {
    items.push({
      key: `g:${g.conversationId}`,
      category: "groupMessages",
      conversationId: g.conversationId,
      displayName: g.namedTitle?.trim() || g.title || "Group message",
      messageCount: g.messageCount,
      trashedAt: trashedAtByConv.get(g.conversationId) ?? "",
      group: g,
    });
  }

  return items;
}

export function itemsForTrashTab(
  items: TrashListItem[],
  tab: TrashTab,
): TrashListItem[] {
  const category: TrashCategory =
    tab === "group-messages" ? "groupMessages" : "contacts";
  return items
    .filter((i) => i.category === category)
    .sort((a, b) => {
      const byDate = byTrashedAtDesc(a.trashedAt, b.trashedAt);
      if (byDate !== 0) return byDate;
      return a.displayName.localeCompare(b.displayName, undefined, {
        sensitivity: "base",
      });
    });
}

export function countTrashTabs(items: TrashListItem[]): {
  contacts: number;
  groupMessages: number;
} {
  let contacts = 0;
  let groupMessages = 0;
  for (const i of items) {
    if (i.category === "contacts") contacts += 1;
    else groupMessages += 1;
  }
  return { contacts, groupMessages };
}

export function filterTrashListItems(
  items: TrashListItem[],
  query: string,
): TrashListItem[] {
  const q = query.trim().toLowerCase();
  if (!q) return items;
  return items.filter((item) => {
    const hay = [item.displayName, item.key];
    if (item.category === "contacts") {
      hay.push(item.handle);
    }
    return hay.join("\0").toLowerCase().includes(q);
  });
}
