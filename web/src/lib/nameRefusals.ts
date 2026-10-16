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
  /** When the save was sent, from `nameSendStamp`. */
  sent: number;
}

let sends = 0;

/**
 * A number that grows with every display name save sent from any Settings
 * screen. Answers arrive in any order, and a screen left and opened again
 * starts afresh, so the order the saves were sent in is what says which
 * refusal or success is the newer one.
 */
export function nameSendStamp(): number {
  sends += 1;
  return sends;
}

/** Refused saves not yet dismissed, oldest first, at most one per Profile. */
export const nameRefusals = createExternalStore<readonly NameRefusal[]>([]);

/**
 * Hold `refusal` for the page to show. A refusal of a save sent later from
 * the same Profile replaces an earlier one, and is not replaced by one. A
 * refusal sent by an account that is no longer logged in is dropped: it is
 * not the logged-in person's to read.
 */
export function reportNameRefusal(refusal: NameRefusal): void {
  if (refusal.accountId !== getAccountId()) return;
  const held = nameRefusals.get().find((r) => r.profilePath === refusal.profilePath);
  if (held && held.sent > refusal.sent) return;
  nameRefusals.set([
    ...nameRefusals.get().filter((r) => r.profilePath !== refusal.profilePath),
    refusal,
  ]);
}

/**
 * Stop showing the refusal from the Profile at `profilePath` whose save was
 * sent before `sentBefore`. The person's Dismiss passes no stamp and removes
 * it whatever its age. A save that went through passes its own stamp, so it
 * clears a refusal of an earlier save and never one of a save sent after it.
 */
export function dismissNameRefusal(
  profilePath: string,
  sentBefore = Number.POSITIVE_INFINITY,
): void {
  const gone = (r: NameRefusal) => r.profilePath === profilePath && r.sent < sentBefore;
  if (!nameRefusals.get().some(gone)) return;
  nameRefusals.set(nameRefusals.get().filter((r) => !gone(r)));
}

/**
 * Drop every refusal held: when the account logs out or another logs in, and
 * for a test that must start without one.
 */
export function clearNameRefusals(): void {
  if (nameRefusals.get().length > 0) nameRefusals.set([]);
}

onAccountIdChange(clearNameRefusals);
