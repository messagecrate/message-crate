import type { Conversation } from "./types";

/**
 * Short service label for a conversation.
 *
 * iMessage and SMS/MMS are the same thing to someone reading their messages — a
 * text message — and which transport carried it is not what the label is for.
 * `text_message` is the stored name of that service, which a participant
 * carries when the conversation names no transport of its own.
 * Anything else (WhatsApp, say) keeps its own name.
 */
function formatServiceLabel(service: string | null | undefined): string | null {
  const s = (service ?? "").trim();
  if (!s || s.toLowerCase() === "unknown") return null;
  const lower = s.toLowerCase();
  const texting = ["text_message", "imessage", "ios", "sms/mms", "sms", "mms"];
  if (texting.includes(lower) || lower.includes("sms")) return "Text Message";
  return s;
}

/**
 * The service label the conversation list and the conversation panel show:
 * the conversation's own, or for a one-to-one conversation without one, its
 * participant's.
 */
export function conversationServiceLabel(conv: Conversation): string | null {
  const fromConv = formatServiceLabel(conv.service);
  if (fromConv || conv.is_group) return fromConv;
  const p = conv.participants[0];
  return p ? formatServiceLabel(p.service) : null;
}
