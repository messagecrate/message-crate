/**
 * A session the server refused to an Import Run, as the login sees it.
 *
 * The run's server calls do not go through TanStack Query: the push sends
 * its own requests from the desktop side, and the run's own calls go
 * straight through `serverApi.ts`. So the query
 * client's `onUnauthorized` never sees their refusals, and the session has to
 * end here instead, as a query the server refuses ends it: for the push
 * (#1491), and for the run's own calls (#1677).
 *
 * `auth.tsx` registers how to end the session here, and the run in
 * `screens/import/useImportJob.ts` calls it, without either depending on the
 * other.
 */

let endSession: (token: string) => void = () => {};

/**
 * Record what ends the session when the server refuses the session token an
 * Import Run sent. Returns what to call once that no longer applies.
 */
export function onSessionRefused(handler: (token: string) => void): () => void {
  endSession = handler;
  return () => {
    if (endSession === handler) endSession = () => {};
  };
}

/**
 * The server refused `token`, the session the Import Run sent: end it here as
 * well, unless a later login has replaced it.
 */
export function sessionRefused(token: string): void {
  endSession(token);
}
