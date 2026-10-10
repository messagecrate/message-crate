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
 * Turn anything a promise rejected with, or a `catch` caught, into text: the
 * message of an `Error`, and `String(err)` of anything else. An `Error` whose
 * message is empty gives `String(err)`, which is `"Error"`.
 */
export function errorText(err: unknown): string {
  return apiErrorMessage(err, String(err));
}
