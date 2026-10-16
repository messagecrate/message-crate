/**
 * The search language, as the server describes it.
 *
 * The browser keeps no list of words of its own: `GET /v1/search-fields/{list}`
 * says which words one list accepts, what kind of value each takes, and which values
 * a choice word allows, and the search box builds its suggestions from that.
 * What is left here are two rules about the *shape* of a query — whether it
 * carries a `word:` token at all, and what its plain words are — which the
 * contact and conversation lists use to decide whether they can narrow rows in
 * the browser or must ask the server.
 */

import { dropTokens, type FieldToken, fieldTokens } from "./searchQuery";
import type { SearchFieldList } from "./serverApi";
import type { components } from "./serverApi.types";

type Schema = components["schemas"];
export type SearchField = Schema["FieldDoc"];
export type SearchList = SearchFieldList;

/**
 * True when the query has a `word:` token, which only the server can apply.
 * The tokens are read as the server's lexer reads them (`fieldTokens`), so a
 * colon inside a quoted phrase or a pasted `http://` address is not one.
 */
export function hasFieldToken(q: string): boolean {
  return fieldTokens(q).length > 0;
}

/** The field words a query carries, lower-cased and without the leading minus, in order of appearance. */
function fieldWords(q: string): string[] {
  return [...new Set(fieldTokens(q).map((t) => t.word))];
}

/**
 * The words in `q` that `fields` (one list's registry) does not carry. A
 * screen that sends one query to two lists uses this to tell which pane a
 * word applies to, instead of letting the other pane answer with a 422.
 */
export function unsupportedFieldWords(q: string, fields: readonly SearchField[]): string[] {
  const known = new Set(fields.map((f) => f.word));
  return fieldWords(q).filter((w) => !known.has(w));
}

/** The free-text words of a query, with every `word:value` token removed. */
export function stripFieldTokens(q: string): string {
  return dropTokens(q, fieldTokens(q)).replace(/\s+/g, " ").trim();
}

/** The name a list goes by on screen. */
export const SEARCH_LIST_NAMES: Record<SearchList, string> = {
  contacts: "Contacts",
  conversations: "Conversations",
  messages: "Messages",
};

/** A `word:value` token one list does not take and another list does. */
export type MarkedWord = FieldToken & { worksIn: SearchList };

/**
 * The tokens of `query` whose word `fields` (one list's words) does not
 * carry and `other.fields` does, each with the list it works in. A word
 * neither list has is left to the server, which refuses it and offers the
 * nearest word.
 */
export function markedWords(
  query: string,
  fields: readonly SearchField[],
  other: { list: SearchList; fields: readonly SearchField[] },
): MarkedWord[] {
  const here = new Set(fields.map((f) => f.word));
  const there = new Set(other.fields.map((f) => f.word));
  return fieldTokens(query)
    .filter((t) => !here.has(t.word) && there.has(t.word))
    .map((t) => ({ ...t, worksIn: other.list }));
}
