import { phonesMatch } from "./phoneTokens";

/** The type of a backup identity's address: an email address or a phone number. */
export type IdentityType = "phone" | "email";

/** Anything with an `@` is an email; everything else is a phone. */
export function identityType(value: string): IdentityType {
  return value.includes("@") ? "email" : "phone";
}

/** Whether one backup identity is on the account's profile. */
export function identityOnProfile(
  value: string,
  profile: { phones: string[]; emails: string[] },
): boolean {
  if (identityType(value) === "email") {
    const needle = value.trim().toLowerCase();
    return profile.emails.some((email) => email.trim().toLowerCase() === needle);
  }
  return profile.phones.some((phone) => phonesMatch(value, phone));
}

/**
 * Messages staged under one backup identity, sent and received. A staged
 * identity is the same address as this one when it matches the way a profile
 * entry would, so two spellings of one phone number count together.
 */
export function identityMessageCounts(
  identity: string,
  ownerIdentities: { identity: string; sent: number; received: number }[],
): { sent: number; received: number } {
  const address =
    identityType(identity) === "email"
      ? { phones: [], emails: [identity] }
      : { phones: [identity], emails: [] };
  return ownerIdentities
    .filter((staged) => identityOnProfile(staged.identity, address))
    .reduce(
      (total, { sent, received }) => ({
        sent: total.sent + sent,
        received: total.received + received,
      }),
      { sent: 0, received: 0 },
    );
}

/**
 * Whether Import should stop before creating the Import Run: identities were
 * read and none is on the profile. Fails open — no identities read, or no
 * profile loaded (fetch failed), never blocks an import.
 */
export function needsIdentityStop(
  identities: string[],
  profile: { phones: string[]; emails: string[] } | null,
): boolean {
  if (identities.length === 0 || profile === null) return false;
  return !identities.some((identity) => identityOnProfile(identity, profile));
}

/** The Import Run's stored identity list, or null when absent or malformed. */
export function parseSourceIdentities(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  return value.every((item) => typeof item === "string") ? (value as string[]) : null;
}
