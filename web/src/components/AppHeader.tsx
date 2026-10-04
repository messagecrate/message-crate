import { useState } from "react";
import type { SearchScope } from "../lib/recentSearches";
import { otherResultsView } from "../lib/resultsView";
import type { SearchList } from "../lib/searchFields";
import { Z_APP_HEADER } from "../lib/zLayers";
import type { AdvancedSearchMode } from "./AdvancedSearchForm";
import AppAccountMenu from "./AppAccountMenu";
import { loadWidth } from "./columnResize";
import {
  LEFT_PANEL_DEFAULT_WIDTH,
  LEFT_PANEL_MAX_WIDTH,
  LEFT_PANEL_MIN_WIDTH,
  LEFT_PANEL_STORAGE_KEY,
  LEFT_PANEL_WIDTH_VAR,
} from "./leftPanelWidth";
import SearchBar from "./SearchBar";
import VersionNotice from "./VersionNotice";

/** Which list the header search runs against. */
export type HeaderSearchTarget = "accounts" | "contacts" | "conversations" | "messages" | "trash";

/**
 * Every target uses the same bar; only the wording, the recents bucket, the
 * advanced form, and the list whose words it suggests differ. Trash sends one
 * query to the conversations list and the contacts list at once, so its
 * advanced form offers only the words both accept (`contacts` mode); the
 * TrashScreen explains any typed word that one of the two lists refuses.
 */
const SEARCH_TARGETS: Record<
  HeaderSearchTarget,
  {
    scope: SearchScope;
    list: SearchList | null;
    /** The list whose words the box marks rather than sends (#1561); see `SearchBar`. */
    otherList: SearchList | null;
    placeholder: string;
    advancedMode: AdvancedSearchMode | null;
  }
> = {
  // Owner Home filters the accounts table by username: no search words, no advanced form.
  accounts: {
    scope: "account",
    list: null,
    otherList: null,
    placeholder: "Search accounts",
    advancedMode: null,
  },
  contacts: {
    scope: "contact",
    list: "contacts",
    otherList: null,
    placeholder: "Search contacts",
    advancedMode: "contacts",
  },
  // The Messages screen's two result lists (#313): one search box, whose
  // words and wording follow the list the switch shows. Both keep one set
  // of recent searches, because one box serves both. A word only the other
  // list takes is marked in the box, so switching keeps it for later.
  conversations: {
    scope: "message",
    list: "conversations",
    otherList: otherResultsView("conversations"),
    placeholder: "Search conversations",
    advancedMode: "messages",
  },
  messages: {
    scope: "message",
    list: "messages",
    otherList: otherResultsView("messages"),
    placeholder: "Search messages",
    advancedMode: "messages",
  },
  trash: {
    scope: "trash",
    list: "conversations",
    otherList: null,
    placeholder: "Search Trash",
    // Trash sends one query to both the contacts and the conversations
    // list; the contacts form offers only words both lists accept.
    advancedMode: "contacts",
  },
};

/** Full-width bar: app name on the left, search in the middle, the account button on the far right. */
/** What the header searches: a list, and that list's search so far. */
export type HeaderSearch = { target: HeaderSearchTarget; query: string };

export default function AppHeader({
  search,
  onSearchChange,
  onSearch,
}: {
  /**
   * The list the search box searches: the one in the section the person is
   * in. `null` on a screen with no list (Import, Export, Settings), which
   * shows no box; a box that comes back starts from its list's search rather
   * than from text typed before.
   */
  search: HeaderSearch | null;
  onSearchChange: (v: string) => void;
  onSearch: (q: string) => void;
}) {
  const target = search === null ? null : SEARCH_TARGETS[search.target];
  // Same key as LeftPanel so a stored width does not flash at the default.
  const [brandWidth] = useState(() =>
    loadWidth(
      LEFT_PANEL_STORAGE_KEY,
      LEFT_PANEL_DEFAULT_WIDTH,
      LEFT_PANEL_MIN_WIDTH,
      LEFT_PANEL_MAX_WIDTH,
    ),
  );

  return (
    <>
      <header
        className={`relative flex h-[3.5625rem] shrink-0 items-center border-b border-border bg-panel ${Z_APP_HEADER}`}
      >
        <div
          className="box-border flex h-12 shrink-0 items-center px-3"
          style={{ width: `var(${LEFT_PANEL_WIDTH_VAR}, ${brandWidth}px)` }}
        >
          <span className="text-[0.875rem] font-bold text-text">Message Crate</span>
        </div>
        {/* The bar's height is fixed (the search box's height plus its
            padding, as it was when the box set it) rather than taken from the
            box, so a screen with no box keeps the same header and the name and
            the account button do not move. */}
        <div className="flex min-w-0 flex-1 items-center justify-center px-3">
          {search && target && (
            <div className="w-full max-w-xl">
              <SearchBar
                key={search.target}
                value={search.query}
                scope={target.scope}
                list={target.list}
                otherList={target.otherList}
                placeholder={target.placeholder}
                advancedMode={target.advancedMode}
                onChange={onSearchChange}
                onSubmit={onSearch}
              />
            </div>
          )}
        </div>
        <div className="flex shrink-0 items-center px-3">
          <AppAccountMenu />
        </div>
      </header>
      <VersionNotice />
    </>
  );
}
