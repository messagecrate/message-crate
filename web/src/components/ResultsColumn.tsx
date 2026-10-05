import { type Key, ToggleButton, ToggleButtonGroup } from "react-aria-components";
import { useNavigate, useSearchParams } from "react-router-dom";
import { messageSortParam } from "../lib/messageSearchSort";
import {
  AT_PARAM,
  MESSAGE_SORT_PARAM,
  messagesSearch,
  openedAt,
  otherResultsView,
  pickedMessageSort,
  type ResultsView,
  resultsView,
  TAG_PARAM,
  VIEW_PARAM,
} from "../lib/resultsView";
import { SEARCH_LIST_NAMES, useListQuery } from "../lib/searchFields";
import type { Conversation } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import ConversationList from "../screens/ConversationList";
import MessageSearchList from "../screens/MessageSearchList";
import { LIST_TOOLBAR_CLASS } from "./ListRangeHeader";

const SWITCH_BUTTON = `flex-1 cursor-pointer border-none bg-transparent px-2 py-0.5 text-[0.75rem] font-medium text-muted hover:text-text ${focusRing} data-[selected]:bg-accent data-[selected]:text-sent-text`;

/** The Conversations / Messages switch at the top of the Messages screen's results. */
export function ResultsViewSwitch({
  view,
  onChange,
}: {
  view: ResultsView;
  onChange: (view: ResultsView) => void;
}) {
  return (
    // The toolbar's height, so the switch lines up with the sidebar's spacer
    // and the conversation panel's toolbar beside it.
    <div className={LIST_TOOLBAR_CLASS}>
      <ToggleButtonGroup
        aria-label="Show results as"
        selectionMode="single"
        disallowEmptySelection
        selectedKeys={[view]}
        onSelectionChange={(keys: Set<Key>) => {
          const next = [...keys][0];
          if (next === "conversations" || next === "messages") onChange(next);
        }}
        className="flex flex-1 overflow-hidden rounded-md border border-border bg-elevated"
      >
        <ToggleButton id="conversations" className={SWITCH_BUTTON}>
          Conversations
        </ToggleButton>
        <ToggleButton id="messages" className={SWITCH_BUTTON}>
          Messages
        </ToggleButton>
      </ToggleButtonGroup>
    </div>
  );
}

/**
 * The Messages screen's results: the switch, then the conversations the
 * search matches or the messages it matches (#313). The header search drives
 * both. Which one, the sort picked in the Messages list, and the message a
 * result opened at all ride in the address, so opening a result keeps the
 * list as it was.
 *
 * A word only the other list takes, such as `from:` while Conversations is
 * picked, stays in the box, marked there, and each list searches with the
 * words it takes (#1561). Opening a result carries the search as typed, so
 * switching back finds the word again.
 */
export default function ResultsColumn({
  query,
  tag,
  searchTyped,
  selectedConversationId,
  onSelectConversation,
}: {
  /** The search both lists run: the typed search, with a page's tag or filter in it. */
  query: string;
  /**
   * The Message Tag in `query`, or `none` on the No Message Tag page, or
   * null. A result opened from the Messages list carries it apart from `q`
   * (#1562).
   */
  tag: string | null;
  /**
   * Whether the header box holds a search. Without one the Messages list asks
   * for a search, even on a tag page whose `query` holds the tag (#313: an
   * empty search shows no messages).
   */
  searchTyped: boolean;
  selectedConversationId: number | null;
  onSelectConversation: (conversation: Conversation) => void;
}) {
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const view = resultsView(searchParams);
  const { listQuery, marked, ready } = useListQuery(query, view, otherResultsView(view));

  const setParam = (key: string, value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value) next.set(key, value);
    else next.delete(key);
    setSearchParams(next, { replace: true });
  };

  return (
    <>
      <ResultsViewSwitch
        view={view}
        onChange={(next) => setParam(VIEW_PARAM, next === "messages" ? "messages" : "")}
      />
      {!ready ? (
        // Until both lists' words are known, a word the list does not take
        // cannot be told apart from one it does, and the server would refuse it.
        <div role="status" className="p-4 text-[0.813rem] text-muted">
          Loading…
        </div>
      ) : view === "messages" && searchTyped && marked.length > 0 && listQuery.trim() === "" ? (
        // Every word was left out. The Messages list would ask for a search
        // the box already holds, so say why there is nothing to list.
        <div role="status" className="p-4 text-[0.813rem] text-muted">
          Every word of this search works only in {SEARCH_LIST_NAMES[otherResultsView(view)]}, so
          there is nothing to search here.
        </div>
      ) : view === "messages" ? (
        <MessageSearchList
          query={searchTyped ? listQuery : ""}
          sortPick={pickedMessageSort(searchParams)}
          onSortPick={(next) => setParam(MESSAGE_SORT_PARAM, messageSortParam(next))}
          selectedId={openedAt(searchParams)}
          onSelect={(message) =>
            // `q` stays what the person typed, and the tag rides beside it,
            // so the conversation route lists what this list searched.
            navigate(
              `/messages/${message.conversation.id}${messagesSearch(searchParams, {
                [TAG_PARAM]: tag ?? "",
                [AT_PARAM]: String(message.id),
              })}`,
            )
          }
        />
      ) : (
        <ConversationList
          selectedId={selectedConversationId}
          onSelect={onSelectConversation}
          query={listQuery}
        />
      )}
    </>
  );
}
