/**
 * Pull the human-readable part out of an API error: the message of an
 * `Error`, and `fallback` for anything else. An `Error` whose message is
 * empty gives `fallback` too, so a caller never shows an empty line.
 *
 * `apiClient` rejects with an `ApiError` whose `message` is already the
 * server's own sentence (`problemFromBody` in `api.ts` reads the problem
 * document once, at the client), so there is nothing left to unwrap here.
 */
export function apiErrorMessage(err: unknown, fallback: string): string {
  return err instanceof Error && err.message ? err.message : fallback;
}

/**
 * What went wrong, in a sentence, for a call that reaches either the server or
 * the desktop app: a desktop command rejects with its error as a string, which
 * is shown as it is, and anything else goes through `apiErrorMessage`.
 */
export function rejectionMessage(err: unknown, fallback: string): string {
  return typeof err === "string" && err ? err : apiErrorMessage(err, fallback);
}

/**
 * Turn anything a promise rejected with, or a `catch` caught, into text: the
 * message of an `Error`, and `String(err)` of anything else. An `Error` whose
 * message is empty gives `String(err)`, which is the error's `name` (`"Error"`
 * for a plain `Error`).
 */
export function errorText(err: unknown): string {
  return apiErrorMessage(err, String(err));
}
