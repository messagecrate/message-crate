import { useCallback } from "react";
import ListRangeHeader from "../components/ListRangeHeader";
import MessageSearchRow from "../components/MessageSearchRow";
import SortMenu, { type SortField } from "../components/SortMenu";
import VirtualList from "../components/VirtualList";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { MAX_LIST_OFFSET } from "../lib/listPaging";
import {
  type MessageSearchSort,
  type MessageSearchSortKey,
  type MessageSortParam,
  messageSortFromParam,
  messageSortParam,
} from "../lib/messageSearchSort";
import { countOf } from "../lib/plural";
import { keys } from "../lib/queryKeys";
import { type PagedFetchPage, useRoutePagedList } from "../lib/routeQuery";
import { listMessages } from "../lib/serverApi";
import type { FreeTextTerm, Message, MessageSearch } from "../lib/types";
import { useDebouncedQuery } from "../lib/useDebouncedQuery";

/** Rows read at a time as the person scrolls. */
const PAGE_SIZE = 40;

const RELEVANCE: SortField<MessageSearchSortKey> = { id: "relevance", label: "Relevance" };
const DATE: SortField<MessageSearchSortKey> = { id: "date", label: "Date" };

/** No terms to bold: one shared value, so a row's memo sees the same array. */
const NO_TERMS: readonly FreeTextTerm[] = [];

/** What a page of `GET /v1/messages` says beside its rows: the server's reading of the search. */
type MessagesPageExtra = { search: MessageSearch };

/**
 * The Messages list: one row per message the search matches, from every
 * conversation (#313). It reads 40 rows at a time as the person scrolls and
 * shows the total, up to the route's offset ceiling of 50,000. An empty
 * search lists nothing and asks for one.
 *
 * The server picks the order unless the person picked one, and every page
 * says which order it applied and which free-text terms it ranks by (#1538).
 * The sort menu shows that order, offers Relevance only when terms came
 * back, and the rows bold those terms, so nothing here parses the search to
 * guess what the server will do. A pick is the caller's to keep, so it
 * survives opening a result.
 */
export default function MessageSearchList({
  query,
  sortPick,
  onSortPick,
  selectedId,
  onSelect,
}: {
  query: string;
  sortPick: MessageSearchSort | null;
  onSortPick: (next: MessageSearchSort) => void;
  selectedId: number | null;
  onSelect: (message: Message) => void;
}) {
  const debouncedQ = useDebouncedQuery(query);
  const q = debouncedQ.trim();

  if (!q) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <ListRangeHeader />
        <div className="p-4 text-[0.813rem] text-muted">
          Type a search above to list the messages it matches.
        </div>
      </div>
    );
  }

  const sortParam = sortPick ? messageSortParam(sortPick) : null;
  return (
    <MessageResults
      key={`${q}\u0000${sortParam ?? ""}`}
      q={q}
      sortParam={sortParam}
      sortPick={sortPick}
      onSortPick={onSortPick}
      selectedId={selectedId}
      onSelect={onSelect}
    />
  );
}

function MessageResults({
  q,
  sortParam,
  sortPick,
  onSortPick,
  selectedId,
  onSelect,
}: {
  q: string;
  /** The `sort` to send, or null to let the server pick. */
  sortParam: MessageSortParam | null;
  sortPick: MessageSearchSort | null;
  onSortPick: (next: MessageSearchSort) => void;
  selectedId: number | null;
  onSelect: (message: Message) => void;
}) {
  const fetchPage = useCallback<PagedFetchPage<Message, MessagesPageExtra>>(
    async ({ limit, offset, signal }) => {
      const res = await listMessages(
        sortParam ? { q, sort: sortParam, limit, offset } : { q, limit, offset },
        { signal },
      );
      return { items: res.items, total: res.total, search: res.search };
    },
    [q, sortParam],
  );
  const { items, total, lastPage, loading, refreshing, filling, error, hasMore, loadMore } =
    useRoutePagedList(keys.messages.list(q, sortParam ?? ""), fetchPage, {
      firstPageSize: PAGE_SIZE,
      fillPageSize: PAGE_SIZE,
      maxOffset: MAX_LIST_OFFSET,
    });

  const search = lastPage?.search ?? null;
  const terms = search?.terms ?? NO_TERMS;
  // Until the first page says which order it is in, the menu shows the
  // person's pick, or nothing when the server is picking.
  const sort = search ? messageSortFromParam(search.sort) : sortPick;
  const sortMenu = sort ? (
    <SortMenu
      fields={terms.length > 0 ? [RELEVANCE, DATE] : [DATE]}
      unordered={["relevance"]}
      sort={sort.sort}
      order={sort.order}
      onChange={onSortPick}
      itemNoun="messages"
      ascLabel="Oldest first"
      descLabel="Newest first"
    />
  ) : null;
  if (error && items.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <ListRangeHeader actions={sortMenu} />
        <div className="p-4 text-[0.813rem] text-danger">
          {apiErrorMessage(error, "Could not load messages.")}
        </div>
      </div>
    );
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ListRangeHeader
        rangeLabel={loading && items.length === 0 ? "Loading…" : countOf(total, "message")}
        refreshing={refreshing}
        filling={filling}
        actions={sortMenu}
      />
      <VirtualList
        count={items.length}
        estimateSize={72}
        dynamicSize
        onNearEnd={() => {
          if (hasMore) loadMore();
        }}
        empty={!loading ? <div className="p-4 text-[0.813rem] text-muted">No messages</div> : null}
        renderItem={(index) => {
          const m = items[index];
          if (!m) return null;
          return (
            <MessageSearchRow
              message={m}
              terms={terms}
              isSelected={m.id === selectedId}
              onClick={() => onSelect(m)}
            />
          );
        }}
      />
    </div>
  );
}
