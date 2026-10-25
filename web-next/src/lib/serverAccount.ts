import { redirect } from "next/navigation";

import { NOT_SIGNED_IN, requireAccountId } from "./accountContext";
import { runWithAccountAsync } from "./accountScope";
import { SESSION_ENDED_PATH } from "./sessionEnded";
import { accountNeedsOnboarding } from "./vault/account";

/**
 * Run a page's reads, sending the browser to {@link SESSION_ENDED_PATH} when
 * one of them says the person is not signed in: a session cookie is missing,
 * or the server refused the token in it (a new login as the same account, a
 * logout elsewhere, or expiry). Any other failure is thrown as it is.
 */
export async function redirectWhenSignedOut<T>(
  read: () => Promise<T>,
): Promise<T> {
  try {
    return await read();
  } catch (err) {
    if (err instanceof Error && err.message === NOT_SIGNED_IN) {
      redirect(SESSION_ENDED_PATH);
    }
    throw err;
  }
}

export async function withServerAccount<T>(
  fn: (accountId: string) => T | Promise<T>,
  options?: { allowIncomplete?: boolean },
): Promise<T> {
  return redirectWhenSignedOut(async () => {
    const accountId = await requireAccountId();
    return runWithAccountAsync(accountId, async () => {
      if (!options?.allowIncomplete && (await accountNeedsOnboarding())) {
        redirect("/onboarding");
      }
      return fn(accountId);
    });
  });
}
