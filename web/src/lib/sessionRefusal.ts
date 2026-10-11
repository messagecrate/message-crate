/**
 * A session the server refused, as the login sees it.
 *
 * Three callers learn the server refused the session, and none of them can
 * reach the login: the query client (`sessionQueryClient.tsx`), built above
 * the provider that holds the session, and an Import Run, whose Upload sends
 * its own requests from the desktop side and whose own calls go straight
 * through `serverApi.ts` (#1491, #1677). Each reports the refusal here.
 *
 * `auth.tsx` registers how to end the session here, and the query client and
 * the run in `screens/import/run/` (`serverCalls.ts`, `upload.ts`) call it,
 * without either side depending on the other.
 */

let endSession: (token?: string) => void = () => {};

/**
 * Record what ends the session when the server refuses it. Returns what to
 * call once that no longer applies.
 */
export function onSessionRefused(handler: (token?: string) => void): () => void {
  endSession = handler;
  return () => {
    if (endSession === handler) endSession = () => {};
  };
}

/**
 * The server refused a session: end it here as well. `token` is the session
 * an Import Run sent, and a refusal of a token a later login has replaced
 * ends nothing. Without `token`, the refusal is of the session in use now.
 */
export function sessionRefused(token?: string): void {
  endSession(token);
}
