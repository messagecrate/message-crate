/**
 * The words each list takes, as `GET /v1/search-fields/{list}` answers, for
 * tests that need a list's words without a server. The words and lists are
 * those of `docs/architecture/search.md`, "Words"; every value is text.
 */

import type { SearchField, SearchList } from "../lib/searchFields";

const WORDS: Record<SearchList, readonly string[]> = {
  contacts: [
    "name",
    "identity",
    "group",
    "tag",
    "kind",
    "service",
    "date",
    "first-message",
    "last-message",
    "messages",
    "conversations",
    "groups",
    "trashed",
  ],
  conversations: [
    "body",
    "subject",
    "name",
    "title",
    "identity",
    "with",
    "group",
    "tag",
    "kind",
    "service",
    "source",
    "import",
    "date",
    "first-message",
    "last-message",
    "attachment",
    "filename",
    "size",
    "messages",
    "participants",
    "trashed",
  ],
  messages: [
    "body",
    "subject",
    "name",
    "title",
    "identity",
    "with",
    "from",
    "to",
    "in",
    "group",
    "tag",
    "kind",
    "service",
    "source",
    "import",
    "date",
    "first-message",
    "last-message",
    "attachment",
    "filename",
    "size",
    "participants",
    "attachments",
    "trashed",
  ],
};

/** The words `list` takes. */
export function searchFieldsFor(list: SearchList): SearchField[] {
  return WORDS[list].map((word) => ({
    word,
    value_type: "text",
    values: [],
    help: "",
    example: `${word}:x`,
  }));
}
