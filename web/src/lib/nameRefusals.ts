import { getAccountId, onAccountIdChange } from "./api";
import { createExternalStore } from "./externalStore";

/**
 * A display name save the server refused after the Settings screen that
 * sent it had gone away, so nothing on that screen could say so (#2991).
 * The page shows each one across its top until the person dismisses it.
 */
export interface NameRefusal {
  /** The logged-in account that sent the save. */
  accountId: number | null;
  /** The Profile tab the name was typed on, to go back to. */
  profilePath: string;
  /** The account the name was for when it is not the logged-in one: an account the owner opened. */
  username: string | null;
  /** The name that was sent; empty when the save cleared the name. */
  name: string;
  /** Why the server refused it, as one line. */
  reason: string;
}

/** Refused saves not yet dismissed, oldest first, at most one per Profile. */
export const nameRefusals = createExternalStore<readonly NameRefusal[]>([]);

/**
 * Hold `refusal` for the page to show. A newer refusal from the same Profile
 * replaces the older one. A refusal sent by an account that is no longer
 * logged in is dropped: it is not the logged-in person's to read.
 */
export function reportNameRefusal(refusal: NameRefusal): void {
  if (refusal.accountId !== getAccountId()) return;
  nameRefusals.set([
    ...nameRefusals.get().filter((r) => r.profilePath !== refusal.profilePath),
    refusal,
  ]);
}

/**
 * Stop showing the refusal from the Profile at `profilePath`: the person
 * dismissed it, or a later save from that Profile went through.
 */
export function dismissNameRefusal(profilePath: string): void {
  if (!nameRefusals.get().some((r) => r.profilePath === profilePath)) return;
  nameRefusals.set(nameRefusals.get().filter((r) => r.profilePath !== profilePath));
}

/** Drop every refusal held, for a test that must start without one. */
export function clearNameRefusals(): void {
  if (nameRefusals.get().length > 0) nameRefusals.set([]);
}

// Logging out, or in as another account, drops every refusal held.
onAccountIdChange(clearNameRefusals);
