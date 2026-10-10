/**
 * Pull the human-readable part out of an API error.
 *
 * `apiClient` now rejects with a `ApiError` whose `message` is already
 * the server's own sentence (`problemFromBody` in `api.ts` reads the problem
 * document once, at the client). There is no `"<status>: <body>"`
 * shape left to unwrap here — this just falls back to `fallback` when the
 * rejection is not an `Error`, or has no message, at all.
 */
export function apiErrorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

/**
 * Turn anything a promise rejected with, or a `catch` caught, into text: the
 * message of an `Error`, and `String(err)` of anything else.
 */
export function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
