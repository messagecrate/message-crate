/**
 * The words each list takes, as `GET /v1/search-fields/{list}` answers, for
 * tests that need a list's words without a server.
 *
 * They are read from the user guide's word table (`search.mdx`, "The words"),
 * whose tiles name the lists each word works on. The server's `docs` tests
 * (`crates/server/server/src/search/tests.rs`) hold that table to the
 * registry in `fields.rs`, so these words cannot drift from the server's.
 * Every value is text: the tests that read these need only the words.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { SearchField, SearchList } from "../lib/searchFields";

// Tests run from `web/`. `import.meta.url` is not a file address under jsdom.
const SEARCH_PAGE = resolve(
  process.cwd(),
  "../docs/src/content/docs/docs/user/features/messages/search.mdx",
);

/** The tile letter the word table uses for each list. */
const TILES: Record<SearchList, string> = { contacts: "C", conversations: "V", messages: "M" };

/** One row of the table: `` | `word:` | … | <ListTiles on="C V M" /> | ``. */
const ROW = /^\| `([a-z-]+):` \|.*<ListTiles on="([A-Z ]+)" \/>/;

/** The words `list` takes. */
export function searchFieldsFor(list: SearchList): SearchField[] {
  return readFileSync(SEARCH_PAGE, "utf8")
    .split("\n")
    .flatMap((line) => {
      const row = ROW.exec(line);
      return row?.[2].split(" ").includes(TILES[list]) ? [row[1]] : [];
    })
    .map((word) => ({ word, value_type: "text", values: [], help: "", example: `${word}:x` }));
}
