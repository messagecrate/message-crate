import { loadAccount } from "./accounts";
import { currentAccountId } from "./accountScope";
import { inferHandleType, normalizeHandle } from "./handleKind";
import { loadAccountProfile } from "./accountProfile";

/** Strip non-digits; drop leading US country code 1 when 11 digits. */
export function phoneDigits(handle: string): string {
  let digits = handle.replace(/\D/g, "");
  if (digits.length === 11 && digits.startsWith("1")) {
    digits = digits.slice(1);
  }
  return digits;
}

/**
 * Owner-handle predicate that loads the account and owner profile once.
 * Prefer this over calling {@link isOwnerHandle} in a loop: each account read
 * opens its own connection.
 *
 * Owner handles come from `account_handles JOIN handles`: phones (E.164) and
 * email handles (lowercased). Matching is handle-type aware: the candidate is
 * normalized the same way before comparison.
 */
export function ownerHandleMatcher(): (handle: string) => boolean {
  const accountId = currentAccountId();
  const emails = new Set(
    loadAccount(accountId).emails.map((entry) => entry.email.toLowerCase()),
  );
  const phones = new Set(
    loadAccountProfile(accountId).phones
      .map((p) => normalizeHandle(p, "phone"))
      .filter(Boolean),
  );

  return (handle: string) => {
    const trimmed = handle.trim();
    if (!trimmed) return false;
    const type = inferHandleType(trimmed);
    if (type === "email") return emails.has(normalizeHandle(trimmed, "email"));
    if (type === "phone") return phones.has(normalizeHandle(trimmed, "phone"));
    return false;
  };
}

/** True when handle belongs to this account's phones or emails. */
export function isOwnerHandle(handle: string): boolean {
  return ownerHandleMatcher()(handle);
}

export function assertNotOwnerHandle(handle: string): void {
  if (isOwnerHandle(handle)) {
    throw new Error(
      "This number or email belongs to your account and cannot be assigned to a contact",
    );
  }
}
