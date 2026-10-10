import { countOf } from "../../lib/plural";
import type { components } from "../../lib/serverApi.types";

/** One identity as the server lists it, with its messages. */
export type Identity = components["schemas"]["Identity"];

/**
 * "12 direct messages and 30 group messages", "1 direct message", or null when
 * there are none. Orphaned messages are named only when there are some.
 */
export function messagesPhrase(identity: Identity): string | null {
  const parts: string[] = [];
  const count = (n: number, kind: string) => countOf(n, `${kind} message`);
  if (identity.direct_messages > 0) parts.push(count(identity.direct_messages, "direct"));
  if (identity.group_messages > 0) parts.push(count(identity.group_messages, "group"));
  if (identity.orphaned_messages > 0) parts.push(count(identity.orphaned_messages, "orphaned"));
  if (parts.length === 0) return null;
  if (parts.length === 1) return parts[0];
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/** What the confirm dialog says an identity is tied to. */
export function removeBody(identity: Identity): string {
  const phrase = messagesPhrase(identity);
  return phrase
    ? `${phrase} will no longer be associated with this account.`
    : `${identity.address} has no messages. It will no longer count as this account's own.`;
}
