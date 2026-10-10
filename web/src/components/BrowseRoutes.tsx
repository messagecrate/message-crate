/**
 * The elements of the routes under the app layout, one per kind of page. Each
 * reads its own address, renders the columns beside the navigation panel, and
 * declares to the layout what the header searches (`useLayoutSection`).
 */

import { type ReactNode, useCallback, useState } from "react";
import { useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { groupFromSlug } from "../lib/contactGroups";
import { tagFromSlug } from "../lib/messageTags";
import {
  conversationListQuery,
  listedTag,
  MATCHED_PARAM,
  MESSAGE_SORT_PARAM,
  messagesSearch,
  resultsView,
  TAG_PARAM,
  VIEW_PARAM,
} from "../lib/resultsView";
import { useListQuery } from "../lib/searchFields";
import { trashed } from "../lib/searchQuery";
import type { Conversation } from "../lib/types";
import { useContactGroups } from "../lib/useContactGroups";
import { useMessageTags } from "../lib/useMessageTags";
import ContactList from "../screens/ContactList";
import ConversationList from "../screens/ConversationList";
import CheckedContactsPanel from "./CheckedContactsPanel";
import ContactDrawer from "./ContactDrawer";
import {
  type ContactListPreviewSource,
  type ContactPreview,
  contactPreviewFromListRow,
  sameContactPreviews,
} from "./contactDrawer/contactDrawerTypes";
import ListColumn from "./ListColumn";
import { useAppLayout, useLayoutSection, useReplaceSearchParams } from "./layoutSection";
import MessageRoute from "./MessageRoute";
import ResultsColumn from "./ResultsColumn";
import RightPane from "./RightPane";

/** Scrollable content column that hosts the routed screen. */
const mainPane = "min-w-0 flex-1 overflow-auto bg-bg text-text";

/** Centered placeholder when a column has nothing selected yet. */
const emptyMain = "flex h-full items-center justify-center text-[0.875rem] text-muted";

/**
 * Why a Contact Group or Message Tag page has no list: the sets have not
 * loaded yet, or no set has the name in the link. A page in either state
 * lists nothing, so it never shows every contact or conversation under the
 * set's name, and nothing on it exports them.
 */
type SetPageState = "loading" | "missing" | null;

function setPageState(slug: string | null, loading: boolean, found: string | null): SetPageState {
  if (slug === null) return null;
  if (loading) return "loading";
  return found === null ? "missing" : null;
}

/** What the list column says in place of a list for a set page it cannot show. */
function SetPageStatus({
  state,
  setLabel,
  setsLabel,
  name,
}: {
  state: "loading" | "missing";
  setLabel: string;
  setsLabel: string;
  name: string;
}) {
  return (
    <div role="status" className="p-4 text-[0.813rem] text-muted">
      {state === "loading" ? `Loading ${setsLabel}…` : `There is no ${setLabel} named ${name}.`}
    </div>
  );
}

/**
 * The section of a route that shows conversations: the header searches the
 * Conversations or Messages list, whichever is shown, and Export starts from
 * the conversation list's query (`listQuery`), or from nothing when the page
 * has no list.
 */
function useConversationsSection({
  listQuery,
  hasList,
  onSearch,
}: {
  listQuery: string;
  hasList: boolean;
  onSearch: (q: string) => void;
}) {
  const [searchParams] = useSearchParams();
  const replaceSearchParams = useReplaceSearchParams();
  // Export runs on the Conversations list, so it leaves out the words the
  // Conversations list leaves out (#1561). Until the lists' words are known,
  // nothing is known to leave out, and Export starts from the search as typed.
  const exportQuery = useListQuery(listQuery, "conversations", "messages");
  useLayoutSection({
    search: {
      target: resultsView(searchParams) === "messages" ? "messages" : "conversations",
      query: searchParams.get("q") || "",
    },
    onSearchChange: (q) => {
      // The versions a result was found by belong to the search that found it.
      const matched =
        q === (searchParams.get("q") || "") ? (searchParams.get(MATCHED_PARAM) ?? "") : "";
      replaceSearchParams({ q, f: "", [MATCHED_PARAM]: matched });
    },
    onSearch,
    browseQuery: hasList ? (exportQuery.ready ? exportQuery.listQuery : listQuery) : "",
  });
}

/**
 * The conversation list: every conversation (`/`), those with no Message Tag
 * (`/no-tag`, `untagged`), or those with the tag a `/tag/:slug` page names.
 */
export function ConversationsRoute({ untagged = false }: { untagged?: boolean }) {
  const { slug = null } = useParams<{ slug: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { tags, loading: tagsLoading } = useMessageTags();
  const activeTag = slug !== null ? tagFromSlug(slug, tags) : null;
  const tagPage = setPageState(slug, tagsLoading, activeTag);
  const tagFilter = untagged ? "none" : activeTag;

  const conversationSearch = searchParams.get("q") || "";
  const conversationFilter = searchParams.get("f") || "";
  const listQuery = conversationListQuery(searchParams, tagFilter);

  useConversationsSection({
    listQuery,
    hasList: tagPage === null,
    onSearch: (q) => navigate(`${location.pathname}${messagesSearch(searchParams, { q, at: "" })}`),
  });

  // The message route filters its list by `q`, `f` and the tag, so the
  // conversation opened carries the list's query in them. Its path has no
  // tag, so a tag page's tag rides in its own parameter, and `q` stays what
  // the person typed (#1562).
  const handleConversationSelect = (c: Conversation) => {
    const params = new URLSearchParams();
    if (conversationSearch) params.set("q", conversationSearch);
    if (conversationFilter) params.set("f", conversationFilter);
    if (tagFilter) params.set(TAG_PARAM, tagFilter);
    // The results view and the Messages list's picked sort stay for when the
    // person switches back.
    for (const key of [VIEW_PARAM, MESSAGE_SORT_PARAM]) {
      const value = searchParams.get(key);
      if (value) params.set(key, value);
    }
    const search = params.toString();
    navigate(`/messages/${c.id}${search ? `?${search}` : ""}`, { state: { conversation: c } });
  };

  return (
    <>
      <ListColumn>
        {tagPage ? (
          <SetPageStatus
            state={tagPage}
            setLabel="Message Tag"
            setsLabel="Message Tags"
            name={slug ?? ""}
          />
        ) : (
          <ResultsColumn
            query={listQuery}
            tag={tagFilter}
            searchTyped={conversationSearch !== ""}
            selectedConversationId={null}
            onSelectConversation={handleConversationSelect}
          />
        )}
      </ListColumn>
      <RightPane>
        <main className={mainPane}>
          <div className={emptyMain}>Select a conversation to view messages</div>
        </main>
      </RightPane>
    </>
  );
}

/**
 * One open conversation (`/messages/:conversationId`), beside the list it was
 * opened from. On a conversation opened from a tag page, the tag rides in the
 * address apart from `q`, so Export starts from it too.
 */
export function OpenConversationRoute() {
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  useConversationsSection({
    listQuery: conversationListQuery(searchParams, listedTag(searchParams)),
    hasList: true,
    // The open conversation stays open, at the message a result opened it
    // at, and the results keep their list and sort.
    onSearch: (q) =>
      navigate(`${location.pathname}${messagesSearch(searchParams, { q })}`, {
        state: location.state,
      }),
  });
  return <MessageRoute />;
}

/**
 * The contact list: every contact (`/contacts`), those in no Contact Group
 * (`/no-group`, group `"none"`), the unknown ones (`/unknown`, group
 * `"unknown"`), or those in the group a `/group/:slug` page names.
 */
export function ContactsRoute({ group }: { group?: "none" | "unknown" }) {
  const { slug = null } = useParams<{ slug: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const replaceSearchParams = useReplaceSearchParams();
  const { selectedContact, selectContact, closeContactDrawer, browseContactConversations } =
    useAppLayout();
  const { groups, loading: groupsLoading } = useContactGroups();
  const activeGroup = slug !== null ? groupFromSlug(slug, groups) : null;
  const groupPage = setPageState(slug, groupsLoading, activeGroup);
  // "unknown" reaches the server as `group:unknown`, which it answers from
  // contact state rather than from stored membership.
  const groupFilter = group ?? activeGroup;
  const contactSearch = searchParams.get("cq") || "";

  useLayoutSection({
    search: { target: "contacts", query: contactSearch },
    onSearchChange: (q) => replaceSearchParams({ cq: q }),
    onSearch: (q) => navigate(`${location.pathname}${q ? `?cq=${encodeURIComponent(q)}` : ""}`),
    browseQuery: "",
  });

  const [checkedContacts, setCheckedContacts] = useState<ContactPreview[]>([]);
  const [clearCheckedRev, setClearCheckedRev] = useState(0);
  const handleCheckedContacts = useCallback((contacts: ContactListPreviewSource[]) => {
    // The child re-maps its checked rows on every render, so store only when the
    // value actually changed — otherwise each store schedules the next render.
    setCheckedContacts((prev) => {
      const next = contacts.map(contactPreviewFromListRow);
      return sameContactPreviews(prev, next) ? prev : next;
    });
  }, []);
  const clearCheckedContacts = useCallback(() => {
    setClearCheckedRev((n) => n + 1);
  }, []);

  return (
    <>
      <ListColumn>
        {groupPage ? (
          <SetPageStatus
            state={groupPage}
            setLabel="Contact Group"
            setsLabel="Contact Groups"
            name={slug ?? ""}
          />
        ) : (
          <ContactList
            filter={contactSearch}
            groupFilter={groupFilter}
            selectedId={selectedContact?.id ?? null}
            onSelect={(c) => selectContact(contactPreviewFromListRow(c))}
            onCheckedChange={handleCheckedContacts}
            clearCheckedRev={clearCheckedRev}
          />
        )}
      </ListColumn>
      <RightPane>
        {checkedContacts.length > 0 ? (
          <CheckedContactsPanel contacts={checkedContacts} onClear={clearCheckedContacts} />
        ) : selectedContact ? (
          <ContactDrawer
            variant="docked"
            contactId={selectedContact.id}
            preview={selectedContact}
            onClose={closeContactDrawer}
            onBrowseConversations={browseContactConversations}
          />
        ) : (
          <main className={mainPane}>
            <div className={emptyMain}>Select a contact to view details</div>
          </main>
        )}
      </RightPane>
    </>
  );
}

/**
 * Trash (`/trash`): the trashed conversations, and `children`, the Trash
 * screen, beside them. Clicking a trashed row sets `tsel` instead of
 * navigating to `/messages/:id`, so the Trash screen can offer Restore for it
 * without leaving the Trash list behind.
 */
export function TrashRoute({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const replaceSearchParams = useReplaceSearchParams();
  // Trash keeps its own term so leaving and returning to Trash does not inherit
  // whatever the inbox was last searching for.
  const trashSearch = searchParams.get("tq") || "";
  const selectedRaw = searchParams.get("tsel");
  const selectedId = selectedRaw && /^\d+$/.test(selectedRaw) ? Number(selectedRaw) : null;

  useLayoutSection({
    search: { target: "trash", query: trashSearch },
    // Narrowing Trash can filter the selected row out of the left column, so
    // the selection goes with the search rather than leaving the Restore
    // panel pointed at a conversation the list no longer shows.
    onSearchChange: (q) => replaceSearchParams({ tq: q, tsel: "" }),
    onSearch: (q) => navigate(`/trash${q ? `?tq=${encodeURIComponent(q)}` : ""}`),
    browseQuery: "",
  });

  return (
    <>
      <ListColumn>
        <ConversationList
          selectedId={selectedId}
          onSelect={(c) => replaceSearchParams({ tsel: String(c.id) })}
          query={trashed(trashSearch)}
        />
      </ListColumn>
      <RightPane>
        <main className={mainPane}>{children}</main>
      </RightPane>
    </>
  );
}

/**
 * A screen with no list, such as Settings: it takes the whole width beside
 * the navigation panel, and declares no section, so the header offers no
 * search. Export carries `?q=` for its own scope box, which typing in the
 * header must never change (#1568).
 */
export function FullScreenRoute({ children }: { children: ReactNode }) {
  return <main className={mainPane}>{children}</main>;
}
