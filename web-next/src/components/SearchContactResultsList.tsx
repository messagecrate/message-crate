"use client";

import type { SearchContactHit, SearchConversationHit } from "@/lib/search";
import type { MouseEvent } from "react";
import { BrowseContactRow } from "./BrowseContactRow";
import { LoadMoreResultsRow, SearchHitSummary } from "./SearchResultsList";

/** Match BrowsePeopleTreePane: left edge of contact name / phone / labels. */
const NESTED_FROM_NAME_PX = 76;
const EMPTY_SELECTED_CONTACT_IDS: ReadonlySet<number> = new Set();

/**
 * Search results grouped by contact: the normal contact row, expandable to the
 * conversations of theirs that matched. Selecting contacts here feeds the same
 * bulk actions as the contact list, so matches can be labeled directly.
 */
export function SearchContactResultsList({
  contacts,
  total,
  loading,
  highlightTerms,
  selectedConversationId,
  selectedContactIds = EMPTY_SELECTED_CONTACT_IDS,
  expandedContactIds = EMPTY_SELECTED_CONTACT_IDS,
  contactId = null,
  onToggleContact,
  onOpenContact,
  onToggleExpand,
  onSelectHit,
  onContactContextMenu,
  emptyLabel = "No matches",
  loadingMore = false,
  onLoadMore,
}: {
  contacts: SearchContactHit[];
  total: number;
  loading: boolean;
  highlightTerms?: string[];
  selectedConversationId: number | null;
  selectedContactIds?: ReadonlySet<number>;
  expandedContactIds?: ReadonlySet<number>;
  /** Contact open in the detail pane. */
  contactId?: number | null;
  onToggleContact?: (
    contactId: number,
    mods?: { shiftKey: boolean },
  ) => void;
  /** Open/focus on plain name click. */
  onOpenContact?: (contactId: number) => void;
  onToggleExpand?: (contactId: number) => void;
  onSelectHit?: (hit: SearchConversationHit) => void;
  onContactContextMenu?: (contactId: number, x: number, y: number) => void;
  emptyLabel?: string;
  loadingMore?: boolean;
  onLoadMore?: () => void;
}) {
  if (loading) {
    return (
      <p className="px-3 py-8 text-center text-[13px] text-muted">Searching…</p>
    );
  }

  if (contacts.length === 0) {
    return (
      <p className="px-3 py-8 text-center text-[13px] text-muted">
        {emptyLabel}
      </p>
    );
  }

  const selectionActive = selectedContactIds.size >= 1;

  return (
    <div className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable]">
      <div className="sticky top-0 z-10 border-b border-border bg-sidebar px-3 py-1 text-[11px] font-semibold tracking-wider text-muted uppercase">
        Results · {total.toLocaleString()}
      </div>
      {contacts.map(({ contact, hits }, index) => {
        const expanded = expandedContactIds.has(contact.id);
        const checked = selectedContactIds.has(contact.id);
        const active = contact.id === contactId;
        return (
          <div key={contact.id}>
            <BrowseContactRow
              contact={contact}
              active={active}
              checked={checked}
              selectionActive={selectionActive}
              expanded={expanded}
              showExpandChevron
              showInsetDivider={!expanded && index < contacts.length - 1}
              onSelectColumnClick={(id, e: MouseEvent) => {
                if (e.shiftKey || e.metaKey || e.ctrlKey) {
                  onToggleContact?.(id, { shiftKey: e.shiftKey });
                  return;
                }
                // Avatar column: open when nothing checked, else toggle.
                if (selectedContactIds.size === 0 && onOpenContact) {
                  onOpenContact(id);
                  return;
                }
                onToggleContact?.(id, { shiftKey: false });
              }}
              onNamePhoneClick={(id, e) => {
                if (e.shiftKey || e.metaKey || e.ctrlKey) {
                  onToggleContact?.(id, { shiftKey: e.shiftKey });
                  return;
                }
                // Name always opens so focus moves cleanly between contacts.
                if (onOpenContact) {
                  onOpenContact(id);
                  return;
                }
                onToggleContact?.(id, { shiftKey: false });
              }}
              onContextMenu={(id, x, y) => onContactContextMenu?.(id, x, y)}
              onToggleExpand={onToggleExpand}
            />
            {expanded ? (
              <div
                className="mb-1 mr-2 overflow-hidden rounded-md bg-elevated/55"
                style={{ marginLeft: NESTED_FROM_NAME_PX }}
              >
                {(() => {
                  const directHits = hits.filter(
                    (hit) => hit.conversationType === "one-to-one",
                  );
                  const groupHits = hits.filter(
                    (hit) => hit.conversationType === "group",
                  );
                  const emptyRow = (label: string, bordered: boolean) => (
                    <p
                      className={`px-3 py-2.5 text-[12px] text-muted ${
                        bordered ? "border-b border-border/40" : ""
                      }`}
                    >
                      {label}
                    </p>
                  );
                  const hitButton = (
                    hit: SearchConversationHit,
                    isDirect: boolean,
                    showDivider: boolean,
                  ) => (
                    <button
                      key={hit.conversationId}
                      type="button"
                      onClick={() => onSelectHit?.(hit)}
                      className={`relative flex w-full min-w-0 flex-col gap-0.5 py-2 pr-3 pl-3 text-left transition-colors ${
                        selectedConversationId === hit.conversationId
                          ? "bg-accent/20 hover:bg-accent/25"
                          : isDirect
                            ? "bg-sidebar hover:bg-hover-strong"
                            : "bg-transparent hover:bg-hover"
                      }`}
                    >
                      {selectedConversationId === hit.conversationId ? (
                        <span
                          aria-hidden
                          className="absolute top-1 bottom-1 left-0 w-1 rounded-full bg-accent/80"
                        />
                      ) : null}
                      {showDivider ? (
                        <span
                          aria-hidden
                          className="pointer-events-none absolute bottom-0 left-1/2 h-px w-[95%] -translate-x-1/2 bg-border/55"
                        />
                      ) : null}
                      <SearchHitSummary
                        hit={hit}
                        highlightTerms={highlightTerms}
                        showMeta={false}
                      />
                    </button>
                  );
                  return (
                    <>
                      {directHits.length === 0
                        ? emptyRow("No direct messages", true)
                        : directHits.map((hit, i) =>
                            hitButton(
                              hit,
                              true,
                              i < directHits.length - 1 || groupHits.length > 0,
                            ),
                          )}
                      {groupHits.length === 0
                        ? emptyRow("No group messages", false)
                        : groupHits.map((hit, i) =>
                            hitButton(
                              hit,
                              false,
                              i < groupHits.length - 1,
                            ),
                          )}
                    </>
                  );
                })()}
              </div>
            ) : null}
          </div>
        );
      })}
      <LoadMoreResultsRow
        shown={contacts.length}
        total={total}
        loadingMore={loadingMore}
        onLoadMore={onLoadMore}
      />
    </div>
  );
}
