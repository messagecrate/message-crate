/**
 * Home dashboard counts, assembled from `/v1` list totals and the account's
 * storage.
 */
import type { HomeStats } from "@/lib/types";

import { listSources, loadStorageUsage } from "./account";
import { dayOf, vaultCount } from "./client";
import { listContacts } from "./contacts";
import { allConversations } from "./conversations";

/** Messages matching `q`: the `total` of `GET /v1/messages`. */
async function countMessages(q: string): Promise<number> {
  return vaultCount("/v1/messages", { q });
}

export async function homeStats(): Promise<HomeStats> {
  const [contacts, conversations, sources, messages, sentMessages, storage] =
    await Promise.all([
      listContacts("all"),
      allConversations(),
      listSources(),
      countMessages(""),
      countMessages("from:me"),
      loadStorageUsage(),
    ]);

  let dateStart: string | null = null;
  let dateEnd: string | null = null;
  for (const c of conversations) {
    const start = dayOf(c.first_message_at);
    const end = dayOf(c.last_message_at);
    if (start && (!dateStart || start < dateStart)) dateStart = start;
    if (end && (!dateEnd || end > dateEnd)) dateEnd = end;
  }

  const recentContacts = contacts
    .filter((c): c is typeof c & { dateEnd: string } => c.dateEnd != null)
    .sort(
      (a, b) =>
        b.dateEnd.localeCompare(a.dateEnd) || b.messageCount - a.messageCount,
    )
    .slice(0, 6)
    .map((c) => ({
      id: c.id,
      displayName: c.displayName,
      messageCount: c.messageCount,
      groupChatCount: c.groupMessageCount,
      dateEnd: c.dateEnd,
    }));

  return {
    all: contacts.length,
    noMessages: contacts.filter(
      (c) => c.messageCount === 0 && c.groupMessageCount === 0,
    ).length,
    groupChats: conversations.filter((c) => c.is_group).length,
    messages,
    // Hidden cross-source duplicates are not reported by the API.
    messageDuplicates: 0,
    contacts: contacts.length,
    sentMessages,
    receivedMessages: Math.max(0, messages - sentMessages),
    attachments: storage.attachmentCount,
    sources: sources.length,
    dateStart,
    dateEnd,
    recentContacts,
  };
}
