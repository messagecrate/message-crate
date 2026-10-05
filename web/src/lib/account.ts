import type { components } from "./serverApi.types";

/**
 * The logged-in account as the server returns it from `GET /v1/accounts/{id}`:
 * its profile, its flags, and how much it holds.
 *
 * Generated from the server's own OpenAPI document rather than written here, so
 * a field renamed on the server becomes a build error instead of a screen that
 * silently shows nothing.
 */
export type AccountProfile = components["schemas"]["Account"];

/**
 * One phone number on the account, with every service it is an identity
 * under: a number on Text Message and on WhatsApp is one entry naming both.
 */
export type AccountPhone = components["schemas"]["AccountPhone"];

/** The account's phone numbers, each once, whatever services they are on. */
export function phoneNumbers(profile: Pick<AccountProfile, "phones">): string[] {
  return profile.phones.map((phone) => phone.address);
}

/**
 * The account's own addresses, phone numbers and email addresses, each once,
 * for matching a backup's identities against them.
 */
export function profileAddresses(profile: Pick<AccountProfile, "phones" | "emails">): {
  phones: string[];
  emails: string[];
} {
  return { phones: phoneNumbers(profile), emails: profile.emails };
}

/**
 * What nobody may change on an account, the owner included: each field is
 * true when the server refuses that change for the account.
 *
 * Only the Demo Account has any. Every visitor shares it, so the server
 * refuses these by its id, whatever its permission row says
 * (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`). The settings
 * screens read this one list instead of each testing `is_demo`, so a limit
 * the server adds is named here once.
 */
export type FixedSettings = {
  displayName: boolean;
  timeZone: boolean;
  /** The account's own identities, which decide which of its messages read as sent. */
  identities: boolean;
  /** Loading an address book into the account's contacts. */
  addressBook: boolean;
  /** Whether the account is active, and its import, export and delete permissions. */
  statusAndPermissions: boolean;
  /** Setting a password: the Demo Account never has one. */
  password: boolean;
  /** Deleting every message for good, by the account or by the owner. */
  deleteMessages: boolean;
  /** The account deleting itself. The owner may still delete it. */
  deleteOwnAccount: boolean;
};

/** What nobody may change on the account `profile` describes ({@link FixedSettings}). */
export function fixedSettings(profile: Pick<AccountProfile, "is_demo">): FixedSettings {
  const demo = profile.is_demo;
  return {
    displayName: demo,
    timeZone: demo,
    identities: demo,
    addressBook: demo,
    statusAndPermissions: demo,
    password: demo,
    deleteMessages: demo,
    deleteOwnAccount: demo,
  };
}
