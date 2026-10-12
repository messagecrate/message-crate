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

/**
 * `text` with its first letter capitalised, for a server `detail` shown as a
 * line of its own. The server writes each `detail` in lowercase with no final
 * period (`docs/architecture/http-api.md`, "Failures"), and a line of UI text
 * starts with a capital.
 */
export function sentenceCase(text: string): string {
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}

/**
 * What went wrong, as a line of its own on a screen: `errorText` with its
 * first letter capitalised.
 */
export function errorLine(err: unknown): string {
  return sentenceCase(errorText(err));
}

/**
 * A failure on a screen that runs a search, as a line of its own. A search
 * refusal that is about one word starts with that word as it is typed
 * (`from: is not a Contacts word; it works on Messages`), and a capital would
 * change the word, so it is shown as it is. Any other sentence starts with a
 * capital, as `errorLine` gives it.
 */
export function searchErrorLine(err: unknown, fallback: string): string {
  const text = apiErrorMessage(err, fallback);
  return /^[a-z][a-z-]*:/.test(text) ? text : sentenceCase(text);
}
