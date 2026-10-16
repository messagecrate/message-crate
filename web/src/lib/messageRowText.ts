import type { Message, MessageConversation } from "./types";

/**
 * Who sent a message: "You" for one the account sent, else the participant
 * whose identity sent it, by the name the conversation gives them, else the
 * identity itself. Null when a received message names no sender: the row
 * then shows no sender rather than a word, since "Unknown" is a Contact
 * Group's name.
 */
export function messageSenderName(message: Message): string | null {
  if (message.is_from_me) return "You";
  const sender = message.sender;
  if (!sender) return null;
  const participant = message.conversation.participants.find((p) => p.identity === sender);
  return participant?.name ?? sender;
}

/** The text a row shows for a message: its text, or the names of its attachments when it has none. */
export function messageRowText(message: Message): string {
  const text = message.text?.trim();
  if (text) return text;
  return message.attachments
    .map((a) => a.original_name?.trim())
    .filter((name): name is string => Boolean(name))
    .join(", ");
}
