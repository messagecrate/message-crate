"use client";

import type {
  ContactDetail,
  GroupParticipant,
  MessageRow,
  YearThread,
} from "@/lib/types";
import type { ConversationType } from "@/lib/searchQuery";
import { formatSourceLabel } from "@/lib/sourceLabels";
import {
  memo,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  GroupParticipantChip,
} from "./GroupParticipantChip";
import { MessageList } from "./MessageList";
import { ChevronDownIcon, PaperclipIcon } from "./icons";
import { useDateTimeFormat } from "./useDateTimeFormat";

const HEADER_EXPANDED_KEY = "mv-browse-thread-header-expanded";
const NEAR_BOTTOM_PX = 120;

function yearFromTimestamp(ts: string): number | null {
  const y = Number(ts.slice(0, 4));
  return Number.isFinite(y) ? y : null;
}

function prefersReducedMotion(): boolean {
  if (typeof window === "undefined") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type BrowseGroupThreadMeta = {
  participants: GroupParticipant[];
  dateStart: string;
  dateEnd: string;
  messageCount: number;
};

export type SearchConversationSection = {
  conversationIds: number[];
  title: string;
  conversationType: ConversationType;
};

/** Isolated from scroll chrome so year-highlight updates do not rebuild bubbles. */
const ThreadMessageSections = memo(function ThreadMessageSections({
  messagesByYear,
  highlightTerms,
  hasOlder,
  loadingOlder,
}: {
  messagesByYear: Array<{ year: number; messages: MessageRow[] }>;
  highlightTerms: string[];
  hasOlder: boolean;
  loadingOlder: boolean;
}) {
  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-1">
      {(loadingOlder || hasOlder) && (
        <div className="py-2 text-center text-[12px] text-muted">
          {loadingOlder
            ? "Loading earlier messages…"
            : hasOlder
              ? "Scroll up for earlier messages"
              : null}
        </div>
      )}
      {messagesByYear.map((section) => (
        <div
          key={section.year}
          id={`year-${section.year}`}
          className="scroll-mt-3"
        >
          <div className="sticky top-0 z-10 -mx-1 mb-1.5 bg-bg/95 px-1 py-1 backdrop-blur-sm">
            <div className="text-[12px] font-semibold tracking-wide text-muted tabular-nums">
              {section.year || "Unknown"}
            </div>
          </div>
          <div className="flex flex-col">
            <MessageList
              messages={section.messages}
              highlightTerms={highlightTerms}
            />
          </div>
        </div>
      ))}
    </div>
  );
});

const SearchConversationMessageSections = memo(
  function SearchConversationMessageSections({
    sections,
    messages,
    highlightTerms,
  }: {
    sections: SearchConversationSection[];
    messages: MessageRow[];
    highlightTerms: string[];
  }) {
    return (
      <div className="mx-auto flex max-w-2xl flex-col gap-5">
        {sections.map((section) => {
          const sectionMessages = messages
            .filter(
              (message) =>
                message.conversationId != null &&
                section.conversationIds.includes(message.conversationId),
            )
            .sort(
              (a, b) =>
                a.timestamp.localeCompare(b.timestamp) || a.id - b.id,
            );
          return (
            <section key={section.conversationIds.join(",")}>
              <div className="sticky top-0 z-10 mb-2 border-b border-border bg-bg/95 py-2 backdrop-blur-sm">
                <div className="truncate text-[13px] font-semibold text-text">
                  {section.title}
                </div>
                <div className="text-[11px] text-muted">
                  {section.conversationType === "group" ? "Group" : "Direct"} ·{" "}
                  {sectionMessages.length.toLocaleString()} messages
                </div>
              </div>
              <MessageList
                messages={sectionMessages}
                highlightTerms={highlightTerms}
              />
            </section>
          );
        })}
      </div>
    );
  },
);

export function BrowseThreadPane({
  detail,
  sources,
  messageSources,
  sourceCounts,
  source,
  onSourceChange,
  yearly,
  messages,
  loadingMessages,
  threadsReady = false,
  activeThread,
  groupThread,
  onParticipantClick,
  hasConversationChoices = false,
  conversationsPanelCollapsed = false,
  highlightTerms = [],
  scrollToMessageId = null,
  scrollToMessageNonce = 0,
  findOpen = false,
  hasOlder = false,
  loadingOlder = false,
  onLoadOlder,
  onEnsureYear,
  searchConversationSections = [],
}: {
  detail: ContactDetail | null;
  sources: string[];
  messageSources: string[];
  sourceCounts: { all: number; bySource: Record<string, number> };
  source: string | null;
  onSourceChange: (id: string | null) => void;
  yearly: YearThread[];
  messages: MessageRow[];
  loadingMessages: boolean;
  /** True once the current contact's threads have finished loading (so an empty state means "no messages"). */
  threadsReady?: boolean;
  activeThread: string | null;
  /** When viewing a group thread, show participants / date / counts under the contact name. */
  groupThread?: BrowseGroupThreadMeta | null;
  onParticipantClick?: (
    participant: GroupParticipant,
    anchor: DOMRect,
  ) => void;
  /** Direct and/or groups exist in column 3 — prompt the user to pick one. */
  hasConversationChoices?: boolean;
  /**
   * When the conversations panel (panel 3) is visible, hide the duplicated
   * group identity header. Show it only when that panel is collapsed.
   */
  conversationsPanelCollapsed?: boolean;
  highlightTerms?: string[];
  scrollToMessageId?: number | null;
  /** Bumps on every find jump so scroll re-runs even for the same message id. */
  scrollToMessageNonce?: number;
  /** Hide the end-of-thread control while in-thread find is open. */
  findOpen?: boolean;
  hasOlder?: boolean;
  loadingOlder?: boolean;
  onLoadOlder?: () => void | Promise<void>;
  /** Ensure a calendar year is loaded before scrolling to its section. */
  onEnsureYear?: (year: number) => void | Promise<void>;
  searchConversationSections?: SearchConversationSection[];
}) {
  const { formatDateRange } = useDateTimeFormat();
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollAnchorRef = useRef<{ id: string; offset: number } | null>(null);
  const nearTopLoadingRef = useRef(false);
  const [headerExpanded, setHeaderExpanded] = useState(() => {
    if (typeof window === "undefined") return true;
    try {
      return sessionStorage.getItem(HEADER_EXPANDED_KEY) !== "0";
    } catch {
      return true;
    }
  });
  const [scrolledYear, setScrolledYear] = useState<number | null>(null);
  /** Thread key for which attachment-only filter is active (resets on thread change). */
  const [attachmentsFilterThread, setAttachmentsFilterThread] = useState<
    string | null
  >(null);
  const [awayFromBottom, setAwayFromBottom] = useState(false);
  const attachmentsOnly =
    attachmentsFilterThread != null && attachmentsFilterThread === activeThread;

  const toggleHeaderExpanded = () => {
    setHeaderExpanded((prev) => {
      const next = !prev;
      try {
        sessionStorage.setItem(HEADER_EXPANDED_KEY, next ? "1" : "0");
      } catch {
        /* ignore */
      }
      return next;
    });
  };

  const threadStats = useMemo(() => {
    if (groupThread) {
      return {
        messageCount: groupThread.messageCount,
        attachmentCount: messages.reduce((n, m) => n + m.attachments.length, 0),
      };
    }
    if (activeThread === "dm" && yearly.length > 0) {
      return {
        messageCount: yearly.reduce((n, y) => n + y.messageCount, 0),
        attachmentCount: yearly.reduce((n, y) => n + y.attachmentCount, 0),
      };
    }
    if (messages.length > 0) {
      return {
        messageCount: messages.length,
        attachmentCount: messages.reduce((n, m) => n + m.attachments.length, 0),
      };
    }
    return null;
  }, [groupThread, activeThread, yearly, messages]);

  const yearsInThread = useMemo(() => {
    // Prefer complete year metadata over partially loaded message pages.
    if (yearly.length > 0 && (activeThread === "dm" || !groupThread)) {
      return [...yearly].sort((a, b) => a.year - b.year);
    }
    if (yearly.length > 0 && activeThread?.startsWith("gfull-")) {
      // Group opens still receive contact yearly for DMs; derive from messages
      // plus any years already represented in the loaded page.
      const years = new Set<number>();
      for (const m of messages) {
        const y = yearFromTimestamp(m.timestamp);
        if (y != null) years.add(y);
      }
      return [...years]
        .sort((a, b) => a - b)
        .map((year) => ({
          year,
          messageCount: 0,
          attachmentCount: 0,
          dateStart: "",
          dateEnd: "",
          conversationIds: [] as number[],
        }));
    }
    const years = new Set<number>();
    for (const m of messages) {
      const y = yearFromTimestamp(m.timestamp);
      if (y != null) years.add(y);
    }
    return [...years]
      .sort((a, b) => a - b)
      .map((year) => ({
        year,
        messageCount: 0,
        attachmentCount: 0,
        dateStart: "",
        dateEnd: "",
        conversationIds: [] as number[],
      }));
  }, [yearly, messages, activeThread, groupThread]);

  const visibleMessages = useMemo(
    () =>
      attachmentsOnly
        ? messages.filter((m) => m.attachments.length > 0)
        : messages,
    [messages, attachmentsOnly],
  );

  const messagesByYear = useMemo(() => {
    // Progressive pages arrive chronological; only sort when needed.
    let chronological = visibleMessages;
    for (let i = 1; i < chronological.length; i++) {
      const prev = chronological[i - 1]!;
      const cur = chronological[i]!;
      if (
        cur.timestamp < prev.timestamp ||
        (cur.timestamp === prev.timestamp && cur.id < prev.id)
      ) {
        chronological = [...visibleMessages].sort((a, b) =>
          a.timestamp < b.timestamp
            ? -1
            : a.timestamp > b.timestamp
              ? 1
              : a.id - b.id,
        );
        break;
      }
    }
    const sections: Array<{ year: number; messages: MessageRow[] }> = [];
    for (const m of chronological) {
      const y = yearFromTimestamp(m.timestamp) ?? 0;
      const last = sections[sections.length - 1];
      if (!last || last.year !== y) {
        sections.push({ year: y, messages: [m] });
      } else {
        last.messages.push(m);
      }
    }
    return sections;
  }, [visibleMessages]);

  const yearIds = useMemo(
    () => new Set(messagesByYear.map((s) => s.year)),
    [messagesByYear],
  );
  const firstYear = messagesByYear[0]?.year ?? null;
  const activeYear =
    scrolledYear != null && yearIds.has(scrolledYear)
      ? scrolledYear
      : firstYear;

  useEffect(() => {
    const root = scrollRef.current;
    if (!root || messagesByYear.length === 0) return;

    const sectionEls = messagesByYear
      .map((s) => root.querySelector(`#year-${s.year}`))
      .filter((el): el is Element => el != null);
    if (sectionEls.length === 0) return;

    const visible = new Map<number, IntersectionObserverEntry>();

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const id = entry.target.id;
          const year = Number(id.replace(/^year-/, ""));
          if (!Number.isFinite(year)) continue;
          if (entry.isIntersecting) visible.set(year, entry);
          else visible.delete(year);
        }
        if (visible.size === 0) return;
        let bestYear: number | null = null;
        let bestTop = Number.POSITIVE_INFINITY;
        for (const [year, entry] of visible) {
          const top = entry.boundingClientRect.top;
          if (top < bestTop) {
            bestTop = top;
            bestYear = year;
          }
        }
        if (bestYear != null) setScrolledYear(bestYear);
      },
      {
        root,
        // Prefer the section occupying the upper portion of the scroll pane.
        rootMargin: "0px 0px -70% 0px",
        threshold: [0, 0.01, 0.1],
      },
    );

    for (const el of sectionEls) observer.observe(el);
    return () => observer.disconnect();
  }, [messagesByYear]);

  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const onScroll = () => {
      const distance =
        root.scrollHeight - root.scrollTop - root.clientHeight;
      setAwayFromBottom(distance > NEAR_BOTTOM_PX);

      if (
        onLoadOlder &&
        hasOlder &&
        !loadingOlder &&
        !loadingMessages &&
        !nearTopLoadingRef.current &&
        root.scrollTop < 120
      ) {
        nearTopLoadingRef.current = true;
        const firstMsg = root.querySelector('[id^="msg-"]');
        if (firstMsg) {
          const rootRect = root.getBoundingClientRect();
          const rect = firstMsg.getBoundingClientRect();
          scrollAnchorRef.current = {
            id: firstMsg.id,
            offset: rect.top - rootRect.top,
          };
        }
        void Promise.resolve(onLoadOlder()).finally(() => {
          nearTopLoadingRef.current = false;
        });
      }
    };
    onScroll();
    root.addEventListener("scroll", onScroll, { passive: true });
    return () => root.removeEventListener("scroll", onScroll);
  }, [
    visibleMessages.length,
    loadingMessages,
    activeThread,
    onLoadOlder,
    hasOlder,
    loadingOlder,
  ]);

  useLayoutEffect(() => {
    const root = scrollRef.current;
    const anchor = scrollAnchorRef.current;
    if (!root || !anchor) return;
    const el = root.querySelector(`#${CSS.escape(anchor.id)}`);
    if (!el) return;
    const rootRect = root.getBoundingClientRect();
    const rect = el.getBoundingClientRect();
    root.scrollTop += rect.top - rootRect.top - anchor.offset;
    scrollAnchorRef.current = null;
  }, [messages]);

  const jumpToYear = (year: number) => {
    setScrolledYear(year);
    const scroll = () => {
      const el = scrollRef.current?.querySelector(`#year-${year}`);
      el?.scrollIntoView({
        behavior: prefersReducedMotion() ? "auto" : "smooth",
        block: "start",
      });
    };
    if (onEnsureYear) {
      void Promise.resolve(onEnsureYear(year)).then(() => {
        requestAnimationFrame(scroll);
      });
      return;
    }
    scroll();
  };

  const jumpToLatest = () => {
    const root = scrollRef.current;
    if (!root) return;
    root.scrollTo({
      top: root.scrollHeight,
      behavior: prefersReducedMotion() ? "auto" : "smooth",
    });
  };

  useEffect(() => {
    if (scrollToMessageId == null || loadingMessages) return;
    const root = scrollRef.current;
    if (!root) return;
    const scrollToEl = () => {
      const el = root.querySelector(`#msg-${scrollToMessageId}`);
      if (!el) return false;
      el.scrollIntoView({
        behavior: prefersReducedMotion() ? "auto" : "smooth",
        block: "center",
      });
      return true;
    };
    if (scrollToEl()) return;
    // Message may still be mounting after a page load — retry once.
    const frame = requestAnimationFrame(() => {
      scrollToEl();
    });
    return () => cancelAnimationFrame(frame);
  }, [
    scrollToMessageId,
    scrollToMessageNonce,
    loadingMessages,
    visibleMessages,
  ]);

  const sourceOptions = [
    {
      id: null as string | null,
      label: "Combined",
      enabled: true,
      count: sourceCounts.all,
    },
    ...sources.map((id) => ({
      id,
      label: formatSourceLabel(id),
      enabled: messageSources.includes(id),
      count: sourceCounts.bySource[id] ?? 0,
    })),
  ];

  const stripItems: Array<{
    key: string;
    label: string;
    title?: string;
    active: boolean;
    disabled?: boolean;
    onClick: () => void;
  }> = sourceOptions.map((opt) => ({
    key: `src-${opt.id ?? "all"}`,
    label: opt.label,
    title: `${opt.label}: ${opt.count.toLocaleString()} messages`,
    active: opt.id === null ? source === null : source === opt.id,
    disabled: !opt.enabled,
    onClick: () => {
      if (!opt.enabled) return;
      onSourceChange(opt.id);
    },
  }));

  const yearItems = yearsInThread.map((y) => ({
    key: `y-${y.year}`,
    year: y.year,
    label: String(y.year),
    title:
      y.messageCount > 0
        ? `${y.year}: ${y.messageCount.toLocaleString()} messages`
        : `Jump to ${y.year}`,
    active: activeYear === y.year,
    onClick: () => jumpToYear(y.year),
  }));

  const dateLabel = groupThread
    ? formatDateRange(groupThread.dateStart, groupThread.dateEnd)
    : null;

  // Group identity lives in the tree/inspector; only repeat it when the
  // conversations list panel is collapsed (Group Messages layout).
  const showGroupIdentity =
    conversationsPanelCollapsed &&
    !!groupThread &&
    (groupThread.participants.length > 0 || !!dateLabel);
  const hasMessages = messages.length > 0;
  const showYearJump = hasMessages && yearItems.length > 0;
  const showSourceRow = stripItems.length > 0 && sourceCounts.all > 0;
  const showPhotosFilter = threadStats != null;
  const showUtility = showSourceRow || showPhotosFilter;
  const showHeader = showGroupIdentity || showYearJump || showUtility;

  return (
    <section className="relative flex h-full min-h-0 flex-col bg-bg">
      {showHeader && (
        <div className="shrink-0 border-b border-border px-5 py-2">
          {(headerExpanded || !showYearJump) && showGroupIdentity && (
            <div className="text-center">
              {groupThread && groupThread.participants.length > 0 ? (
                <div className="flex flex-wrap items-center justify-center gap-y-0 text-[14px] font-medium leading-snug text-text">
                  {groupThread.participants.map((p, idx) => (
                    <span
                      key={`${p.handle}-${idx}`}
                      className="inline-flex items-center"
                    >
                      {onParticipantClick ? (
                        <GroupParticipantChip
                          label={p.name}
                          onClick={(anchor) => onParticipantClick(p, anchor)}
                        />
                      ) : (
                        <span className="whitespace-nowrap px-1.5 py-0.5">
                          {p.name}
                        </span>
                      )}
                    </span>
                  ))}
                </div>
              ) : null}

              {dateLabel && (
                <div className="mt-1 text-[14px] text-muted tabular-nums">
                  {dateLabel}
                </div>
              )}
            </div>
          )}

          {showYearJump && (
            <div
              className={`relative flex min-h-7 items-center justify-center ${
                headerExpanded && showGroupIdentity ? "mt-1.5" : ""
              }`}
            >
              <div
                className={`flex max-w-full flex-wrap items-center justify-center gap-x-1 gap-y-0.5 ${
                  showGroupIdentity ? "pr-8" : ""
                }`}
              >
                {yearItems.map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    title={item.title}
                    onClick={item.onClick}
                    className={`rounded-md px-1.5 py-0.5 text-[13px] font-medium tabular-nums transition-colors ${
                      item.active
                        ? "text-accent underline decoration-accent decoration-2 underline-offset-[5px]"
                        : "text-text hover:text-accent"
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
              {showGroupIdentity && (
                <button
                  type="button"
                  aria-expanded={headerExpanded}
                  aria-label={
                    headerExpanded
                      ? "Hide thread details"
                      : "Show thread details"
                  }
                  title={headerExpanded ? "Hide details" : "Show details"}
                  onClick={toggleHeaderExpanded}
                  className="absolute top-1/2 right-0 inline-flex size-7 -translate-y-1/2 items-center justify-center rounded-md text-muted transition-colors hover:bg-hover hover:text-text"
                >
                  <ChevronDownIcon
                    className={`size-3.5 transition-transform ${
                      headerExpanded ? "rotate-180" : ""
                    }`}
                  />
                </button>
              )}
            </div>
          )}

          {showUtility && (
            <div
              className={`flex flex-wrap items-center justify-center gap-x-3 gap-y-1 ${
                showYearJump || showGroupIdentity ? "mt-1.5" : ""
              }`}
            >
              {showSourceRow && (
                <div className="flex max-w-full flex-wrap items-center justify-center gap-y-0.5">
                  {stripItems.map((item, i) => (
                    <span key={item.key} className="flex items-center">
                      {i > 0 && (
                        <span
                          className="mx-1.5 text-[12px] text-muted/45"
                          aria-hidden
                        >
                          ·
                        </span>
                      )}
                      <button
                        type="button"
                        disabled={item.disabled}
                        title={item.title}
                        onClick={item.onClick}
                        className={`text-[12px] font-medium ${
                          item.disabled
                            ? "cursor-default text-muted/40"
                            : item.active
                              ? "text-accent"
                              : "text-muted hover:text-text"
                        }`}
                      >
                        {item.label}
                      </button>
                    </span>
                  ))}
                </div>
              )}
              {showSourceRow && showPhotosFilter ? (
                <span className="hidden text-muted/40 sm:inline" aria-hidden>
                  |
                </span>
              ) : null}
              {showPhotosFilter && threadStats ? (
                <button
                  type="button"
                  title={
                    attachmentsOnly
                      ? "Show all messages"
                      : "Show only messages with photos or files"
                  }
                  aria-label={
                    attachmentsOnly
                      ? "Show all messages"
                      : "Filter to photos and files"
                  }
                  aria-pressed={attachmentsOnly}
                  disabled={threadStats.attachmentCount === 0}
                  onClick={() =>
                    setAttachmentsFilterThread((prev) =>
                      prev === activeThread ? null : activeThread,
                    )
                  }
                  className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[12px] font-medium transition-colors outline-none focus:outline-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent ${
                    threadStats.attachmentCount === 0
                      ? "cursor-default border-border/50 text-muted/40"
                      : attachmentsOnly
                        ? "border-accent/50 bg-accent/15 text-accent"
                        : "border-border text-muted hover:border-accent/40 hover:text-accent"
                  }`}
                >
                  <PaperclipIcon className="size-3.5 shrink-0 opacity-80" />
                  Photos &amp; files
                </button>
              ) : null}
            </div>
          )}
        </div>
      )}

      <div
        ref={scrollRef}
        className="min-h-0 flex-1 overflow-y-auto px-4 pt-3 pb-4"
      >
        {!activeThread && !loadingMessages && detail && (
          <p className="pt-8 text-center text-[13px] text-muted">
            {!threadsReady
              ? "Loading messages…"
              : hasConversationChoices
                ? "Pick a conversation"
                : "No messages"}
          </p>
        )}
        {loadingMessages && messages.length === 0 && (
          <p className="pt-8 text-center text-[13px] text-muted">
            Loading messages…
          </p>
        )}
        {messages.length > 0 &&
          attachmentsOnly &&
          visibleMessages.length === 0 && (
            <p className="pt-8 text-center text-[13px] text-muted">
              No messages with photos or files
            </p>
          )}
        {visibleMessages.length > 0 &&
          (searchConversationSections.length > 1 ? (
            <SearchConversationMessageSections
              sections={searchConversationSections}
              messages={visibleMessages}
              highlightTerms={highlightTerms}
            />
          ) : (
            <ThreadMessageSections
              messagesByYear={messagesByYear}
              highlightTerms={highlightTerms}
              hasOlder={hasOlder}
              loadingOlder={loadingOlder}
            />
          ))}
      </div>

      {awayFromBottom && !findOpen && visibleMessages.length > 0 && (
        <button
          type="button"
          onClick={jumpToLatest}
          title="Scroll to the newest messages"
          className="absolute right-4 bottom-4 z-20 inline-flex items-center gap-1.5 rounded-full border border-border bg-elevated/95 px-3 py-1.5 text-[12px] font-medium text-text shadow-[0_4px_16px_rgba(0,0,0,0.28)] backdrop-blur-sm transition-colors hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
        >
          Newest messages
          <ChevronDownIcon className="size-3.5 opacity-80" />
        </button>
      )}
    </section>
  );
}
