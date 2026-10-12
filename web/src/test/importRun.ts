import { type ImportRunState, importRunStore } from "../screens/import/importRunStore";

/**
 * Put a run in the app's Import Run store as `accountId` started it, with
 * `patch` on top.
 *
 * Pass the logged-in account's id. The run service resets a store that
 * another account holds before the code under test reads it (`takeRunFor` in
 * `screens/import/run/service.ts`), and the screens show such a run as the
 * fresh form, so a run seeded under any other account is gone before the
 * test looks and the test checks nothing (#2579). Only a test of another
 * account's run passes that account's id, on purpose.
 */
export function seedImportRun(
  accountId: number | null,
  patch: Omit<Partial<ImportRunState>, "accountId">,
): void {
  importRunStore.set({ ...patch, accountId });
}
