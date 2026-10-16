/** What a conversation's name is made from: the fields every conversation the server sends carries. */
type ConversationNameParts = {
  /** The title the server gives the conversation: the export's, or the account's name for a conversation with yourself. */
  shown_title: string | null | undefined;
  /** Whether the conversation is a group: the server's `is_group`. */
  is_group: boolean;
  participants: readonly { name: string }[];
};

/**
 * A conversation's name, wherever it is shown: its title, else the one other
 * person in a one-to-one conversation, else every participant, else
 * "(unknown)". Every screen names a conversation with this, so no two screens
 * name one conversation two ways.
 */
export function conversationName({
  shown_title,
  is_group,
  participants,
}: ConversationNameParts): string {
  const trimmed = shown_title?.trim();
  if (trimmed) return trimmed;
  const names = participants.map((p) => p.name);
  if (!is_group) return names[0] ?? "(unknown)";
  return names.length > 0 ? names.join(", ") : "(unknown)";
}
