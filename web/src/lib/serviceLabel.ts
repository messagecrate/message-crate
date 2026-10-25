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
 * The screen name of `email`, the type the identities list names an email
 * address by, and the option an identity is added under.
 */
export const EMAIL_LABEL = "Email";

/**
 * The words that are Text Message. iMessage, SMS, MMS and RCS are one
 * service to someone reading their messages, and which one carried a text
 * is not what the label is for.
 *
 * A conversation's `service` is not a transport: the server builds it from
 * the import sources of its messages (`display_service_label` in
 * `crates/server/server/src/db/conversations.rs`), as `SMS/MMS`, `imessage`,
 * `WhatsApp`, or the one source's id. So the source ids that carry nothing
 * but texts are listed too. `imazing` and `openextract` are not, because an
 * iMazing export can hold WhatsApp chats.
 */
const TEXT_MESSAGE_WORDS: ReadonlySet<string> = new Set([
  "imessage",
  "ios",
  "sms",
  "mms",
  "sms/mms",
  "rcs",
  "sms-backup-restore",
  "sms-backup-plus",
  "go-sms-pro",
]);

function isIdentityService(word: string): word is IdentityService {
  return Object.hasOwn(IDENTITY_SERVICE_LABELS, word);
}

/**
 * The screen name of a service word, the one map every screen reads: a
 * service the server takes for an identity, a word that is Text Message, or
 * `email`. Any other word keeps its own spelling with its first letter
 * capitalized, such as `Signal`. Null for an empty word or `unknown`, which
 * name no service.
 */
export function serviceLabel(service: string | null | undefined): string | null {
  const trimmed = (service ?? "").trim();
  const word = trimmed.toLowerCase();
  if (!word || word === "unknown") return null;
  if (isIdentityService(word)) return IDENTITY_SERVICE_LABELS[word];
  if (TEXT_MESSAGE_WORDS.has(word)) return IDENTITY_SERVICE_LABELS.text_message;
  if (word === "email") return EMAIL_LABEL;
  return trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
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
