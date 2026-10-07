"use client";

import type {
  ContactDetail,
  ContactListItem,
  ContactSection,
  GroupChatThread,
  GroupParticipant,
  YearThread,
} from "@/lib/types";
import {
  GROUP_CHAT_SORT_ALLOWED,
  GROUP_CHAT_SORT_KEY,
  GROUP_CHAT_SORT_ORDER_KEY,
  SORT_ORDER_ALLOWED,
} from "@/lib/groupChatList";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useVaultSearch } from "./useVaultSearch";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import type { SearchConversationHit, SearchMessageHit } from "@/lib/search";
import { parseSearchQuery } from "@/lib/searchQuery";
import { isDeletionUiBlocked, isDeletionUiEnabled } from "@/lib/v1Capabilities";
import { ThreadFindBar } from "./ThreadFindBar";
import { useThreadFind } from "./useThreadFind";
import {
  applySearchResultRangeSelect,
  applySearchRangeSelect,
  orderedSearchContactIds,
  orderedSearchResultKeys,
  searchResultKey,
  selectedSearchResultGroups,
  type SearchResultKey,
} from "@/lib/searchSelection";
import { seedContactEditDraft } from "./contactEdit";
import {
  BrowseContactCtxMenu,
  BrowseMergeIntoPanel,
} from "./BrowseContactCtxMenu";
import type { CollapsedGroupConversation } from "@/lib/groupChatList";
import { BrowseDetailsInspector } from "./BrowseDetailsInspector";
import { BrowsePeopleTreePane } from "./BrowsePeopleTreePane";
import { BrowseThreadColumn } from "./BrowseThreadColumn";
import { SearchMessageCtxMenu } from "./SearchMessageCtxMenu";
import { useBrowsePeopleTree } from "./useBrowsePeopleTree";
import {
  contactFormAnchorFromRect,
  type ContactFormAnchor,
} from "./ContactFormOverlay";
import {
  createGroupChatTrashOptions,
  groupChatToastTitle,
} from "./groupChatTrash";
import { LabelsMenu } from "./LabelsMenu";
import { useHistory } from "./history";
import {
  trashContactsLabel,
  trashMessageThreadsLabel,
} from "./history/historyTypes";
import { ParticipantContactFormOverlay } from "./ParticipantContactFormOverlay";
import { VcfImportPreviewDialog } from "./VcfImportPreviewDialog";
import type {
  VcfCategoryMapping,
  VcfImportPreview,
} from "@/lib/contactsVcfImport";
import {
  type BrowseGroupChatSortBy,
  type SortMode,
  type SortOrder,
} from "./SortByMenu";
import { useSourceFilter } from "./SourceFilter";
import {
  useBrowseContactListBase,
  useBrowseContactListView,
} from "./useBrowseContactList";
import { useBrowseLabelMembership } from "./useBrowseLabelMembership";
import { useCollapsedGroupChatList } from "./useCollapsedGroupChatList";
import { useListSelection } from "./useListSelection";
import { useParticipantContactForm } from "./useParticipantContactForm";
import { useTrashActions } from "./useTrashActions";
import { useProgressiveThreadMessages } from "./useProgressiveThreadMessages";
import { useDismissible } from "./useDismissible";
import { usePersistedEnum } from "./usePersistedEnum";
import { PaneSeparator } from "./PaneSeparator";
import { usePanelLayoutStorage } from "./panelLayoutStorage";
import { Group, Panel, useDefaultLayout, usePanelRef } from "react-resizable-panels";

const INSPECTOR_PANEL_COLLAPSED_KEY = "mv-browse-inspector-collapsed";

const SORT_MODE_KEY = "mv-contact-sort";
const SORT_ORDER_KEY = "mv-contact-sort-order";
const SORT_MODE_ALLOWED = [
  "first",
  "last",
  "messages",
  "group-messages",
  "phone",
] as const;

export function BrowseShell({
  paneStorageKey,
  sectionLabel,
  contactSection,
  contacts,
  allLabels = [],
  initialContactId,
}: {
  paneStorageKey: string;
  sectionLabel: string;
  contactSection: ContactSection;
  contacts: ContactListItem[];
  allLabels?: string[];
  initialContactId: number | null;
}) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const { push: pushHistory, revision: historyRevision } = useHistory();
  const { sources, source, setSource, sourceQuery } = useSourceFilter();
  const [sortMode, setSortMode] = usePersistedEnum(
    SORT_MODE_KEY,
    SORT_MODE_ALLOWED,
    "last",
  );
  const [sortOrder, setSortOrder] = usePersistedEnum(
    SORT_ORDER_KEY,
    SORT_ORDER_ALLOWED,
    "asc",
  );
  const setSort = useCallback(
    (next: { sort: SortMode; order: SortOrder }) => {
      setSortMode(next.sort);
      setSortOrder(next.order);
    },
    [setSortMode, setSortOrder],
  );
  const sort = sortMode;
  const [query, setQuery] = useState("");
  const urlContactId = useMemo(() => {
    const raw = searchParams.get("c");
    if (!raw) return null;
    const n = Number(raw);
    return Number.isFinite(n) ? n : null;
  }, [searchParams]);
  const [contactId, setContactId] = useState<number | null>(
    () => initialContactId ?? urlContactId,
  );
  useEffect(() => {
    setContactId((prev) => (prev === urlContactId ? prev : urlContactId));
  }, [urlContactId]);
  const [detail, setDetail] = useState<ContactDetail | null>(null);
  const [yearly, setYearly] = useState<YearThread[]>([]);
  const [groupChats, setGroupChats] = useState<GroupChatThread[]>([]);
  const [messageSources, setMessageSources] = useState<string[]>([]);
  const [sourceCounts, setSourceCounts] = useState<{
    all: number;
    bySource: Record<string, number>;
  }>({ all: 0, bySource: {} });
  const [threadConversationIds, setThreadConversationIds] = useState<
    number[] | null
  >(null);
  const [activeThread, setActiveThread] = useState<string | null>(null);
  const [loadingThreads, setLoadingThreads] = useState(false);
  const loadedContactIdRef = useRef<number | null>(null);
  /** State mirror of loadedContactIdRef so the thread pane can tell "empty" from "still loading". */
  const [threadsLoadedFor, setThreadsLoadedFor] = useState<number | null>(null);
  const activeThreadRef = useRef<string | null>(null);
  activeThreadRef.current = activeThread;
  /** False after URL/hydration restore so Panel 4 stays empty until a click. */
  const allowAutoOpenThreadRef = useRef(initialContactId == null);
  const activeSourceRef = useRef<string | null>(null);
  activeSourceRef.current = source;
  const cancelContactFormRef = useRef<() => void>(() => {});

  const [saving, setSaving] = useState(false);
  const [labelOverrides, setLabelOverrides] = useState<Map<number, string[]>>(
    () => new Map(),
  );
  const labelOverridesRef = useRef(labelOverrides);
  labelOverridesRef.current = labelOverrides;
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [threadsEpoch, setThreadsEpoch] = useState(0);
  const [ctxMenu, setCtxMenu] = useState<{
    id: number;
    x: number;
    y: number;
  } | null>(null);
  const [mergeFromId, setMergeFromId] = useState<number | null>(null);
  const [mergeQuery, setMergeQuery] = useState("");
  const [mergePos, setMergePos] = useState<{ x: number; y: number } | null>(
    null,
  );
  const [toolbarLabelsPos, setToolbarLabelsPos] = useState<{
    x: number;
    y: number;
  } | null>(null);
  const ctxMenuRef = useRef<HTMLDivElement>(null);
  const searchResultCtxMenuRef = useRef<HTMLDivElement>(null);
  const mergePanelRef = useRef<HTMLDivElement>(null);
  const pendingEditIdRef = useRef<number | null>(null);
  const statusShowTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const statusClearTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const storage = usePanelLayoutStorage();
  const mainLayout = useDefaultLayout({
    id: "mv-browse-main-v3",
    panelIds: ["tree", "thread", "inspector"],
    storage,
  });
  const inspectorPanelRef = usePanelRef();
  const [inspectorCollapsed, setInspectorCollapsed] = useState(() => {
    if (typeof window === "undefined") return false;
    return window.localStorage.getItem(INSPECTOR_PANEL_COLLAPSED_KEY) === "1";
  });
  const persistInspectorCollapsed = useCallback(() => {
    const panel = inspectorPanelRef.current;
    if (!panel) return;
    const collapsed = panel.isCollapsed();
    window.localStorage.setItem(
      INSPECTOR_PANEL_COLLAPSED_KEY,
      collapsed ? "1" : "0",
    );
    setInspectorCollapsed(collapsed);
  }, [inspectorPanelRef]);
  useLayoutEffect(() => {
    const panel = inspectorPanelRef.current;
    if (!panel) return;
    if (inspectorCollapsed && !panel.isCollapsed()) panel.collapse();
    if (!inspectorCollapsed && panel.isCollapsed()) panel.expand();
  }, [inspectorCollapsed, inspectorPanelRef]);
  const [groupChatSortBy, setGroupChatSortBy] = usePersistedEnum(
    GROUP_CHAT_SORT_KEY,
    GROUP_CHAT_SORT_ALLOWED,
    "date",
  );
  const [groupChatSortOrder, setGroupChatSortOrder] = usePersistedEnum(
    GROUP_CHAT_SORT_ORDER_KEY,
    SORT_ORDER_ALLOWED,
    "desc",
  );
  const setGroupChatSort = useCallback(
    (next: { sortBy: BrowseGroupChatSortBy; order: SortOrder }) => {
      setGroupChatSortBy(next.sortBy);
      setGroupChatSortOrder(next.order);
    },
    [setGroupChatSortBy, setGroupChatSortOrder],
  );
  const [groupChatFilterYear, setGroupChatFilterYear] = useState<number | null>(
    null,
  );
  const initialVaultQuery = searchParams.get("q") ?? "";
  const vaultSearch = useVaultSearch(initialVaultQuery);
  const [scrollToMessageId, setScrollToMessageId] = useState<number | null>(
    null,
  );
  const [scrollToMessageNonce, setScrollToMessageNonce] = useState(0);
  const [focusedSearchMessageId, setFocusedSearchMessageId] = useState<
    number | null
  >(null);
  const [contextLoadIds, setContextLoadIds] = useState<number[]>([]);
  const [focusedSearchHit, setFocusedSearchHit] =
    useState<SearchConversationHit | null>(null);
  const [selectedSearchResultKeys, setSelectedSearchResultKeys] = useState<
    Set<SearchResultKey>
  >(() => new Set());
  const searchResultAnchorRef = useRef<SearchResultKey | null>(null);
  const [searchResultCtxMenu, setSearchResultCtxMenu] = useState<{
    hit: SearchConversationHit;
    x: number;
    y: number;
  } | null>(null);
  const [selectedGroupConversationId, setSelectedGroupConversationId] =
    useState<number | null>(null);
  const [selectionGroupChats, setSelectionGroupChats] = useState<
    GroupChatThread[]
  >([]);
  const [loadingSelectionGroups, setLoadingSelectionGroups] = useState(false);
  const pendingConvIdRef = useRef<number | null>(
    (() => {
      const raw = searchParams.get("conv");
      if (!raw) return null;
      const n = Number(raw);
      return Number.isFinite(n) ? n : null;
    })(),
  );
  const peopleTree = useBrowsePeopleTree({
    sourceQuery,
    reloadToken: threadsEpoch,
  });
  const {
    expandContact,
    bundle: peopleTreeBundle,
    expandedContactId: peopleTreeExpandedId,
    loading: peopleTreeLoading,
    patchCachedDetail: _patchCachedDetail,
  } = peopleTree;
  void _patchCachedDetail;

  const {
    visibleContacts,
    sortedRaw,
    selectAllIds,
    compareContacts,
  } = useBrowseContactListBase({
    contacts,
    sort,
    sortOrder,
    query,
  });

  const selectContactRef = useRef<(id: number) => void>(() => {});

  // Search spans every contact, so matches outside this section still need to be
  // selectable (and labelable) even though the list never loaded them.
  const searchContactsById = useMemo(() => {
    const map = new Map<number, ContactListItem>();
    for (const hit of vaultSearch.contactHits) {
      map.set(hit.contact.id, hit.contact);
    }
    return map;
  }, [vaultSearch.contactHits]);

  const validIds = useMemo(
    () => [...new Set([...contacts.map((c) => c.id), ...searchContactsById.keys()])],
    [contacts, searchContactsById],
  );
  const validIdSet = useMemo(() => new Set(validIds), [validIds]);

  /** Apply the Contacts sort menu to Show-contact search results. */
  const sortedSearchContactHits = useMemo(() => {
    const hits = [...vaultSearch.contactHits];
    hits.sort((a, b) => compareContacts(a.contact, b.contact));
    return hits;
  }, [vaultSearch.contactHits, compareContacts]);

  /** Flat hits: use contact sort when linked, else title / match count / handle. */
  const sortedSearchHits = useMemo(() => {
    const hits = [...vaultSearch.hits];
    const dir = sortOrder === "desc" ? -1 : 1;
    hits.sort((a, b) => {
      const aContact =
        a.contactId != null ? searchContactsById.get(a.contactId) : undefined;
      const bContact =
        b.contactId != null ? searchContactsById.get(b.contactId) : undefined;
      if (aContact && bContact) return compareContacts(aContact, bContact);
      if (aContact && !bContact) return -1;
      if (!aContact && bContact) return 1;

      let cmp = 0;
      if (sort === "messages" || sort === "group-messages") {
        cmp = a.matchCount - b.matchCount;
      } else if (sort === "phone") {
        cmp = a.chatIdentifier.localeCompare(b.chatIdentifier, undefined, {
          numeric: true,
          sensitivity: "base",
        });
      } else {
        cmp = a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
      }
      if (cmp !== 0) return cmp * dir;
      return (
        (b.dateEnd ?? "").localeCompare(a.dateEnd ?? "") ||
        a.conversationId - b.conversationId
      );
    });
    return hits;
  }, [
    vaultSearch.hits,
    searchContactsById,
    compareContacts,
    sort,
    sortOrder,
  ]);
  const orderedSearchResultIds = useMemo(
    () => orderedSearchResultKeys(sortedSearchHits),
    [sortedSearchHits],
  );
  const selectedSearchGroups = useMemo(
    () =>
      selectedSearchResultGroups(sortedSearchHits, selectedSearchResultKeys),
    [selectedSearchResultKeys, sortedSearchHits],
  );
  const allSearchResultsSelected =
    orderedSearchResultIds.length > 0 &&
    orderedSearchResultIds.every((key) => selectedSearchResultKeys.has(key));

  const searchContactIds = useMemo(() => {
    if (vaultSearch.mode === "contacts") {
      return sortedSearchContactHits
        .map((hit) => hit.contact.id)
        .filter((id) => validIdSet.has(id));
    }
    return [...new Set(vaultSearch.contactIds)].filter((id) =>
      validIdSet.has(id),
    );
  }, [
    vaultSearch.mode,
    sortedSearchContactHits,
    vaultSearch.contactIds,
    validIdSet,
  ]);
  const [listOrderIds, setListOrderIds] = useState<number[]>([]);

  const {
    selectedIds,
    setSelectedIds,
    hasSelection,
    allSelected: allGroupSelected,
    selectAllRef,
    clearSelection: clearSelectionBase,
    toggleSelectAll: toggleSelectAllInGroup,
    onSelectColumnClick,
    onRowClick: onNamePhoneClick,
  } = useListSelection<number>({
    orderedIds:
      listOrderIds.length > 0 ? listOrderIds : sortedRaw.map((c) => c.id),
    selectAllIds,
    validIds,
    rangeMode: "selectionSpan",
    multiThreshold: "any",
    focusedId: contactId,
    // Name/phone always opens (and clears checks via onOpen). Avatar
    // column still toggles checkboxes once anything is selected, or on
    // plain click when using alwaysOpen with an empty selection.
    rowClickMode: "alwaysOpen",
    checkboxEvents: "preventAndStop",
    escapeToClear: false,
    selectAllSetsAnchor: false,
    onOpen: (id) => {
      // Opening a contact clears checkbox selection so the previous
      // contact doesn't stay highlighted as checked.
      setSelectedIds(new Set());
      selectContactRef.current(id);
    },
  });

  const allSearchContactsSelected =
    searchContactIds.length > 0 &&
    searchContactIds.every((id) => selectedIds.has(id));
  const orderedVisibleSearchContactIds = useMemo(
    () =>
      (vaultSearch.mode === "contacts"
        ? sortedSearchContactHits.map((hit) => hit.contact.id)
        : orderedSearchContactIds(sortedSearchHits)
      ).filter((id) => validIdSet.has(id)),
    [
      vaultSearch.mode,
      sortedSearchContactHits,
      sortedSearchHits,
      validIdSet,
    ],
  );
  const searchSelectionAnchorRef = useRef<number | null>(null);

  useEffect(() => {
    searchSelectionAnchorRef.current = null;
    searchResultAnchorRef.current = null;
    setSelectedIds(new Set());
    setSelectedSearchResultKeys(new Set());
    setSearchResultCtxMenu(null);
  }, [vaultSearch.committed, setSelectedIds]);

  const toggleSearchContact = useCallback(
    (id: number, mods?: { shiftKey: boolean }) => {
      if (!validIdSet.has(id)) return;
      if (mods?.shiftKey) {
        setSelectedIds(
          applySearchRangeSelect(
            orderedVisibleSearchContactIds,
            id,
            searchSelectionAnchorRef.current,
          ),
        );
        return;
      }
      setSelectedIds((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
      searchSelectionAnchorRef.current = id;
    },
    [orderedVisibleSearchContactIds, setSelectedIds, validIdSet],
  );
  const toggleSelectAllSearchContacts = useCallback(() => {
    setSelectedIds((prev) => {
      if (searchContactIds.every((id) => prev.has(id))) {
        searchSelectionAnchorRef.current = null;
        return new Set();
      }
      searchSelectionAnchorRef.current =
        orderedVisibleSearchContactIds[0] ?? searchContactIds[0] ?? null;
      return new Set(searchContactIds);
    });
  }, [orderedVisibleSearchContactIds, searchContactIds, setSelectedIds]);
  const toggleSearchResult = useCallback(
    (
      hit: SearchConversationHit,
      mods: {
        shiftKey: boolean;
        altKey: boolean;
        metaKey: boolean;
        ctrlKey: boolean;
      },
    ) => {
      const key = searchResultKey(hit);
      if (mods.shiftKey) {
        setSelectedSearchResultKeys(
          applySearchResultRangeSelect(
            orderedSearchResultIds,
            key,
            searchResultAnchorRef.current,
          ),
        );
      } else {
        setSelectedSearchResultKeys((prev) => {
          const next = new Set(prev);
          if (next.has(key)) next.delete(key);
          else next.add(key);
          return next;
        });
        searchResultAnchorRef.current = key;
      }
    },
    [orderedSearchResultIds],
  );
  const toggleSelectAllSearchResults = useCallback(() => {
    setSelectedSearchResultKeys((prev) => {
      if (orderedSearchResultIds.every((key) => prev.has(key))) {
        searchResultAnchorRef.current = null;
        return new Set();
      }
      searchResultAnchorRef.current = orderedSearchResultIds[0] ?? null;
      return new Set(orderedSearchResultIds);
    });
  }, [orderedSearchResultIds]);
  const searchConversationSections = useMemo(
    () =>
      selectedSearchGroups.map((group) => ({
        conversationIds: group.conversationIds,
        title: group.title,
        conversationType: group.conversationType,
      })),
    [selectedSearchGroups],
  );
  useEffect(() => {
    if (selectedSearchGroups.length === 0) return;
    setFocusedSearchHit(null);
    setFocusedSearchMessageId(null);
    setContextLoadIds([]);
    setSelectedGroupConversationId(null);
    setScrollToMessageId(null);
    setThreadConversationIds(
      [...new Set(selectedSearchGroups.flatMap((group) => group.conversationIds))],
    );
    setActiveThread("search-selection");
  }, [selectedSearchGroups]);

  const { sorted, grouped } = useBrowseContactListView({
    sortedRaw,
    visibleContacts,
    compareContacts,
    query,
    selectedIds,
    sort,
  });

  useLayoutEffect(() => {
    const next = sorted.map((c) => c.id);
    setListOrderIds((prev) => {
      if (
        prev.length === next.length &&
        prev.every((id, i) => id === next[i])
      ) {
        return prev;
      }
      return next;
    });
  }, [sorted]);

  const syncConvUrl = useCallback(
    (conversationId: number | null) => {
      const params = new URLSearchParams(searchParams.toString());
      if (conversationId != null) params.set("conv", String(conversationId));
      else params.delete("conv");
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams],
  );

  const selectContact = useCallback(
    (id: number) => {
      allowAutoOpenThreadRef.current = true;
      // Drop checkbox highlights on other contacts when focus moves.
      setSelectedIds(new Set());
      setSelectedGroupConversationId(null);
      setGroupChatFilterYear(null);
      cancelContactFormRef.current();
      pendingConvIdRef.current = null;
      if (id === contactId) {
        // Re-focus: reload and let the threads effect apply Direct-only auto-open.
        setThreadConversationIds(null);
        setActiveThread(null);
        syncConvUrl(null);
        expandContact(id, { force: true });
        setThreadsEpoch((e) => e + 1);
        return;
      }
      setContactId(id);
      // Keep prior detail only when it matches; inspector falls back to the
      // lightweight contact-list row until the matching bundle arrives.
      setDetail((prev) => (prev?.id === id ? prev : null));
      setYearly([]);
      setGroupChats([]);
      setMessageSources([]);
      setSourceCounts({ all: 0, bySource: {} });
      setThreadConversationIds(null);
      setActiveThread(null);
      setSelectedGroupConversationId(null);
      setScrollToMessageId(null);
      setFocusedSearchHit(null);
      setFocusedSearchMessageId(null);
      setContextLoadIds([]);
      loadedContactIdRef.current = null;
      setThreadsLoadedFor(null);
      expandContact(id);
      const params = new URLSearchParams(searchParams.toString());
      params.set("c", String(id));
      params.delete("h");
      params.delete("y");
      params.delete("conv");
      router.replace(`${pathname}?${params.toString()}`, { scroll: false });
    },
    [
      contactId,
      expandContact,
      pathname,
      router,
      searchParams,
      setSelectedIds,
      syncConvUrl,
    ],
  );
  selectContactRef.current = selectContact;

  const clearContactFocus = useCallback(() => {
    allowAutoOpenThreadRef.current = false;
    setContactId(null);
    setThreadConversationIds(null);
    setActiveThread(null);
    setSelectedGroupConversationId(null);
    expandContact(null);
    pendingConvIdRef.current = null;
    const params = new URLSearchParams(searchParams.toString());
    params.delete("c");
    params.delete("h");
    params.delete("y");
    params.delete("conv");
    const qs = params.toString();
    router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
  }, [expandContact, pathname, router, searchParams]);

  useEffect(() => {
    if (selectedIds.size === 0 || contactId == null) return;
    if (selectedIds.has(contactId)) return;
    clearContactFocus();
  }, [selectedIds, contactId, clearContactFocus]);

  useEffect(() => {
    return () => {
      if (statusShowTimerRef.current) clearTimeout(statusShowTimerRef.current);
      if (statusClearTimerRef.current) clearTimeout(statusClearTimerRef.current);
    };
  }, []);

  const queueStatusMessage = useCallback((message: string) => {
    if (statusShowTimerRef.current) clearTimeout(statusShowTimerRef.current);
    if (statusClearTimerRef.current) clearTimeout(statusClearTimerRef.current);
    statusShowTimerRef.current = setTimeout(() => {
      setStatusMsg(message);
      statusClearTimerRef.current = setTimeout(() => {
        setStatusMsg(null);
        statusClearTimerRef.current = null;
      }, 5000);
      statusShowTimerRef.current = null;
    }, 300);
  }, []);

  // Expand/collapse the people-tree cache when focus changes.
  useEffect(() => {
    if (contactId == null) {
      expandContact(null);
      return;
    }
    if (peopleTreeExpandedId !== contactId) {
      expandContact(contactId);
    }
  }, [contactId, expandContact, peopleTreeExpandedId]);

  // Apply cached/fetched thread bundles into BrowseShell state.
  useEffect(() => {
    if (!contactId) {
      loadedContactIdRef.current = null;
      allowAutoOpenThreadRef.current = false;
      setThreadsLoadedFor(null);
      setDetail(null);
      setYearly([]);
      setGroupChats([]);
      setMessageSources([]);
      setSourceCounts({ all: 0, bySource: {} });
      setThreadConversationIds(null);
      setActiveThread(null);
      setSelectedGroupConversationId(null);
      setLoadingThreads(false);
      return;
    }

    setLoadingThreads(peopleTreeLoading);
    const data = peopleTreeBundle;
    // Ignore bundles that belong to a different contact (stale cache/race).
    if (
      !data ||
      peopleTreeExpandedId !== contactId ||
      data.detail.id !== contactId
    ) {
      if (peopleTreeExpandedId === contactId && !peopleTreeLoading) {
        // Expanded for this contact but no valid bundle yet/failed.
        if (!data || data.detail.id !== contactId) {
          setDetail(null);
          setYearly([]);
          setGroupChats([]);
          setMessageSources([]);
          setSourceCounts({ all: 0, bySource: {} });
          setThreadConversationIds(null);
          setActiveThread(null);
          loadedContactIdRef.current = null;
          setThreadsLoadedFor(null);
        }
      } else if (loadedContactIdRef.current !== contactId) {
        // Switching contacts: clear previous person's inspector/reader shell
        // until the matching bundle arrives.
        setDetail(null);
        setYearly([]);
        setGroupChats([]);
        setMessageSources([]);
        setSourceCounts({ all: 0, bySource: {} });
        setThreadConversationIds(null);
        setActiveThread(null);
        setSelectedGroupConversationId(null);
        loadedContactIdRef.current = null;
        setThreadsLoadedFor(null);
      }
      return;
    }

    const switchingContact = loadedContactIdRef.current !== contactId;
    const hydrateOnly =
      !allowAutoOpenThreadRef.current &&
      activeThreadRef.current == null &&
      pendingConvIdRef.current == null;

    {
      const contact = data.detail;
      const ov = labelOverridesRef.current.get(contact.id);
      setDetail(ov ? { ...contact, labels: ov } : contact);
    }
    const nextYearly = data.yearly;
    const nextGroupChats = data.groupChats;
    setYearly(nextYearly);
    setGroupChats(nextGroupChats);
    setMessageSources(data.messageSources);
    setSourceCounts(data.sourceCounts);
    loadedContactIdRef.current = contactId;
    setThreadsLoadedFor(contactId);

    const available = data.messageSources;
    const selected = activeSourceRef.current;
    if (selected && !available.includes(selected)) {
      setSource(null);
    }

    const dmIds = [...new Set(nextYearly.flatMap((y) => y.conversationIds))];
    const hasGroups = nextGroupChats.length > 0;
    const pendingConv = pendingConvIdRef.current;

    if (pendingConv != null) {
      pendingConvIdRef.current = null;
      allowAutoOpenThreadRef.current = true;
      const group = nextGroupChats.find((t) => {
        const ids =
          t.conversationIds?.length > 0
            ? t.conversationIds
            : [t.conversationId];
        return ids.includes(pendingConv) || t.conversationId === pendingConv;
      });
      if (group) {
        const ids =
          group.conversationIds?.length > 0
            ? group.conversationIds
            : [group.conversationId];
        setSelectedGroupConversationId(group.conversationId);
        setActiveThread(`gfull-${ids.join("-")}`);
        setThreadConversationIds(ids);
        return;
      }
      if (dmIds.includes(pendingConv)) {
        setSelectedGroupConversationId(null);
        setActiveThread("dm");
        setThreadConversationIds(dmIds);
        return;
      }
    }

    if (hydrateOnly) {
      setThreadConversationIds(null);
      setActiveThread(null);
      setSelectedGroupConversationId(null);
      return;
    }

    setActiveThread((prev) => {
      if (switchingContact) return null;
      if (prev === "dm") return prev;
      if (prev?.startsWith("gfull-")) {
        const stillThere = nextGroupChats.some((t) => {
          const ids =
            t.conversationIds?.length > 0
              ? t.conversationIds
              : [t.conversationId];
          return `gfull-${ids.join("-")}` === prev;
        });
        return stillThere ? prev : null;
      }
      return null;
    });

    let key = switchingContact ? null : activeThreadRef.current;
    if (key?.startsWith("gfull-")) {
      const stillThere = nextGroupChats.some((t) => {
        const ids =
          t.conversationIds?.length > 0
            ? t.conversationIds
            : [t.conversationId];
        return `gfull-${ids.join("-")}` === key;
      });
      if (!stillThere) key = null;
    } else if (key !== "dm") {
      key = null;
    }

    if (switchingContact) {
      setSelectedGroupConversationId(null);
      if (dmIds.length > 0 && !hasGroups) {
        key = "dm";
        setActiveThread("dm");
      } else {
        setActiveThread(null);
        setThreadConversationIds(null);
        return;
      }
    } else if (!key) {
      if (dmIds.length > 0 && !hasGroups) {
        key = "dm";
        setActiveThread("dm");
      } else {
        setThreadConversationIds(null);
        return;
      }
    } else if (key === "dm" && dmIds.length === 0) {
      setActiveThread(null);
      setThreadConversationIds(null);
      return;
    }

    let convIds: number[] | null = null;
    if (key === "dm") {
      convIds = dmIds;
    } else if (key.startsWith("gfull-")) {
      const g = nextGroupChats.find((t) => {
        const ids =
          t.conversationIds?.length > 0
            ? t.conversationIds
            : [t.conversationId];
        return `gfull-${ids.join("-")}` === key;
      });
      if (g) {
        convIds =
          g.conversationIds?.length > 0
            ? g.conversationIds
            : [g.conversationId];
      }
    }
    if (!convIds?.length) {
      setThreadConversationIds(null);
      return;
    }
    setThreadConversationIds(convIds);
  }, [
    contactId,
    peopleTreeBundle,
    peopleTreeExpandedId,
    peopleTreeLoading,
    setSource,
  ]);

  const openThread = useCallback(
    (conversationIds: number[], key: string) => {
      allowAutoOpenThreadRef.current = true;
      setActiveThread(key);
      setThreadConversationIds(conversationIds);
      const convId =
        key === "dm"
          ? (conversationIds[0] ?? null)
          : (conversationIds[0] ?? null);
      syncConvUrl(convId);
    },
    [syncConvUrl],
  );

  const selectedContacts = useMemo(() => {
    const selected = new Set(selectedIds);
    const fromSorted = sorted.filter((c) => selected.has(c.id));
    if (fromSorted.length === selectedIds.size) return fromSorted;
    // Keep contacts that left the visible list after a filter or section change.
    const have = new Set(fromSorted.map((c) => c.id));
    const byId = new Map(contacts.map((c) => [c.id, c]));
    const extras: ContactListItem[] = [];
    for (const id of selectedIds) {
      if (have.has(id)) continue;
      const c = byId.get(id) ?? searchContactsById.get(id);
      if (c) extras.push(c);
    }
    return [...fromSorted, ...extras];
  }, [sorted, selectedIds, contacts, searchContactsById]);

  const trashIdsForContext = useCallback(
    (ctxId: number): number[] => {
      if (hasSelection && selectedIds.has(ctxId)) {
        return selectedContacts.map((c) => c.id);
      }
      return [ctxId];
    },
    [hasSelection, selectedIds, selectedContacts],
  );

  const selectionIdsKey = useMemo(
    () =>
      [...selectedIds]
        .sort((a, b) => a - b)
        .join(","),
    [selectedIds],
  );

  useEffect(() => {
    // Search selection is used for bulk contact actions. Shared-group lookup
    // is hidden in search mode and can exceed URL limits for large result sets.
    if (vaultSearch.resultsMode || !selectionIdsKey) {
      setSelectionGroupChats([]);
      setLoadingSelectionGroups(false);
      return;
    }
    let cancelled = false;
    setLoadingSelectionGroups(true);
    const params = new URLSearchParams({ ids: selectionIdsKey });
    if (source) params.set("source", source);
    fetch(`/api/contacts/shared-group-chats?${params.toString()}`)
      .then((r) => r.json())
      .then((data) => {
        if (cancelled) return;
        const next: GroupChatThread[] = data.groupChats ?? [];
        setSelectionGroupChats(next);
        const prev = activeThreadRef.current;
        if (prev?.startsWith("gfull-")) {
          const stillThere = next.some((t) => {
            const ids =
              t.conversationIds?.length > 0
                ? t.conversationIds
                : [t.conversationId];
            return `gfull-${ids.join("-")}` === prev;
          });
          if (!stillThere) {
            setActiveThread(null);
            setSelectedGroupConversationId(null);
            setThreadConversationIds(null);
          }
        }
      })
      .catch((err) => {
        console.error(err);
        if (!cancelled) setSelectionGroupChats([]);
      })
      .finally(() => {
        if (!cancelled) setLoadingSelectionGroups(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectionIdsKey, source, threadsEpoch, vaultSearch.resultsMode]);

  // Undo/redo restores server state but Panel 3 uses client-fetched lists.
  useEffect(() => {
    if (historyRevision === 0) return;
    setThreadsEpoch((n) => n + 1);
    if (vaultSearch.resultsMode) vaultSearch.refresh();
  }, [historyRevision, vaultSearch.resultsMode, vaultSearch.refresh]);

  const panelGroupChats = hasSelection ? selectionGroupChats : groupChats;

  const groupChatYears = useMemo(() => {
    const years = new Set<number>();
    for (const g of panelGroupChats) years.add(g.year);
    return [...years].sort((a, b) => b - a);
  }, [panelGroupChats]);

  useEffect(() => {
    if (
      groupChatFilterYear != null &&
      !groupChatYears.includes(groupChatFilterYear)
    ) {
      setGroupChatFilterYear(null);
    }
  }, [groupChatFilterYear, groupChatYears]);

  const { collapsedGroupChats, orderedGroupIds, collapsedById } =
    useCollapsedGroupChatList({
      groupChats: panelGroupChats,
      filterYear: groupChatFilterYear,
      query: "",
      sortBy: groupChatSortBy,
      sortOrder: groupChatSortOrder,
    });
  const selectGroupRef = useRef<(id: number) => void>(() => {});

  const {
    selectedIds: selectedGroupIds,
    setSelectedIds: setSelectedGroupIds,
    hasSelection: hasGroupSelection,
    clearSelection: clearGroupSelection,
    onSelectColumnClick: onGroupSelectColumnClick,
    onRowClick: onGroupRowClick,
  } = useListSelection<number>({
    orderedIds: orderedGroupIds,
    validIds: orderedGroupIds,
    rangeMode: "selectionSpan",
    multiThreshold: "any",
    focusedId: selectedGroupConversationId,
    rowClickMode: "openWhenEmptyElseToggle",
    checkboxEvents: "preventAndStop",
    escapeToClear: true,
    selectAllSetsAnchor: false,
    onOpen: (id) => selectGroupRef.current(id),
  });

  const selectGroupConversation = useCallback(
    (g: CollapsedGroupConversation) => {
      if (
        !hasGroupSelection &&
        hasSelection &&
        selectedGroupConversationId === g.conversationId
      ) {
        setSelectedGroupConversationId(null);
        setActiveThread(null);
        setThreadConversationIds(null);
        setFocusedSearchHit(null);
        setFocusedSearchMessageId(null);
        setContextLoadIds([]);
        syncConvUrl(null);
        return;
      }
      setFocusedSearchHit(null);
      setFocusedSearchMessageId(null);
      setContextLoadIds([]);
      setSelectedGroupConversationId(g.conversationId);
      const key = `gfull-${g.conversationIds.join("-")}`;
      openThread(g.conversationIds, key);
    },
    [
      hasGroupSelection,
      hasSelection,
      selectedGroupConversationId,
      openThread,
      syncConvUrl,
    ],
  );

  const openGroupById = useCallback(
    (id: number) => {
      const g = collapsedById.get(id);
      if (!g) return;
      selectGroupConversation(g);
    },
    [collapsedById, selectGroupConversation],
  );
  selectGroupRef.current = openGroupById;

  useEffect(() => {
    if (!hasGroupSelection) return;
    setActiveThread(null);
    setThreadConversationIds(null);
    setSelectedGroupConversationId(null);
  }, [hasGroupSelection]);

  // Multi-select is for shared-group discovery — clear an open Direct thread.
  useEffect(() => {
    if (!hasSelection) return;
    if (activeThreadRef.current !== "dm") return;
    setActiveThread(null);
    setThreadConversationIds(null);
  }, [hasSelection]);

  useEffect(() => {
    clearGroupSelection();
    setSelectedGroupConversationId(null);
  }, [selectionIdsKey, contactId, paneStorageKey, clearGroupSelection]);

  const openDirectThread = useCallback(() => {
    const dmIds = [...new Set(yearly.flatMap((y) => y.conversationIds))];
    if (dmIds.length === 0) return;
    clearGroupSelection();
    setSelectedGroupConversationId(null);
    setFocusedSearchHit(null);
    setFocusedSearchMessageId(null);
    setContextLoadIds([]);
    openThread(dmIds, "dm");
  }, [yearly, openThread, clearGroupSelection]);

  const {
    messages,
    loading: loadingMessages,
    loadingOlder,
    hasOlder,
    loadOlder,
    ensureYearLoaded,
    ensureMessageIdsLoaded,
  } = useProgressiveThreadMessages({
    conversationIds: threadConversationIds,
    sourceQuery,
    enabled: !hasGroupSelection,
    reloadToken: threadsEpoch,
  });

  const threadFind = useThreadFind({
    conversationIds: hasGroupSelection ? null : threadConversationIds,
    source,
    onJump: (match) => {
      setScrollToMessageId(match.id);
      setScrollToMessageNonce((n) => n + 1);
      const year = Number(match.timestamp.slice(0, 4));
      void ensureMessageIdsLoaded(
        [match.id],
        Number.isFinite(year) ? year : null,
      );
    },
  });

  // Search hits / deep links may target messages outside the newest page.
  useEffect(() => {
    if (scrollToMessageId == null || threadConversationIds == null) return;
    const wantIds = [
      scrollToMessageId,
      ...contextLoadIds.filter((id) => id !== scrollToMessageId),
    ];
    if (wantIds.every((id) => messages.some((m) => m.id === id))) return;
    const yearHint = focusedSearchHit?.topMatch?.timestamp
      ? Number(focusedSearchHit.topMatch.timestamp.slice(0, 4))
      : focusedSearchHit?.dateEnd
        ? Number(focusedSearchHit.dateEnd.slice(0, 4))
        : null;
    void ensureMessageIdsLoaded(
      wantIds,
      yearHint != null && Number.isFinite(yearHint) ? yearHint : null,
    );
  }, [
    scrollToMessageId,
    contextLoadIds,
    threadConversationIds,
    messages,
    focusedSearchHit,
    ensureMessageIdsLoaded,
  ]);

  const createDefaults = useMemo(() => {
    if (typeof contactSection === "object") {
      return { labels: [contactSection.label] };
    }
    return { labels: [] as string[] };
  }, [contactSection]);

  const participantForm = useParticipantContactForm({
    knownLabels: allLabels,
    createDefaults,
    setStatus: setStatusMsg,
    shouldIgnoreEscape: () =>
      ctxMenu != null || labelsPanelPos != null || toolbarLabelsPos != null,
    onSaved: (result) => {
      if (result.kind === "edit") {
        if (result.contact && result.contactId === contactId) {
          setDetail(result.contact);
        }
        setThreadsEpoch((e) => e + 1);
        router.refresh();
        return;
      }
      if (result.contact) {
        const name = result.contact.displayName ?? "contact";
        pushHistory({
          type: "createContact",
          contactId: result.contact.id,
          name,
          label: `Create contact ${name}`,
        });
        if (contactId == null) {
          setDetail(result.contact);
          selectContact(result.contact.id);
        } else {
          setThreadsEpoch((e) => e + 1);
        }
      }
      router.refresh();
    },
  });
  cancelContactFormRef.current = participantForm.cancelContactForm;

  const {
    formOpen,
    contactCreating,
    editContactId,
    contactSaving,
  } = participantForm;
  const contactEditing = editContactId != null;

  const {
    labelsPanelWrapRef,
    labelsCreatePinnedRef,
    labelsPanelPos,
    selectionDirtyRef,
    canEditLabels,
    menuLabels,
    labelChecks,
    toggleLabel,
    createAndAssignLabel,
    clearAllLabelsForSelection,
    onSelectionMenuOpenChange,
    openCtxLabels,
    closeLabelsPanel,
    scheduleCloseLabelsPanel,
    cancelCloseLabelsPanel,
    flushSelectionDirty,
  } = useBrowseLabelMembership({
    allLabels,
    contacts,
    selectedContacts,
    hasSelection,
    detail,
    setDetail,
    setThreadsEpoch,
    formOpen,
    labelOverrides,
    setLabelOverrides,
    ctxMenu,
    trashIdsForContext,
    queueStatusMessage,
    pushHistory,
  });
  const openSearchResultCtxMenu = useCallback(
    (hit: SearchConversationHit, x: number, y: number) => {
      closeLabelsPanel();
      const key = searchResultKey(hit);
      setSelectedSearchResultKeys((prev) =>
        prev.has(key) ? prev : new Set([key]),
      );
      searchResultAnchorRef.current = key;
      setSearchResultCtxMenu({ hit, x, y });
      setCtxMenu(null);
    },
    [closeLabelsPanel],
  );

  useEffect(() => {
    setSelectedIds(new Set());
    setLabelOverrides(new Map());
    selectionDirtyRef.current = false;
    cancelContactFormRef.current();
  }, [paneStorageKey, setSelectedIds, selectionDirtyRef]);

  const clearSelection = useCallback(() => {
    clearSelectionBase();
    if (selectionDirtyRef.current) {
      selectionDirtyRef.current = false;
      setLabelOverrides(new Map());
      router.refresh();
    } else {
      setLabelOverrides(new Map());
    }
  }, [clearSelectionBase, router, selectionDirtyRef]);

  useEffect(() => {
    if (!hasSelection && selectedSearchResultKeys.size === 0) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (
        ctxMenu != null ||
        searchResultCtxMenu != null ||
        labelsPanelPos != null
      ) return;
      e.preventDefault();
      clearSelection();
      setSelectedSearchResultKeys(new Set());
      searchResultAnchorRef.current = null;
      const el = document.activeElement;
      if (el instanceof HTMLElement) el.blur();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [
    hasSelection,
    selectedSearchResultKeys.size,
    clearSelection,
    ctxMenu,
    searchResultCtxMenu,
    labelsPanelPos,
  ]);

  useEffect(() => {
    if (!hasSelection) return;
    participantForm.cancelContactForm();
  }, [hasSelection, participantForm.cancelContactForm]);

  const beginContactEdit = useCallback(
    (anchor?: ContactFormAnchor | null) => {
      if (!detail || hasSelection || contactCreating) return;
      participantForm.openEditFromDraft(
        detail.id,
        seedContactEditDraft({
          ...detail,
          labels:
            labelOverrides.get(detail.id) ?? detail.labels,
        }),
        anchor ?? null,
      );
    },
    [
      detail,
      hasSelection,
      contactCreating,
      labelOverrides,
      participantForm.openEditFromDraft,
    ],
  );

  const onContactNameClick = useCallback(
    (anchorRect: DOMRect) => {
      if (saving || contactSaving || formOpen) return;
      beginContactEdit(contactFormAnchorFromRect(anchorRect));
    },
    [
      saving,
      contactSaving,
      formOpen,
      beginContactEdit,
    ],
  );

  // Finish Edit from context menu once the contact detail has loaded.
  useEffect(() => {
    const pending = pendingEditIdRef.current;
    if (pending == null || !detail || detail.id !== pending) return;
    if (hasSelection || contactCreating) return;
    pendingEditIdRef.current = null;
    participantForm.openEditFromDraft(
      detail.id,
      seedContactEditDraft({
        ...detail,
        labels:
          labelOverrides.get(detail.id) ?? detail.labels,
      }),
      null,
    );
  }, [
    detail,
    hasSelection,
    contactCreating,
    labelOverrides,
    participantForm.openEditFromDraft,
  ]);

  const openCreateContactInPlace = useCallback(
    (handle: string, anchor: ContactFormAnchor) => {
      participantForm.openCreateContactWithHandle(handle, anchor);
    },
    [participantForm.openCreateContactWithHandle],
  );

  const canDelete =
    isDeletionUiEnabled() &&
    !contactCreating &&
    (hasSelection || contactId != null);

  const executeSearchMessageTrash = useCallback(async () => {
    if (isDeletionUiBlocked()) return;
    if (selectedSearchGroups.length === 0) return;
    const directGroups = selectedSearchGroups.filter(
      (group) => group.conversationType === "individual",
    );
    const groupGroups = selectedSearchGroups.filter(
      (group) => group.conversationType === "group",
    );
    const handles = [
      ...new Set(
        directGroups.map((group) => group.chatIdentifier).filter(Boolean),
      ),
    ];
    const conversationIds = [
      ...new Set(groupGroups.flatMap((group) => group.conversationIds)),
    ];
    const subjects = [
      ...handles.map(
        (handle) =>
          directGroups.find((group) => group.chatIdentifier === handle)?.title ??
          handle,
      ),
      ...conversationIds.map(
        (id) =>
          groupGroups.find((group) => group.conversationIds.includes(id))
            ?.title ??
          "group message",
      ),
    ];
    setSearchResultCtxMenu(null);
    setSaving(true);
    try {
      const response = await fetch("/api/messages/trash", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ handles, conversationIds }),
      });
      const data = (await response.json()) as { error?: string };
      if (!response.ok) throw new Error(data.error ?? "delete failed");
      pushHistory({
        type: "trashMessageThreads",
        handles,
        conversationIds,
        subjects,
        label: trashMessageThreadsLabel(subjects),
      });
      setSelectedSearchResultKeys(new Set());
      searchResultAnchorRef.current = null;
      setFocusedSearchHit(null);
      setFocusedSearchMessageId(null);
      setContextLoadIds([]);
      setSelectedGroupConversationId(null);
      setThreadConversationIds(null);
      setActiveThread(null);
      vaultSearch.refresh();
      router.refresh();
    } catch (error) {
      console.error(error);
      queueStatusMessage(
        error instanceof Error ? error.message : "delete failed",
      );
    } finally {
      setSaving(false);
    }
  }, [
    selectedSearchGroups,
    pushHistory,
    vaultSearch,
    router,
    queueStatusMessage,
  ]);

  const deleteTargetIds = useCallback((): number[] => {
    if (hasSelection) return selectedContacts.map((c) => c.id);
    if (contactId != null) return [contactId];
    return [];
  }, [hasSelection, selectedContacts, contactId]);

  const executeTrash = useCallback(
    async (idsOverride?: number[]) => {
      if (isDeletionUiBlocked()) return;
      const ids = idsOverride ?? deleteTargetIds();
      if (ids.length === 0) return;
      setCtxMenu(null);
      setSaving(true);
      try {
        const res = await fetch("/api/contacts/trash", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ ids, mode: "contact_and_messages" }),
        });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error ?? "delete failed");

        const byId = new Map(contacts.map((c) => [c.id, c]));
        const names = ids.map((id) => {
          const c =
            byId.get(id) ??
            selectedContacts.find((x) => x.id === id) ??
            (detail?.id === id ? detail : null);
          return c?.displayName?.trim() || "contact";
        });
        pushHistory({
          type: "trashContacts",
          contactIds: ids,
          mode: "contact_and_messages",
          names,
          label: trashContactsLabel(names),
        });

        setSelectedIds(new Set());
        setSelectedGroupIds(new Set());
        setLabelOverrides(new Map());
        selectionDirtyRef.current = false;
        cancelContactFormRef.current();

        setDetail(null);
        setYearly([]);
        setGroupChats([]);
        setMessageSources([]);
        setSourceCounts({ all: 0, bySource: {} });
        setThreadConversationIds(null);
        setActiveThread(null);
        setContactId(null);
        setGroupChatFilterYear(null);
        setSelectedGroupConversationId(null);
        loadedContactIdRef.current = null;
        setThreadsLoadedFor(null);
        queueStatusMessage(
          ids.length === 1
            ? "Moved contact & messages to Trash"
            : `Moved ${ids.length} contacts to Trash`,
        );
        const params = new URLSearchParams(searchParams.toString());
        params.delete("c");
        params.delete("h");
        params.delete("y");
        params.delete("conv");
        const qs = params.toString();
        router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
        router.refresh();
      } catch (err) {
        console.error(err);
        queueStatusMessage(
          err instanceof Error ? err.message : "delete failed",
        );
        router.refresh();
      } finally {
        setSaving(false);
      }
    },
    [
      deleteTargetIds,
      contacts,
      selectedContacts,
      detail,
      queueStatusMessage,
      router,
      setSelectedIds,
      setSelectedGroupIds,
      pathname,
      searchParams,
      pushHistory,
    ],
  );

  const openContactCtxMenu = useCallback(
    (id: number, x: number, y: number) => {
      closeLabelsPanel();
      setCtxMenu({ id, x, y });
    },
    [closeLabelsPanel],
  );

  const onCtxEdit = useCallback(
    (anchorEl: HTMLElement) => {
      if (!ctxMenu || hasSelection || formOpen) return;
      const id = ctxMenu.id;
      setCtxMenu(null);
      void participantForm.openEditContact(
        id,
        contactFormAnchorFromRect(anchorEl.getBoundingClientRect()),
      );
    },
    [
      ctxMenu,
      hasSelection,
      formOpen,
      participantForm.openEditContact,
    ],
  );

  const requestTrash = useCallback(
    (idsOverride?: number[]) => {
      void executeTrash(idsOverride);
    },
    [executeTrash],
  );

  const onCtxDelete = useCallback(() => {
    if (!ctxMenu) return;
    requestTrash(trashIdsForContext(ctxMenu.id));
  }, [ctxMenu, trashIdsForContext, requestTrash]);

  useDismissible({
    open:
      ctxMenu != null || searchResultCtxMenu != null || mergeFromId != null,
    onDismiss: () => {
      setCtxMenu(null);
      setSearchResultCtxMenu(null);
      setMergeFromId(null);
      setMergeQuery("");
      setMergePos(null);
      closeLabelsPanel();
      flushSelectionDirty();
    },
    refs: [
      ctxMenuRef,
      searchResultCtxMenuRef,
      labelsPanelWrapRef,
      mergePanelRef,
    ],
    onEscape: (e) => {
      if (mergeFromId != null) {
        e.preventDefault();
        setMergeFromId(null);
        setMergeQuery("");
        setMergePos(null);
        return false;
      }
      if (labelsPanelPos != null) {
        e.preventDefault();
        closeLabelsPanel();
        return false;
      }
    },
  });

  const ctxMenuContact = useMemo(
    () => (ctxMenu ? contacts.find((c) => c.id === ctxMenu.id) : null),
    [contacts, ctxMenu],
  );
  const ctxMenuIsNameless = Boolean(
    ctxMenuContact && !(ctxMenuContact.preferredName ?? "").trim(),
  );

  const mergeTargets = useMemo(() => {
    if (mergeFromId == null) return [];
    const q = mergeQuery.trim().toLowerCase();
    return contacts
      .filter((c) => {
        if (c.id === mergeFromId) return false;
        const hasName = Boolean((c.preferredName ?? "").trim());
        if (!hasName) return false;
        if (!q) return true;
        return (
          c.displayName.toLowerCase().includes(q) ||
          (c.preferredHandle ?? "").toLowerCase().includes(q)
        );
      })
      .sort((a, b) =>
        a.sortFirst.localeCompare(b.sortFirst, undefined, {
          sensitivity: "base",
        }),
      )
      .slice(0, 40);
  }, [contacts, mergeFromId, mergeQuery]);

  const runMergeInto = useCallback(
    async (intoId: number) => {
      if (mergeFromId == null || isDeletionUiBlocked()) return;
      setSaving(true);
      try {
        const res = await fetch("/api/contacts/merge", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ fromId: mergeFromId, intoId }),
        });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error ?? "merge failed");
        setMergeFromId(null);
        setMergeQuery("");
        setMergePos(null);
        setCtxMenu(null);
        queueStatusMessage(
          `Merged into ${data.contact?.displayName ?? "contact"}`,
        );
        selectContact(intoId);
        router.refresh();
      } catch (err) {
        console.error(err);
        queueStatusMessage(
          err instanceof Error ? err.message : "merge failed",
        );
      } finally {
        setSaving(false);
      }
    },
    [
      mergeFromId,
      queueStatusMessage,
      selectContact,
      router,
    ],
  );
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Delete" && e.key !== "Backspace") return;
      if (isDeletionUiBlocked()) return;
      if (
        ctxMenu != null ||
        searchResultCtxMenu != null ||
        labelsPanelPos != null
      ) {
        return;
      }
      if (formOpen) return;
      if (selectedSearchResultKeys.size > 0) {
        e.preventDefault();
        void executeSearchMessageTrash();
        return;
      }
      if (!canDelete) return;
      const t = e.target;
      if (t instanceof HTMLElement) {
        const tag = t.tagName;
        if (
          tag === "INPUT" ||
          tag === "TEXTAREA" ||
          tag === "SELECT" ||
          t.isContentEditable
        ) {
          return;
        }
      }
      const ids = deleteTargetIds();
      if (ids.length === 0) return;
      e.preventDefault();
      requestTrash(ids);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [
    ctxMenu,
    searchResultCtxMenu,
    labelsPanelPos,
    formOpen,
    selectedSearchResultKeys.size,
    executeSearchMessageTrash,
    canDelete,
    deleteTargetIds,
    requestTrash,
  ]);
  const injectSelectedParticipants = useCallback(
    (participants: GroupParticipant[]) => {
      const next = [...participants];
      const extras = hasSelection
        ? selectedContacts
        : detail
          ? [detail]
          : [];
      for (const c of extras) {
        const handles = new Set(
          [
            c.preferredHandle,
            ...("phones" in c && Array.isArray(c.phones) ? c.phones : []),
          ]
            .filter(Boolean)
            .map((h) => String(h).trim()),
        );
        const already = next.some(
          (p) =>
            p.contactId === c.id ||
            (p.handle && handles.has(p.handle.trim())),
        );
        if (already) continue;
        next.push({
          name: c.displayName || c.preferredHandle || "Contact",
          handle: c.preferredHandle ?? "",
          handleType: c.handleType ?? null,
          contactId: c.id,
        });
      }
      next.sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
      );
      return next;
    },
    [hasSelection, selectedContacts, detail],
  );

  const directThreadMeta = useMemo(() => {
    if (activeThread !== "dm") return null;
    const attachmentCount = messages.reduce(
      (n, m) => n + m.attachments.length,
      0,
    );
    const fromYearly = yearly.length > 0;
    return {
      title:
        detail?.displayName ||
        (contactId != null
          ? (contacts.find((c) => c.id === contactId)?.displayName ?? null)
          : null) ||
        (focusedSearchHit?.conversationType === "individual"
          ? focusedSearchHit.title
          : null) ||
        "Direct",
      dateStart: fromYearly
        ? yearly.reduce(
            (min, y) => (y.dateStart < min ? y.dateStart : min),
            yearly[0]!.dateStart,
          )
        : (focusedSearchHit?.dateStart ?? null),
      dateEnd: fromYearly
        ? yearly.reduce(
            (max, y) => (y.dateEnd > max ? y.dateEnd : max),
            yearly[0]!.dateEnd,
          )
        : (focusedSearchHit?.dateEnd ?? null),
      messageCount: fromYearly
        ? yearly.reduce((n, y) => n + y.messageCount, 0)
        : (focusedSearchHit?.matchCount ?? messages.length),
      attachmentCount,
    };
  }, [
    activeThread,
    messages,
    yearly,
    detail,
    contactId,
    contacts,
    focusedSearchHit,
  ]);

  const groupThread = useMemo(() => {
    if (hasGroupSelection) return null;
    if (!activeThread?.startsWith("gfull-")) return null;
    const attachmentCount = messages.reduce(
      (n, m) => n + m.attachments.length,
      0,
    );
    const g = collapsedGroupChats.find(
      (t) => `gfull-${t.conversationIds.join("-")}` === activeThread,
    );
    if (g) {
      return {
        participants: injectSelectedParticipants([...(g.participants ?? [])]),
        dateStart: g.dateStart,
        dateEnd: g.dateEnd,
        messageCount: g.messageCount,
        attachmentCount,
        title: g.title,
        namedTitle: g.namedTitle,
      };
    }
    if (
      focusedSearchHit &&
      focusedSearchHit.conversationType === "group" &&
      `gfull-${focusedSearchHit.conversationId}` === activeThread
    ) {
      return {
        participants: [],
        dateStart: focusedSearchHit.dateStart ?? "",
        dateEnd: focusedSearchHit.dateEnd ?? "",
        messageCount: focusedSearchHit.matchCount,
        attachmentCount,
        title: focusedSearchHit.title,
        namedTitle: null as string | null,
      };
    }
    return null;
  }, [
    hasGroupSelection,
    collapsedGroupChats,
    activeThread,
    injectSelectedParticipants,
    focusedSearchHit,
    messages,
  ]);

  const selectedGroupRows = useMemo(
    () =>
      collapsedGroupChats.filter((g) => selectedGroupIds.has(g.conversationId)),
    [collapsedGroupChats, selectedGroupIds],
  );

  const onGroupParticipantClick = useCallback(
    (participant: GroupParticipant, anchorRect: DOMRect) => {
      if (saving || contactSaving) return;
      participantForm.onParticipantClick(participant, anchorRect);
    },
    [
      saving,
      contactSaving,
      participantForm.onParticipantClick,
    ],
  );

  const [vcfPreview, setVcfPreview] = useState<{
    file: File;
    preview: VcfImportPreview;
  } | null>(null);
  const [vcfCommitting, setVcfCommitting] = useState(false);

  const onImportVcf = useCallback(
    async (file: File) => {
      const body = new FormData();
      body.set("file", file);
      body.set("mode", "preview");
      try {
        const res = await fetch("/api/contacts/import-vcf", {
          method: "POST",
          body,
        });
        const data = (await res.json()) as VcfImportPreview & {
          error?: string;
        };
        if (!res.ok) throw new Error(data.error ?? "VCF preview failed");
        setVcfPreview({ file, preview: data });
      } catch (err) {
        console.error(err);
        queueStatusMessage(
          err instanceof Error ? err.message : "VCF preview failed",
        );
      }
    },
    [queueStatusMessage],
  );

  const onExportContactsCsv = useCallback(() => {
    const a = document.createElement("a");
    a.href = "/api/contacts/export-csv";
    a.download = "contacts.csv";
    a.rel = "noopener";
    document.body.appendChild(a);
    a.click();
    a.remove();
    queueStatusMessage("Downloading contacts.csv");
  }, [queueStatusMessage]);

  const onConfirmVcfImport = useCallback(
    async (mappings: VcfCategoryMapping[]) => {
      if (!vcfPreview) return;
      setVcfCommitting(true);
      const body = new FormData();
      body.set("file", vcfPreview.file);
      body.set("mode", "commit");
      body.set("mappings", JSON.stringify(mappings));
      try {
        const res = await fetch("/api/contacts/import-vcf", {
          method: "POST",
          body,
        });
        const data = (await res.json()) as {
          error?: string;
          created?: number;
          updated?: number;
          skipped?: number;
          matched?: number;
          errors?: string[];
        };
        if (!res.ok) throw new Error(data.error ?? "VCF import failed");
        const created = data.created ?? 0;
        const updated = data.updated ?? 0;
        const skipped = data.skipped ?? 0;
        const errCount = data.errors?.length ?? 0;
        const parts = [
          `Created ${created}`,
          `updated ${updated}`,
          `skipped ${skipped}`,
        ];
        if (errCount > 0) {
          parts.push(`${errCount} note${errCount === 1 ? "" : "s"}`);
        }
        queueStatusMessage(`VCF import: ${parts.join(", ")}`);
        if (errCount > 0 && data.errors) {
          console.warn("VCF import notes:", data.errors);
        }
        setVcfPreview(null);
        router.refresh();
      } catch (err) {
        console.error(err);
        queueStatusMessage(
          err instanceof Error ? err.message : "VCF import failed",
        );
      } finally {
        setVcfCommitting(false);
      }
    },
    [vcfPreview, queueStatusMessage, router],
  );

  const groupTrashTargets = useCallback(
    (forId?: number) => {
      const primaryIds =
        forId != null && !hasGroupSelection
          ? [forId]
          : hasGroupSelection
            ? [...selectedGroupIds]
            : selectedGroupConversationId != null
              ? [selectedGroupConversationId]
              : [];
      const out: number[] = [];
      for (const id of primaryIds) {
        const g = collapsedById.get(id);
        const ids = g?.conversationIds?.length ? g.conversationIds : [id];
        for (const cid of ids) {
          if (!out.includes(cid)) out.push(cid);
        }
      }
      return out;
    },
    [
      hasGroupSelection,
      selectedGroupIds,
      selectedGroupConversationId,
      collapsedById,
    ],
  );

  const canTrashGroups =
    isDeletionUiEnabled() &&
    (hasGroupSelection || selectedGroupConversationId != null);

  const browseGroupTrash = useMemo(
    () => createGroupChatTrashOptions({ variant: "browse" }),
    [],
  );

  const {
    saving: groupTrashSaving,
    moveToTrash: moveGroupsToTrash,
    confirmDialog: groupTrashConfirmDialog,
  } = useTrashActions<number>({
    endpoint: browseGroupTrash.endpoint,
    idField: browseGroupTrash.idField,
    getTargets: groupTrashTargets,
    canTrash: canTrashGroups,
    canRestoreOrDelete: false,
    status: browseGroupTrash.status,
    setStatus: (s) => {
      if (s) queueStatusMessage(s);
    },
    onRemoved: (targets) => {
      clearGroupSelection();
      setSelectedGroupConversationId(null);
      setThreadConversationIds(null);
      setActiveThread(null);
      const removed = new Set(targets);
      setGroupChats((prev) =>
        prev.filter((g) => {
          const ids =
            g.conversationIds?.length > 0
              ? g.conversationIds
              : [g.conversationId];
          return !ids.some((id) => removed.has(id));
        }),
      );
      setSelectionGroupChats((prev) =>
        prev.filter((g) => {
          const ids =
            g.conversationIds?.length > 0
              ? g.conversationIds
              : [g.conversationId];
          return !ids.some((id) => removed.has(id));
        }),
      );
    },
    onTrashed: (ids) => {
      const titles = ids.map((id) => {
        const g = collapsedById.get(id);
        return g ? groupChatToastTitle(g) : "group message";
      });
      pushHistory(browseGroupTrash.historyEntry(ids, titles));
    },
    afterTrash: () => {
      setThreadsEpoch((n) => n + 1);
      router.refresh();
    },
  });

  return (
    <>
    <Group
      id="mv-browse-main-v3"
      orientation="horizontal"
      className="h-full w-full"
      defaultLayout={mainLayout.defaultLayout}
      onLayoutChanged={mainLayout.onLayoutChanged}
    >
      <Panel
        id="tree"
        defaultSize={320}
        minSize={220}
        maxSize={560}
        groupResizeBehavior="preserve-pixel-size"
        className="relative z-40 min-h-0 overflow-visible"
      >
        <BrowsePeopleTreePane
          sectionLabel={sectionLabel}
          contactQuery={query}
          onContactQueryChange={setQuery}
          grouped={grouped}
          sortedCount={sorted.length}
          visibleCount={visibleContacts.length}
          contactId={contactId}
          contextMenuId={ctxMenu?.id ?? null}
          selectedContactIds={selectedIds}
          contactSelectAllRef={selectAllRef}
          allContactsSelected={allGroupSelected}
          onToggleSelectAllContacts={toggleSelectAllInGroup}
          onContactSelectColumnClick={onSelectColumnClick}
          onContactNamePhoneClick={onNamePhoneClick}
          onContactContextMenu={openContactCtxMenu}
          onToggleExpandContact={(id) => {
            if (contactId === id) clearContactFocus();
            else selectContact(id);
          }}
          expandedContactId={contactId}
          onNewContact={(el) =>
            openCreateContactInPlace(
              "",
              contactFormAnchorFromRect(el.getBoundingClientRect()),
            )
          }
          onImportVcf={onImportVcf}
          onExportContactsCsv={onExportContactsCsv}
          onLabels={(el) => {
            const rect = el.getBoundingClientRect();
            setToolbarLabelsPos({
              x: Math.max(8, rect.right - 256),
              y: rect.bottom + 4,
            });
          }}
          labelsDisabled={!canEditLabels}
          onEdit={(el) =>
            beginContactEdit(
              contactFormAnchorFromRect(el.getBoundingClientRect()),
            )
          }
          editDisabled={!detail || hasSelection || formOpen}
          onTrashContact={() => requestTrash()}
          deleteDisabled={!canDelete || saving || groupTrashSaving}
          contactSort={sort}
          contactSortOrder={sortOrder}
          onContactSortChange={setSort}
          yearly={yearly}
          groupItems={collapsedGroupChats}
          loadingThreads={loadingThreads}
          selectedConversationId={selectedGroupConversationId}
          selectedGroupIds={selectedGroupIds}
          onGroupSelectColumnClick={onGroupSelectColumnClick}
          onGroupRowClick={onGroupRowClick}
          onTrashMessages={() => void moveGroupsToTrash()}
          trashDisabled={!canTrashGroups || saving || groupTrashSaving}
          years={groupChatYears}
          filterYear={groupChatFilterYear}
          onFilterYearChange={setGroupChatFilterYear}
          groupSortBy={groupChatSortBy}
          groupSortOrder={groupChatSortOrder}
          onGroupSortChange={setGroupChatSort}
          searchQuery={vaultSearch.draft}
          onSearchQueryChange={vaultSearch.setDraft}
          onSearchSubmit={(q) => {
            vaultSearch.submit(q);
            const params = new URLSearchParams(searchParams.toString());
            if (q.trim()) params.set("q", q.trim());
            else params.delete("q");
            const qs = params.toString();
            router.replace(qs ? `${pathname}?${qs}` : pathname, {
              scroll: false,
            });
          }}
          searchSources={sources}
          searchLabels={allLabels}
          resultsMode={vaultSearch.resultsMode}
          searchMode={vaultSearch.mode}
          searchGroupBy={vaultSearch.groupBy}
          searchHits={sortedSearchHits}
          searchMessageHits={vaultSearch.messageHits}
          searchContactHits={sortedSearchContactHits}
          searchTotal={vaultSearch.total}
          searchLoading={vaultSearch.loading}
          searchLoadingMore={vaultSearch.loadingMore}
          onSearchLoadMore={vaultSearch.hasMore ? vaultSearch.loadMore : undefined}
          searchHighlightTerms={vaultSearch.highlightTerms}
          searchContactIds={searchContactIds}
          allSearchContactsSelected={allSearchContactsSelected}
          onToggleSelectAllSearchContacts={toggleSelectAllSearchContacts}
          onToggleSearchContact={toggleSearchContact}
          onOpenSearchContact={selectContact}
          selectedSearchResultKeys={selectedSearchResultKeys}
          allSearchResultsSelected={allSearchResultsSelected}
          onToggleSelectAllSearchResults={toggleSelectAllSearchResults}
          onToggleSearchResult={toggleSearchResult}
          selectedSearchMessageId={focusedSearchMessageId}
          onSelectSearchMessageHit={(hit: SearchMessageHit) => {
            clearGroupSelection();
            setFocusedSearchMessageId(hit.messageId);
            setFocusedSearchHit({
              conversationId: hit.conversationId,
              conversationType: hit.conversationType,
              contactId: hit.contactId,
              title: hit.title,
              chatIdentifier: hit.chatIdentifier,
              matchCount: 1,
              dateStart: hit.timestamp,
              dateEnd: hit.timestamp,
              topMatch: {
                id: hit.messageId,
                timestamp: hit.timestamp,
                snippet: hit.snippet,
                isFromMe: hit.isFromMe,
                sender: hit.sender,
              },
            });
            setScrollToMessageId(hit.messageId);
            setScrollToMessageNonce((n) => n + 1);
            setContextLoadIds([
              ...hit.contextBefore.map((m) => m.id),
              hit.messageId,
              ...hit.contextAfter.map((m) => m.id),
            ]);
            if (
              hit.conversationType === "individual" &&
              hit.contactId != null &&
              hit.contactId !== contactId
            ) {
              selectContact(hit.contactId);
              pendingConvIdRef.current = hit.conversationId;
            }
            setSelectedGroupConversationId(hit.conversationId);
            openThread(
              [hit.conversationId],
              hit.conversationType === "group"
                ? `gfull-${hit.conversationId}`
                : "dm",
            );
            const parsedQuery = parseSearchQuery(vaultSearch.committed);
            const findSeed = [
              ...parsedQuery.terms,
              ...parsedQuery.phrases.map((p) => `"${p}"`),
            ].join(" ");
            if (findSeed) {
              threadFind.openWith(findSeed, hit.messageId);
            } else if (threadFind.open) {
              threadFind.close();
            }
          }}
          onSelectSearchHit={(hit: SearchConversationHit, mods) => {
            if (
              mods?.shiftKey ||
              mods?.altKey ||
              mods?.metaKey ||
              mods?.ctrlKey
            ) {
              toggleSearchResult(hit, {
                shiftKey: Boolean(mods.shiftKey),
                altKey: Boolean(mods.altKey),
                metaKey: Boolean(mods.metaKey),
                ctrlKey: Boolean(mods.ctrlKey),
              });
              return;
            }
            clearGroupSelection();
            setFocusedSearchMessageId(hit.topMatch?.id ?? null);
            setFocusedSearchHit(hit);
            setScrollToMessageId(hit.topMatch?.id ?? null);
            setScrollToMessageNonce((n) => n + 1);
            const parsedQuery = parseSearchQuery(vaultSearch.committed);
            if (parsedQuery.context > 0 && hit.topMatch?.id != null) {
              void fetch(
                `/api/search?around=${hit.topMatch.id}&context=${parsedQuery.context}`,
              )
                .then(async (res) => {
                  if (!res.ok) return;
                  const json = (await res.json()) as { ids?: number[] };
                  if (Array.isArray(json.ids)) setContextLoadIds(json.ids);
                })
                .catch(() => {});
            } else {
              setContextLoadIds(
                hit.topMatch?.id != null ? [hit.topMatch.id] : [],
              );
            }
            // Focus the contact so the 1-1 conversation inspector can load
            // yearly stats; groups don't need a contact focus. pendingConv
            // keeps the threads effect from clearing the open Direct thread
            // when the contact also has groups.
            if (
              hit.conversationType === "individual" &&
              hit.contactId != null &&
              hit.contactId !== contactId
            ) {
              selectContact(hit.contactId);
              pendingConvIdRef.current = hit.conversationId;
            }
            // After selectContact (which clears group selection id).
            setSelectedGroupConversationId(hit.conversationId);
            openThread(
              [hit.conversationId],
              hit.conversationType === "group"
                ? `gfull-${hit.conversationId}`
                : "dm",
            );
            // Hand the text terms off to the in-thread find bar so the user
            // can step through every match, not just the top one.
            const findSeed = [
              ...parsedQuery.terms,
              ...parsedQuery.phrases.map((p) => `"${p}"`),
            ].join(" ");
            if (findSeed) {
              threadFind.openWith(findSeed, hit.topMatch?.id ?? null);
            } else if (threadFind.open) {
              threadFind.close();
            }
          }}
          onSearchContactContextMenu={openContactCtxMenu}
          onSearchResultContextMenu={openSearchResultCtxMenu}
          onDeleteSearchResults={() => void executeSearchMessageTrash()}
          onDirectClick={openDirectThread}
          directActive={activeThread === "dm"}
          emptyGroupsLabel={
            hasSelection
              ? loadingSelectionGroups
                ? "Loading…"
                : selectedIds.size > 1
                  ? "No shared group messages"
                  : "No group messages"
              : contactId && loadingThreads
                ? "Loading…"
                : "No group messages"
          }
        />
      </Panel>

      <PaneSeparator orientation="vertical" />

      <Panel id="thread" minSize="30%" className="min-h-0 min-w-0">
        <BrowseThreadColumn
          paneStorageKey={paneStorageKey}
          detail={detail}
          groupThread={groupThread}
          statusMsg={statusMsg}
          contactId={contactId}
          activeThread={activeThread}
          sources={sources}
          messageSources={messageSources}
          sourceCounts={sourceCounts}
          source={source}
          onSourceChange={setSource}
          yearly={yearly}
          messages={messages}
          loadingMessages={loadingMessages}
          threadsLoadedFor={threadsLoadedFor}
          hasConversationChoices={
            !hasSelection &&
            !hasGroupSelection &&
            (yearly.some((y) => y.conversationIds.length > 0) ||
              groupChats.length > 0)
          }
          highlightTerms={
            threadFind.open ? threadFind.terms : vaultSearch.highlightTerms
          }
          scrollToMessageId={scrollToMessageId}
          scrollToMessageNonce={scrollToMessageNonce}
          findOpen={threadFind.open}
          onContactNameClick={onContactNameClick}
          onGroupParticipantClick={onGroupParticipantClick}
          readerOnly
          hasSelection={hasSelection}
          hasGroupSelection={hasGroupSelection}
          hasOlder={hasOlder}
          loadingOlder={loadingOlder}
          onLoadOlder={loadOlder}
          onEnsureYear={(year) => {
            void ensureYearLoaded(year);
          }}
          searchConversationSections={searchConversationSections}
          findBar={<ThreadFindBar find={threadFind} />}
          onOpenFind={threadFind.openBar}
        />
      </Panel>

      <PaneSeparator orientation="vertical" />

      <Panel
        id="inspector"
        panelRef={inspectorPanelRef}
        defaultSize={280}
        minSize={200}
        maxSize={420}
        collapsible
        collapsedSize={0}
        onResize={persistInspectorCollapsed}
        className="min-h-0 overflow-hidden"
      >
        <BrowseDetailsInspector
          hasContactSelection={hasSelection}
          hasGroupSelection={hasGroupSelection}
          selectedContacts={selectedContacts}
          selectedGroupRows={selectedGroupRows}
          focusedContact={
            contactId != null
              ? detail?.id === contactId
                ? detail
                : (contacts.find((c) => c.id === contactId) ?? null)
              : null
          }
          detail={detail}
          yearly={yearly}
          groupChats={groupChats}
          activeThread={activeThread}
          groupThreadMeta={groupThread}
          directThreadMeta={directThreadMeta}
          openConversation={
            selectedGroupConversationId != null
              ? (collapsedById.get(selectedGroupConversationId) ??
                (focusedSearchHit &&
                focusedSearchHit.conversationId === selectedGroupConversationId
                  ? ({
                      conversationId: focusedSearchHit.conversationId,
                      conversationIds: [focusedSearchHit.conversationId],
                      title: focusedSearchHit.title,
                      titleFull: focusedSearchHit.title,
                      namedTitle: null,
                      participantCount: 0,
                      participantNames: [],
                      participantHandles: [],
                      participants: [],
                      messageCount: focusedSearchHit.matchCount,
                      dateStart: focusedSearchHit.dateStart ?? "",
                      dateEnd: focusedSearchHit.dateEnd ?? "",
                      newestYear: focusedSearchHit.dateEnd
                        ? Number(focusedSearchHit.dateEnd.slice(0, 4)) || 0
                        : 0,
                    } satisfies CollapsedGroupConversation)
                  : null))
              : null
          }
          onClearContactSelection={clearSelection}
          onClearGroupSelection={clearGroupSelection}
          onEditContact={
            detail && !hasSelection && !formOpen
              ? () =>
                  beginContactEdit(
                    contactFormAnchorFromRect(
                      new DOMRect(window.innerWidth - 320, 80, 0, 0),
                    ),
                  )
              : undefined
          }
          onParticipantClick={onGroupParticipantClick}
        />
      </Panel>
    </Group>
    {ctxMenu && (
      <BrowseContactCtxMenu
        menuRef={ctxMenuRef}
        ctxMenu={ctxMenu}
        saving={saving}
        groupTrashSaving={groupTrashSaving}
        hasSelection={hasSelection}
        deleteCount={trashIdsForContext(ctxMenu.id).length}
        contactCreating={contactCreating}
        contactEditing={contactEditing}
        isNameless={ctxMenuIsNameless}
        onMouseEnterItem={scheduleCloseLabelsPanel}
        onNewContact={(el) => {
          setCtxMenu(null);
          openCreateContactInPlace(
            "",
            contactFormAnchorFromRect(el.getBoundingClientRect()),
          );
        }}
        onEdit={onCtxEdit}
        onMergeInto={() => {
          setMergeFromId(ctxMenu.id);
          setMergePos({ x: ctxMenu.x, y: ctxMenu.y });
          setMergeQuery("");
          setCtxMenu(null);
        }}
        onLabelsEnter={openCtxLabels}
        onLabelsLeave={scheduleCloseLabelsPanel}
        onDelete={onCtxDelete}
      />
    )}
    {searchResultCtxMenu && (
      <SearchMessageCtxMenu
        menuRef={searchResultCtxMenuRef}
        x={searchResultCtxMenu.x}
        y={searchResultCtxMenu.y}
        count={selectedSearchResultKeys.size}
        saving={saving}
        onDelete={() => void executeSearchMessageTrash()}
      />
    )}
    {mergeFromId != null && mergePos && (
      <BrowseMergeIntoPanel
        panelRef={mergePanelRef}
        x={mergePos.x}
        y={mergePos.y}
        query={mergeQuery}
        onQueryChange={setMergeQuery}
        targets={mergeTargets}
        saving={saving}
        onSelect={(id) => void runMergeInto(id)}
      />
    )}
    {labelsPanelPos && (
      <div
        ref={labelsPanelWrapRef}
        onMouseEnter={cancelCloseLabelsPanel}
        onMouseLeave={scheduleCloseLabelsPanel}
      >
        <LabelsMenu
          fixedPosition={labelsPanelPos}
          allLabels={menuLabels}
          checks={labelChecks}
          disabled={formOpen}
          onToggle={toggleLabel}
          onCreate={createAndAssignLabel}
          onClearAll={() => void clearAllLabelsForSelection()}
          onModeChange={(mode) => {
            labelsCreatePinnedRef.current = mode === "create";
            if (mode === "create") cancelCloseLabelsPanel();
          }}
          onOpenChange={(open) => {
            if (!open) closeLabelsPanel();
            else onSelectionMenuOpenChange(true);
          }}
        />
      </div>
    )}
    {toolbarLabelsPos && (
      <LabelsMenu
        fixedPosition={toolbarLabelsPos}
        allLabels={menuLabels}
        checks={labelChecks}
        disabled={!canEditLabels}
        onToggle={toggleLabel}
        onCreate={createAndAssignLabel}
        onClearAll={() => void clearAllLabelsForSelection()}
        onOpenChange={(open) => {
          onSelectionMenuOpenChange(open);
          if (!open) setToolbarLabelsPos(null);
        }}
      />
    )}
    <ParticipantContactFormOverlay
      titleId="mv-contact-form-title"
      handlesView={detail?.handles ?? []}
      form={participantForm}
    />
    {vcfPreview && (
      <VcfImportPreviewDialog
        fileName={vcfPreview.file.name}
        preview={vcfPreview.preview}
        busy={vcfCommitting}
        onDismiss={() => {
          if (!vcfCommitting) setVcfPreview(null);
        }}
        onConfirm={(mappings) => void onConfirmVcfImport(mappings)}
      />
    )}
    {groupTrashConfirmDialog}
    </>
  );
}
