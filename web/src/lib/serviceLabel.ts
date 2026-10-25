import type { components } from "./serverApi.types";
import type { Conversation } from "./types";

/** A service the server takes for an identity, from the generated types. */
type IdentityService = components["schemas"]["IdentityService"];

/**
 * The screen name of each service the server takes for an identity. Typed
 * over `IdentityService`, so a service added to the server's list does not
 * compile until it has a name here.
 */
export const IDENTITY_SERVICE_LABELS = {
  text_message: "Text Message",
  whatsapp: "WhatsApp",
} as const satisfies Record<IdentityService, string>;

/**
 * The transports a message's `service` records that are Text Message:
 * iMessage, SMS, MMS and RCS are one service to someone reading their
 * messages, and which one carried a text is not what the label is for.
 */
const TEXT_MESSAGE_TRANSPORTS: ReadonlySet<string> = new Set([
  "imessage",
  "ios",
  "sms",
  "mms",
  "sms/mms",
  "rcs",
]);

function isIdentityService(word: string): word is IdentityService {
  return Object.hasOwn(IDENTITY_SERVICE_LABELS, word);
}

/**
 * The screen name of a service word, the one map every screen reads: a
 * service the server takes for an identity, a transport a message took, or
 * `email`, the type the identities list names an email address by. Any
 * other word keeps its own name with its first letter capitalized, such as
 * `Signal`. Null for an empty word or `unknown`, which name no service.
 */
export function serviceLabel(service: string | null | undefined): string | null {
  const word = (service ?? "").trim().toLowerCase();
  if (!word || word === "unknown") return null;
  if (isIdentityService(word)) return IDENTITY_SERVICE_LABELS[word];
  if (TEXT_MESSAGE_TRANSPORTS.has(word)) return IDENTITY_SERVICE_LABELS.text_message;
  if (word === "email") return "Email";
  return word.charAt(0).toUpperCase() + word.slice(1);
}

/**
 * The service label the conversation list and the conversation panel show:
 * the conversation's own, or for a one-to-one conversation without one, its
 * participant's.
 */
export function conversationServiceLabel(conv: Conversation): string | null {
  const fromConv = serviceLabel(conv.service);
  if (fromConv || conv.is_group) return fromConv;
  const p = conv.participants[0];
  return p ? serviceLabel(p.service) : null;
}
