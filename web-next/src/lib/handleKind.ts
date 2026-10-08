import { stripPhoneFormatting } from "./phoneE164";

/**
 * Handle identity types (mirrors `message_ir::IdentityType`).
 * The type determines normalization and matching for a handle.
 */
export type IdentityType = "phone" | "email" | "username" | "other";

/** iMessage-style: any handle containing `@` is treated as email. */
export function isEmailHandle(handle: string): boolean {
  return handle.includes("@");
}

/** Infer a handle type from the handle's shape when the source does not say. */
export function inferHandleType(raw: string): IdentityType {
  const h = raw.trim();
  if (!h) return "other";
  if (h.includes("@")) return "email";
  const hasDigit = /\d/.test(h);
  const allPhoneChars = /^[\d+\- ().#*]+$/.test(h);
  if (hasDigit && allPhoneChars) return "phone";
  return "other";
}

/**
 * Canonical form of a handle for identity matching, per type (guarded policy,
 * mirroring the vault's `phone::normalize_guarded`).
 *
 * Phone: E.164 when the raw is unambiguous — `+`-prefixed (8–15 digits) or a
 * US national number (10 digits, or 11 starting with 1). Otherwise the digits
 * stay as-is, so a trunk-zero `020 7946 0000` becomes `02079460000` — never
 * the fabricated `+02079460000` — and matches the review-flagged row the
 * import wrote. Email: lowercased. Username/Other: verbatim (trimmed).
 */
export function normalizeHandle(raw: string, handleType: IdentityType): string {
  const trimmed = raw.trim();
  switch (handleType) {
    case "phone": {
      // Keep every digit, including a leading 1 country code: `+1…` values
      // must not be treated as 10-digit nationals (sanitizePhoneDigits would
      // strip the 1). This mirrors the vault's `normalize_certain` rules.
      const digits = stripPhoneFormatting(trimmed).replace(/\D/g, "");
      if (!digits) return trimmed;
      if (trimmed.startsWith("+")) {
        // E.164 country codes never start with 0 (0 is the trunk prefix), so
        // a `+0…` value is fabricated, not certain.
        if (
          digits.length >= 8 &&
          digits.length <= 15 &&
          !digits.startsWith("0")
        ) {
          return `+${digits}`;
        }
        // A + with an implausible length or a leading-0 country code is
        // still ambiguous — keep digits.
        return digits;
      }
      if (digits.length === 10) return `+1${digits}`;
      if (digits.length === 11 && digits.startsWith("1")) return `+${digits}`;
      // Short codes and ambiguous national numbers (e.g. trunk-zero) keep
      // their digits so the vault can flag them for review.
      return digits;
    }
    case "email":
      return trimmed.toLowerCase();
    case "username":
    case "other":
      return trimmed;
  }
}

/**
 * Review note for a phone raw under the guarded policy, mirroring the vault's
 * `phone::normalize_uncertain_reason`. Null when the raw normalizes to
 * unambiguous E.164 (or has no usable digits).
 */
export function phoneReviewNote(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (normalizeHandle(trimmed, "phone").startsWith("+")) return null;
  const digits = stripPhoneFormatting(trimmed).replace(/\D/g, "");
  if (!digits) return null;
  if (trimmed.startsWith("+")) {
    if (digits.startsWith("0")) {
      return "international country code cannot start with 0";
    }
    return "international needs 8–15 digits after +";
  }
  return "USA needs 10 digits or 11 starting with 1";
}

/** Handles safe for contacts.csv `phones` column. */
export function phoneHandlesOnly(handles: string[]): string[] {
  return handles.filter((h) => h.trim() !== "" && !isEmailHandle(h));
}

/** Preferred display phone: first non-email handle, else first handle. */
export function preferredPhoneHandle(handles: string[]): string | null {
  const phones = phoneHandlesOnly(handles);
  if (phones[0]) return phones[0];
  const first = handles.map((h) => h.trim()).find(Boolean);
  return first ?? null;
}
