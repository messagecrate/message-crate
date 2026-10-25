import { ApiError } from "./api";

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

/** `text` with its first letter capitalized. */
function sentenceCase(text: string): string {
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}

/**
 * What went wrong, as a sentence of its own on a screen: on a line of its own,
 * or after another sentence that ends with a period. Every screen that shows a
 * failure that way goes through here. The text is `rejectionMessage`'s, with
 * `fallback` for anything that has no message, or `String(err)` when no
 * fallback is given, so a desktop command's string rejection is shown too.
 *
 * The server writes each `detail` in lowercase with no final period
 * (`docs/architecture/rules/http-api.md`, API-53), and a sentence on a screen
 * starts with a capital, so the first letter is capitalized. A sentence that
 * starts with a name a capital would change is kept as it is: a
 * `search-query-invalid` refusal about one search word starts with that word
 * as it is typed (`from: is not a Contacts word; it works on Messages`), and a
 * server refusal that starts with a field's snake_case name or indexed path
 * (`asset_max_bytes must be at most …`, `identities[0].address: …`) names the
 * field as the API spells it.
 * Any other sentence that names something first, such as
 * `sort: unknown key …`, starts with a capital.
 *
 * A sentence joined after a prefix that already starts the line ("Could not
 * load contacts: …") uses `errorText`, `apiErrorMessage` or
 * `rejectionMessage`, and stays as the server wrote it.
 */
export function errorLine(err: unknown, fallback?: string): string {
  const text = rejectionMessage(err, fallback ?? String(err));
  const aboutAWord =
    err instanceof ApiError && err.type === "search-query-invalid" && /^[a-z][a-z-]*:/.test(text);
  const aboutAField = err instanceof ApiError && /^[a-z][a-z0-9]*(?:_|\[\d+\])/.test(text);
  return aboutAWord || aboutAField ? text : sentenceCase(text);
}
