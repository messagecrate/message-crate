"use client";

import type { GroupYearRow, YearThread } from "@/lib/types";
import {
  GROUP_CHAT_SORT_ALLOWED,
  GROUP_CHAT_SORT_KEY,
  GROUP_CHAT_SORT_ORDER_KEY,
  groupYearRowsToThreads,
  newestYearForConversation,
  SORT_ORDER_ALLOWED,
  type CollapsedGroupConversation,
} from "@/lib/groupChatList";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
} from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { Group, Panel, useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { BrowseDetailsInspector } from "./BrowseDetailsInspector";
import { BrowseGroupChatsPane } from "./BrowseGroupChatsPane";
import { BrowseThreadColumn } from "./BrowseThreadColumn";
import {
  createGroupChatTrashOptions,
  groupChatToastTitle,
} from "./groupChatTrash";
import { PaneSeparator } from "./PaneSeparator";
import { ParticipantContactFormOverlay } from "./ParticipantContactFormOverlay";
import { usePanelLayoutStorage } from "./panelLayoutStorage";
import {
  type BrowseGroupChatSortBy,
  type SortOrder,
} from "./SortByMenu";
import { isDeletionUiEnabled } from "@/lib/v1Capabilities";
import { useHistory } from "./history";
import { useCollapsedGroupChatList } from "./useCollapsedGroupChatList";
import { useListSelection } from "./useListSelection";
import { useParticipantContactForm } from "./useParticipantContactForm";
import { usePersistedEnum } from "./usePersistedEnum";
import { useSourceFilter } from "./SourceFilter";
import { useThreadMessages } from "./useThreadMessages";
import { useTrashActions } from "./useTrashActions";
import { useVaultSearch } from "./useVaultSearch";
import { ThreadFindBar } from "./ThreadFindBar";
import { useThreadFind } from "./useThreadFind";
import type { SearchConversationHit } from "@/lib/search";
import { parseSearchQuery } from "@/lib/searchQuery";

const INSPECTOR_PANEL_COLLAPSED_KEY = "mv-group-messages-inspector-collapsed";

export function GroupMessagesShell({
  groupChats: initialGroupChats,
  initialConversationId,
  initialYear,
}: {
  groupChats: GroupYearRow[];
  initialConversationId: number | null;
  initialYear: number | null;
}) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const { push: pushHistory } = useHistory();
  const { sources, source, setSource, sourceQuery } = useSourceFilter();

  const [groupChats, setGroupChats] = useState(initialGroupChats);
  const [conversationId, setConversationId] = useState<number | null>(
    initialConversationId,
  );
  const [focusYear, setFocusYear] = useState<number | null>(initialYear);
  const [status, setStatus] = useState<string | null>(null);
  const initialVaultQuery = searchParams.get("q") ?? "";
  const vaultSearch = useVaultSearch(initialVaultQuery);
  const [scrollToMessageId, setScrollToMessageId] = useState<number | null>(
    null,
  );
  const [scrollToMessageNonce, setScrollToMessageNonce] = useState(0);
  const [filterYear, setFilterYear] = useState<number | null>(null);
  const [fullMessageIds, setFullMessageIds] = useState<number[] | null>(
    initialConversationId != null ? [initialConversationId] : null,
  );
  const [activeThread, setActiveThread] = useState<string | null>(
    initialConversationId != null ? `gfull-${initialConversationId}` : null,
  );

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

  const storage = usePanelLayoutStorage();
  const mainLayout = useDefaultLayout({
    id: "mv-group-messages-main-v2",
    panelIds: ["groups", "thread", "inspector"],
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

  const pendingScrollYearRef = useRef<number | null>(initialYear);

  useEffect(() => {
    setGroupChats(initialGroupChats);
    if (
      conversationId != null &&
      !initialGroupChats.some((g) => g.id === conversationId)
    ) {
      setConversationId(null);
      setFocusYear(null);
      setActiveThread(null);
      setFullMessageIds(null);
    }
  }, [initialGroupChats, conversationId]);

  const years = useMemo(() => {
    const set = new Set<number>();
    for (const g of groupChats) set.add(g.year);
    return [...set].sort((a, b) => b - a);
  }, [groupChats]);

  useEffect(() => {
    if (filterYear == null) return;
    if (!years.includes(filterYear)) setFilterYear(null);
  }, [years, filterYear]);

  const panelThreads = useMemo(
    () => groupYearRowsToThreads(groupChats),
    [groupChats],
  );

  const { collapsedGroupChats, orderedGroupIds, collapsedById } =
    useCollapsedGroupChatList({
      groupChats: panelThreads,
      filterYear,
      query: "",
      sortBy: groupChatSortBy,
      sortOrder: groupChatSortOrder,
    });

  const selectGroupRef = useRef<(id: number) => void>(() => {});

  const {
    selectedIds,
    setSelectedIds,
    clearSelection: clearGroupSelection,
    hasSelection: hasGroupSelection,
    allSelected,
    selectAllRef,
    toggleSelectAll,
    onSelectColumnClick,
    onRowClick: onGroupRowClick,
  } = useListSelection<number>({
    orderedIds: orderedGroupIds,
    validIds: orderedGroupIds,
    rangeMode: "selectionSpan",
    multiThreshold: "any",
    focusedId: conversationId,
    rowClickMode: "openWhenEmptyElseToggle",
    checkboxEvents: "preventAndStop",
    escapeToClear: true,
    selectAllSetsAnchor: false,
    onOpen: (id) => selectGroupRef.current(id),
  });

  const { messages, loading: loadingMessages } = useThreadMessages({
    conversationIds: fullMessageIds,
    sourceQuery,
    fullConversation: true,
    enabled: !hasGroupSelection,
  });

  // Group threads load in full, so jumping needs no extra page loads.
  const threadFind = useThreadFind({
    conversationIds: hasGroupSelection ? null : fullMessageIds,
    source,
    onJump: (match) => {
      setScrollToMessageId(match.id);
      setScrollToMessageNonce((n) => n + 1);
    },
  });

  const syncUrl = useCallback(
    (id: number | null, year: number | null) => {
      const params = new URLSearchParams(searchParams.toString());
      if (id == null) {
        params.delete("g");
        params.delete("y");
      } else {
        params.set("g", String(id));
        if (year != null) params.set("y", String(year));
        else params.delete("y");
      }
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams],
  );

  const selectGroupConversation = useCallback(
    (g: CollapsedGroupConversation) => {
      if (!hasGroupSelection && conversationId === g.conversationId) {
        setConversationId(null);
        setFocusYear(null);
        syncUrl(null, null);
        return;
      }
      const year = filterYear ?? g.newestYear;
      setConversationId(g.conversationId);
      setFocusYear(year);
      pendingScrollYearRef.current = year;
      syncUrl(g.conversationId, year);
    },
    [hasGroupSelection, conversationId, filterYear, syncUrl],
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

  // Hydrate initial / corrected focus year from URL.
  useEffect(() => {
    if (conversationId == null) return;
    if (!groupChats.some((g) => g.id === conversationId)) return;

    const yearOk =
      focusYear != null &&
      groupChats.some((g) => g.id === conversationId && g.year === focusYear);
    if (yearOk) return;

    const nextYear = newestYearForConversation(groupChats, conversationId);
    if (nextYear == null) return;
    setFocusYear(nextYear);
    pendingScrollYearRef.current = nextYear;
    syncUrl(conversationId, nextYear);
  }, [groupChats, conversationId, focusYear, syncUrl]);

  // Single message-load path: derive ids from the collapsed row (incl. URL hydrate).
  useEffect(() => {
    if (conversationId == null || hasGroupSelection) {
      setActiveThread(null);
      setFullMessageIds(null);
      return;
    }
    const g = collapsedById.get(conversationId);
    const ids = g?.conversationIds?.length
      ? g.conversationIds
      : [conversationId];
    setActiveThread(`gfull-${ids.join("-")}`);
    setFullMessageIds(ids);
  }, [conversationId, hasGroupSelection, collapsedById]);

  const clearFocusAfterRemoval = useCallback(
    (removedIds: number[]) => {
      const removed = new Set(removedIds);
      setGroupChats((prev) => prev.filter((g) => !removed.has(g.id)));
      setSelectedIds((prev) => {
        const next = new Set(prev);
        for (const id of removed) next.delete(id);
        return next;
      });
      if (conversationId != null && removed.has(conversationId)) {
        setConversationId(null);
        setFocusYear(null);
        setActiveThread(null);
        setFullMessageIds(null);
        syncUrl(null, null);
      }
    },
    [conversationId, setSelectedIds, syncUrl],
  );

  const actionTargets = useMemo(() => {
    if (hasGroupSelection) return [...selectedIds];
    if (conversationId != null) return [conversationId];
    return [];
  }, [hasGroupSelection, selectedIds, conversationId]);

  const getTrashTargets = useCallback(
    (forId?: number) => {
      const raw =
        forId != null && !hasGroupSelection ? [forId] : actionTargets;
      return [...new Set(raw)];
    },
    [actionTargets, hasGroupSelection],
  );

  const groupTrash = useMemo(() => createGroupChatTrashOptions(), []);

  const {
    saving,
    moveToTrash,
    confirmDialog,
  } = useTrashActions<number>({
    endpoint: groupTrash.endpoint,
    idField: groupTrash.idField,
    getTargets: getTrashTargets,
    canTrash: true,
    canRestoreOrDelete: false,
    confirmPermanent: groupTrash.confirmPermanent,
    status: groupTrash.status,
    setStatus,
    onRemoved: clearFocusAfterRemoval,
    afterTrash: () => {
      router.refresh();
    },
    onTrashed: (ids) => {
      const titles = ids.map((id) => {
        const g = collapsedById.get(id);
        return g ? groupChatToastTitle(g) : "group message";
      });
      pushHistory(groupTrash.historyEntry(ids, titles));
    },
  });

  const canTrashGroups = isDeletionUiEnabled() && actionTargets.length > 0;

  const participantForm = useParticipantContactForm({ setStatus });

  const selectedGroup = useMemo(
    () =>
      conversationId != null
        ? (collapsedById.get(conversationId) ?? null)
        : null,
    [collapsedById, conversationId],
  );

  const groupThread = useMemo(() => {
    if (hasGroupSelection || !selectedGroup || !activeThread?.startsWith("gfull-")) {
      return null;
    }
    return {
      participants: [...(selectedGroup.participants ?? [])],
      dateStart: selectedGroup.dateStart,
      dateEnd: selectedGroup.dateEnd,
      messageCount: selectedGroup.messageCount,
    };
  }, [hasGroupSelection, selectedGroup, activeThread]);

  const groupThreadMeta = useMemo(() => {
    if (!groupThread || !selectedGroup) return null;
    return {
      ...groupThread,
      title: selectedGroup.title,
      namedTitle: selectedGroup.namedTitle,
      attachmentCount: messages.reduce((n, m) => n + m.attachments.length, 0),
    };
  }, [groupThread, selectedGroup, messages]);

  const yearly: YearThread[] = useMemo(() => {
    if (conversationId == null) return [];
    const ids = new Set(
      selectedGroup?.conversationIds?.length
        ? selectedGroup.conversationIds
        : [conversationId],
    );
    return groupChats
      .filter((g) => ids.has(g.id))
      .map((g) => ({
        year: g.year,
        messageCount: g.messageCount,
        attachmentCount: 0,
        dateStart: g.dateStart,
        dateEnd: g.dateEnd,
        conversationIds: [g.id],
      }))
      .sort((a, b) => a.year - b.year);
  }, [groupChats, conversationId, selectedGroup]);

  const messageSources = useMemo(() => {
    const set = new Set<string>();
    for (const m of messages) {
      if (m.source) set.add(m.source);
    }
    return [...set];
  }, [messages]);

  const sourceCounts = useMemo(() => {
    const bySource: Record<string, number> = {};
    for (const m of messages) {
      if (!m.source) continue;
      bySource[m.source] = (bySource[m.source] ?? 0) + 1;
    }
    return { all: messages.length, bySource };
  }, [messages]);

  // Scroll to focus year after messages load (from URL `y`).
  useEffect(() => {
    const year = pendingScrollYearRef.current;
    if (year == null || loadingMessages || messages.length === 0) return;
    const el = document.querySelector(`#year-${year}`);
    if (el) {
      requestAnimationFrame(() => {
        el.scrollIntoView({ block: "start" });
      });
    }
    pendingScrollYearRef.current = null;
  }, [loadingMessages, messages]);

  const onGroupRowClickWrapped = useCallback(
    (
      id: number,
      e: MouseEvent | { shiftKey: boolean; metaKey?: boolean; ctrlKey?: boolean },
    ) => {
      onGroupRowClick(id, e);
    },
    [onGroupRowClick],
  );

  const selectedGroupRows = useMemo(
    () => collapsedGroupChats.filter((g) => selectedIds.has(g.conversationId)),
    [collapsedGroupChats, selectedIds],
  );

  return (
    <>
      <Group
        id="mv-group-messages-main-v2"
        orientation="horizontal"
        className="h-full w-full"
        defaultLayout={mainLayout.defaultLayout}
        onLayoutChanged={mainLayout.onLayoutChanged}
      >
        <Panel
          id="groups"
          defaultSize={320}
          minSize={220}
          maxSize={560}
          groupResizeBehavior="preserve-pixel-size"
          className="relative z-40 min-h-0 overflow-visible"
        >
          <BrowseGroupChatsPane
            items={collapsedGroupChats}
            selectedConversationId={conversationId}
            selectedIds={selectedIds}
            selectAllRef={selectAllRef}
            allSelected={allSelected}
            onToggleSelectAll={toggleSelectAll}
            onSelectColumnClick={onSelectColumnClick}
            onRowClick={onGroupRowClickWrapped}
            onTrashMessages={() => void moveToTrash()}
            trashDisabled={
              !canTrashGroups || saving || participantForm.contactSaving
            }
            years={years}
            filterYear={filterYear}
            onFilterYearChange={setFilterYear}
            sortBy={groupChatSortBy}
            sortOrder={groupChatSortOrder}
            onSortChange={setGroupChatSort}
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
            searchLabels={[]}
            resultsMode={vaultSearch.resultsMode}
            searchGroupBy={vaultSearch.groupBy}
            searchHits={vaultSearch.hits}
            searchMessageHits={vaultSearch.messageHits}
            searchTotal={vaultSearch.total}
            searchLoading={vaultSearch.loading}
            searchLoadingMore={vaultSearch.loadingMore}
            onSearchLoadMore={
              vaultSearch.hasMore ? vaultSearch.loadMore : undefined
            }
            searchHighlightTerms={vaultSearch.highlightTerms}
            selectedSearchMessageId={scrollToMessageId}
            onSelectSearchMessageHit={(hit) => {
              setSelectedIds(new Set());
              setConversationId(hit.conversationId);
              setScrollToMessageId(hit.messageId);
              setScrollToMessageNonce((n) => n + 1);
              setFullMessageIds([hit.conversationId]);
              setActiveThread(`gfull-${hit.conversationId}`);
              const year = Number(hit.timestamp.slice(0, 4));
              if (Number.isFinite(year)) {
                setFocusYear(year);
                pendingScrollYearRef.current = year;
              }
              syncUrl(hit.conversationId, Number.isFinite(year) ? year : null);
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
            onSelectSearchHit={(hit: SearchConversationHit) => {
              setSelectedIds(new Set());
              setConversationId(hit.conversationId);
              setScrollToMessageId(hit.topMatch?.id ?? null);
              setScrollToMessageNonce((n) => n + 1);
              setFullMessageIds([hit.conversationId]);
              setActiveThread(`gfull-${hit.conversationId}`);
              const year = hit.dateEnd
                ? Number(hit.dateEnd.slice(0, 4))
                : null;
              if (year != null && Number.isFinite(year)) {
                setFocusYear(year);
                pendingScrollYearRef.current = year;
              }
              syncUrl(hit.conversationId, year);
              // Hand the text terms off to the in-thread find bar so the user
              // can step through every match, not just the top one.
              const parsedQuery = parseSearchQuery(vaultSearch.committed);
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
            emptyLabel="No group messages"
          />
        </Panel>

        <PaneSeparator orientation="vertical" />

        <Panel id="thread" minSize="30%" className="min-h-0 min-w-0">
          <BrowseThreadColumn
            paneStorageKey="group-messages"
            detail={null}
            groupThread={groupThread}
            statusMsg={status}
            contactId={null}
            activeThread={activeThread}
            sources={sources}
            messageSources={messageSources}
            sourceCounts={sourceCounts}
            source={source}
            onSourceChange={setSource}
            yearly={yearly}
            messages={messages}
            loadingMessages={loadingMessages}
            threadsLoadedFor={null}
            threadsReadyOverride
            hasConversationChoices={false}
            highlightTerms={
              threadFind.open ? threadFind.terms : vaultSearch.highlightTerms
            }
            scrollToMessageId={scrollToMessageId}
            scrollToMessageNonce={scrollToMessageNonce}
            findOpen={threadFind.open}
            onGroupParticipantClick={participantForm.onParticipantClick}
            readerOnly
            hasSelection={false}
            hasGroupSelection={hasGroupSelection}
            emptySelectionGuidance="Selection details are shown in the inspector on the right."
            emptyIdleGuidance="Select a group message to read the conversation."
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
            hasContactSelection={false}
            hasGroupSelection={hasGroupSelection}
            selectedContacts={[]}
            selectedGroupRows={selectedGroupRows}
            focusedContact={null}
            detail={null}
            yearly={[]}
            conversationYearly={yearly}
            groupChats={[]}
            activeThread={activeThread}
            groupThreadMeta={groupThreadMeta}
            openConversation={
              !hasGroupSelection && conversationId != null
                ? selectedGroup
                : null
            }
            onClearContactSelection={() => {}}
            onClearGroupSelection={clearGroupSelection}
            onParticipantClick={participantForm.onParticipantClick}
            emptyGuidance="Select a group message to see conversation details."
          />
        </Panel>
      </Group>

      {confirmDialog}
      <ParticipantContactFormOverlay
        form={participantForm}
        titleId="mv-group-messages-contact-form-title"
      />
    </>
  );
}
