import type { components } from "./serverApi.types";
import type { Conversation } from "./types";

/** A service the server takes for an identity, from the generated types. */
type IdentityService = components["schemas"]["IdentityService"];

/** A service the server names a conversation by, from the generated types. */
type ConversationService = components["schemas"]["ConversationService"];

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
 * The screen name of `email`, the type the identities list names an email
 * address by, and the option an identity is added under.
 */
export const EMAIL_LABEL = "Email";

/**
 * The screen name of each service the server names a conversation by: the
 * services of an identity under the same names, each other service the
 * conversation file names, and Unknown. The server sends `unknown` only when
 * a message's transport could not be read or the messages name two
 * services, a fault it logs at import, so it is shown, never hidden or read
 * as Text Message. Typed over `ConversationService`, so a service added to
 * the server's list does not compile until it has a name here.
 */
export const CONVERSATION_SERVICE_LABELS = {
  ...IDENTITY_SERVICE_LABELS,
  discord: "Discord",
  signal: "Signal",
  telegram: "Telegram",
  slack: "Slack",
  unknown: "Unknown",
} as const satisfies Record<ConversationService, string>;

function isIdentityService(word: string): word is IdentityService {
  return Object.hasOwn(IDENTITY_SERVICE_LABELS, word);
}

/**
 * The screen name of an identity's service word: a service the server takes
 * for an identity, or `email`. Any other word keeps its own spelling with its
 * first letter capitalized. Null for an empty word or `unknown`, the word
 * `inferService` gives an address it cannot place, which names no service.
 */
export function serviceLabel(service: string | null | undefined): string | null {
  const trimmed = (service ?? "").trim();
  const word = trimmed.toLowerCase();
  if (!word || word === "unknown") return null;
  if (isIdentityService(word)) return IDENTITY_SERVICE_LABELS[word];
  if (word === "email") return EMAIL_LABEL;
  return trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
}

/**
 * The service label the conversation list and the conversation panel show:
 * the name of the service the server names the conversation by, from its
 * messages' transports.
 */
export function conversationServiceLabel(conv: Conversation): string {
  return CONVERSATION_SERVICE_LABELS[conv.service];
}
