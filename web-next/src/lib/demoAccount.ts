/**
 * Id of the seeded demo account (`reset-demo`). The server fixes it at 2:
 * `DEMO_ACCOUNT_ID` in `crates/server/server/src/db/account_profile.rs`.
 */
export const DEMO_ACCOUNT_ID = 2;

/** Account ids arrive as strings (session cookie, route params). */
export function isDemoAccount(accountId: string | number): boolean {
  return String(accountId) === String(DEMO_ACCOUNT_ID);
}

/** The refusal for a change the server never allows on the demo account. */
export const DEMO_ACCOUNT_REFUSAL =
  "The Demo Account’s sign-in, name, and phones can’t be changed, and it can’t be deleted.";
