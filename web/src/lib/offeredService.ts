import type { components } from "./serverApi.types";
import { IDENTITY_SERVICE_LABELS, serviceLabel } from "./serviceLabel";

/**
 * A service offered wherever an identity is added: setup, the account
 * profile, and the contact drawer. `email` is offered on its own; the server
 * takes it as `text_message` (`serverService`).
 */
export type OfferedService = "text_message" | "email" | "whatsapp";

export const OFFERED_SERVICES = [
  "text_message",
  "email",
  "whatsapp",
] as const satisfies readonly OfferedService[];

/** The service the server takes for an identity: `text_message` or `whatsapp`. */
export type ServerService = components["schemas"]["IdentityService"];

/**
 * The service the server takes for an identity offered on `service`. An email
 * address is on Text Message, where iMessage reaches it; the server types
 * an identity by its address, never by its service, and refuses `email` as a
 * service.
 */
export function serverService(service: OfferedService): ServerService {
  switch (service) {
    case "text_message":
    case "email":
      return "text_message";
    case "whatsapp":
      return "whatsapp";
  }
}

/**
 * The service an identity the server lists is on, or undefined when the list
 * names none the server takes. The list names an email address `email`, its
 * type; one a person added is on Text Message.
 */
export function listedServerService(service: string | null | undefined): ServerService | undefined {
  const known = OFFERED_SERVICES.find((candidate) => candidate === service);
  return known === undefined ? undefined : serverService(known);
}

/** The example phone number shown in empty fields. */
export const EXAMPLE_PHONE = "+1 555-555-0119";

/**
 * The services an identity can be on, offered wherever one is added: setup,
 * the account profile, and the contact drawer. Each carries the example shown
 * in an empty field, on the same line as its service. Text Message and
 * WhatsApp take `EXAMPLE_PHONE`, the web app's one phone example; change it
 * there.
 */
export const OFFERED_SERVICE_OPTIONS = [
  {
    value: "text_message",
    label: IDENTITY_SERVICE_LABELS.text_message,
    placeholder: EXAMPLE_PHONE,
  },
  { value: "email", label: "Email", placeholder: "you@example.com" },
  { value: "whatsapp", label: IDENTITY_SERVICE_LABELS.whatsapp, placeholder: EXAMPLE_PHONE },
] as const satisfies ReadonlyArray<{
  value: OfferedService;
  label: string;
  placeholder: string;
}>;

/** Example shown in an empty value field for `service`. */
export function identityPlaceholder(service: OfferedService): string {
  return OFFERED_SERVICE_OPTIONS.find((option) => option.value === service)?.placeholder ?? "";
}

/**
 * The most characters an identity holds, counted as Unicode characters after
 * trimming: the server refuses a longer one (`MAX_IDENTITY_ADDRESS_CHARS` in
 * `crates/server/server/src/text_caps.rs`).
 */
export const MAX_IDENTITY_CHARS = 320;

/**
 * Why a handle cannot be used, or null when it can. An empty value is not an
 * error here: a blank row is one the person has not filled in yet, and the
 * screens that collect handles decide separately how many they need.
 *
 * A value longer than the server keeps is refused on every service. Beyond
 * that, only a value offered as Email is checked, because the person said it
 * is an email address. Text Message and WhatsApp take any value, as the server
 * does: it types a value that is not a number, such as the WhatsApp internal
 * id `123456789012345@lid` or the sender name `AMAZON`, as an identity of type
 * other (#2709, #3090). A mistyped number is then stored as other.
 */
export function identityValidationError(service: OfferedService, value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  if ([...trimmed].length > MAX_IDENTITY_CHARS) {
    return `An identity can be at most ${MAX_IDENTITY_CHARS} characters.`;
  }
  if (service !== "email") return null;
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(trimmed)
    ? null
    : "Enter an email address like you@example.com.";
}

/** Shown against the second and later rows carrying an account already listed. */
export const DUPLICATE_IDENTITY_MESSAGE = "This account is already in the list.";

/**
 * Key two handles share when they are the same account, or null for a value
 * with nothing to compare. Two rows match when their keys are equal.
 *
 * The service is part of the key because the same number on Text Message and
 * on WhatsApp is two accounts, not one, and listing both is the right thing to
 * do. Within a service the comparison ignores how the value was typed: a
 * value written as a number, with at least one digit, compares by its digits,
 * so `+1 (555) 555-0119` and `+15555550119` land on the same key. Any other
 * value folds to lower case, so a WhatsApp internal id is never taken for the
 * number its digits spell.
 *
 * The digits are compared whole rather than by their last ten, so a number
 * written once with its country code and once without is not caught. That is
 * deliberate — guessing at country codes would let this refuse two numbers
 * that really are different, and a missed duplicate costs far less than an
 * error the person cannot talk their way out of.
 */
export function identityDuplicateKey(service: OfferedService, value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const digits = trimmed.replace(/\D/g, "");
  const writtenAsNumber = digits !== "" && /^\+?[\d\s().-]+$/.test(trimmed);
  const normalized = service !== "email" && writtenAsNumber ? digits : trimmed.toLowerCase();
  return `${service}:${normalized}`;
}

/**
 * Guess the service for a handle when the stored service is empty.
 * Emails contain `@`. Phone-like values are mostly digits.
 */
export function inferService(handle: string, service: string | null | undefined): string {
  if (service?.trim()) return service.trim().toLowerCase();
  const h = handle.trim();
  if (h.includes("@") && !h.startsWith("@")) return "email";
  if (/^\+?\d[\d\s().-]{6,}$/.test(h)) return "text_message";
  return "unknown";
}

/**
 * Label shown in the identities table's Service column: the name
 * `serviceLabel` gives the listed service, or the one guessed from the
 * address when the list names none, and `—` when neither names a service.
 */
export function formatOfferedServiceLabel(
  handle: string,
  service: string | null | undefined,
): string {
  return serviceLabel(inferService(handle, service)) ?? "—";
}
