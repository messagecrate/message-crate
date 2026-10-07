"use client";

import type { CollapsedGroupConversation } from "@/lib/groupChatList";
import { formatPhoneDisplay } from "@/lib/phoneE164";
import type {
  ContactDetail,
  ContactHandle,
  ContactListItem,
  GroupChatThread,
  GroupParticipant,
  YearThread,
} from "@/lib/types";
import type { ReactNode } from "react";
import { collapsedParticipantLabels } from "./GroupConversationRow";
import { HandleTypeBadge } from "./HandleTypeBadge";
import { ChevronDownIcon } from "./icons";
import { useDateTimeFormat } from "./useDateTimeFormat";

/** Name/phone that open edit — hover chevron signals menu, not a link. */
function EditableContactField({
  onEdit,
  children,
  className = "",
  title = "Edit contact",
}: {
  onEdit: (anchorRect: DOMRect) => void;
  children: ReactNode;
  className?: string;
  title?: string;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={(e) =>
        onEdit((e.currentTarget as HTMLElement).getBoundingClientRect())
      }
      className={`group inline-flex max-w-full items-center gap-1 text-left outline-none ${className}`}
    >
      <span className="min-w-0 truncate">{children}</span>
      <ChevronDownIcon className="size-3.5 shrink-0 text-muted opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
    </button>
  );
}

function inspectorParticipants(
  open: CollapsedGroupConversation | null,
  meta: { participants: GroupParticipant[] } | null,
): GroupParticipant[] {
  if (open?.participants?.length) return open.participants;
  if (meta?.participants?.length) return meta.participants;
  if (!open) return [];
  const labels = collapsedParticipantLabels(open);
  const handles = open.participantHandles;
  return labels.map((name, i) => ({
    name,
    handle: handles[i] ?? name,
    handleType: null,
    contactId: null,
  }));
}

const SECTION_LABEL =
  "mb-1 text-[12px] font-semibold tracking-wide text-muted uppercase";

function StatRow({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex items-start justify-between gap-3 py-1.5">
      <span className="text-[13px] text-muted">{label}</span>
      <span className="min-w-0 text-right text-[13px] text-text tabular-nums">
        {value}
      </span>
    </div>
  );
}

/**
 * Handle rows in the inspector: typed handles (badge + service) when known,
 * otherwise a plain fallback of raw strings (e.g. search-focused contacts).
 */
function HandlesSection({
  handles,
  fallback,
  canEdit,
  onEdit,
}: {
  handles?: ContactHandle[];
  fallback?: string[];
  canEdit: boolean;
  onEdit?: () => void;
}) {
  const typed = handles && handles.length > 0 ? handles : null;
  const plain = !typed && fallback && fallback.length > 0 ? fallback : null;
  if (!typed && !plain) return null;
  const editable = canEdit && !!onEdit;
  const handleRow = (raw: string) => (
    <>
      {editable ? (
        <EditableContactField
          onEdit={onEdit!}
          className="min-w-0 text-[14px] text-text tabular-nums"
        >
          {formatPhoneDisplay(raw)}
        </EditableContactField>
      ) : (
        <span className="min-w-0 truncate text-[14px] text-text tabular-nums">
          {formatPhoneDisplay(raw)}
        </span>
      )}
    </>
  );
  return (
    <div className="mt-2">
      <div className={SECTION_LABEL}>Handles</div>
      <ul className="space-y-0.5">
        {typed
          ? typed.map((h) => (
              <li
                key={`${h.handle_type}\0${h.raw}`}
                className="flex min-w-0 items-center gap-1.5"
              >
                {handleRow(h.raw)}
                <HandleTypeBadge type={h.handle_type} />
                {h.normalizedNote ? (
                  <span
                    title={h.normalizedNote}
                    className="shrink-0 rounded bg-danger/15 px-1.5 py-0.5 text-[11px] font-medium text-danger"
                  >
                    needs review
                  </span>
                ) : null}
                {h.service ? (
                  <span className="shrink-0 truncate text-[11px] text-muted">
                    {h.service}
                  </span>
                ) : null}
              </li>
            ))
          : plain!.map((p) => (
              <li key={p} className="flex min-w-0 items-center gap-1.5">
                {handleRow(p)}
              </li>
            ))}
      </ul>
    </div>
  );
}

function LabelChips({ labels }: { labels: string[] }) {
  if (labels.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1.5">
      {labels.map((label) => (
        <span
          key={label}
          className="rounded bg-elevated px-2 py-0.5 text-[13px] font-medium text-text"
        >
          {label}
        </span>
      ))}
    </div>
  );
}

function YearBreakdown({ yearly }: { yearly: { year: number; messageCount: number }[] }) {
  if (yearly.length === 0) return null;
  return (
    <div className="mt-3">
      <div className={SECTION_LABEL}>Activity by year</div>
      <ul className="space-y-0.5">
        {[...yearly]
          .sort((a, b) => b.year - a.year)
          .map((y) => (
            <li
              key={y.year}
              className="flex items-center justify-between gap-2 text-[13px]"
            >
              <span className="text-muted tabular-nums">{y.year}</span>
              <span className="text-text tabular-nums">
                {y.messageCount.toLocaleString()} msg
                {y.messageCount === 1 ? "" : "s"}
              </span>
            </li>
          ))}
      </ul>
    </div>
  );
}

export function BrowseDetailsInspector({
  hasContactSelection,
  hasGroupSelection,
  selectedContacts,
  selectedGroupRows,
  focusedContact,
  detail,
  yearly,
  conversationYearly = [],
  groupChats,
  activeThread,
  groupThreadMeta,
  directThreadMeta = null,
  openConversation,
  onClearContactSelection,
  onClearGroupSelection,
  onEditContact,
  onParticipantClick,
  emptyGuidance = "Select a person or conversation to see contact details, conversation stats, and selection summaries here.",
}: {
  hasContactSelection: boolean;
  hasGroupSelection: boolean;
  selectedContacts: ContactListItem[];
  selectedGroupRows: CollapsedGroupConversation[];
  focusedContact: {
    id: number;
    displayName: string;
    preferredHandle: string | null;
    phones?: string[];
    labels?: string[];
  } | null;
  detail: ContactDetail | null;
  yearly: YearThread[];
  /** Per-year activity for the open group conversation (not contact DM years). */
  conversationYearly?: YearThread[];
  groupChats: GroupChatThread[];
  activeThread: string | null;
  groupThreadMeta: {
    title?: string | null;
    namedTitle?: string | null;
    participants: GroupParticipant[];
    dateStart: string | null;
    dateEnd: string | null;
    messageCount: number;
    /** Sum of attachments on loaded messages for the open group thread. */
    attachmentCount?: number;
  } | null;
  /** Stats for an open 1-1 when contact detail is missing (e.g. search). */
  directThreadMeta?: {
    title: string;
    dateStart: string | null;
    dateEnd: string | null;
    messageCount: number;
    attachmentCount: number;
  } | null;
  openConversation: CollapsedGroupConversation | null;
  onClearContactSelection: () => void;
  onClearGroupSelection: () => void;
  onEditContact?: () => void;
  /** Open edit (or create) for a group participant. */
  onParticipantClick?: (
    participant: GroupParticipant,
    anchorRect: DOMRect,
  ) => void;
  emptyGuidance?: string;
}) {
  const { formatDate, formatDateRange } = useDateTimeFormat();

  if (hasGroupSelection) {
    const totalMessages = selectedGroupRows.reduce(
      (sum, g) => sum + g.messageCount,
      0,
    );
    return (
      <InspectorShell title="Selection">
        <div className="mb-3 flex items-center justify-between gap-2">
          <p className="text-[13px] font-semibold text-text">
            {selectedGroupRows.length} conversation
            {selectedGroupRows.length === 1 ? "" : "s"}
          </p>
          <button
            type="button"
            onClick={onClearGroupSelection}
            className="rounded-md bg-hover px-2 py-1 text-[11px] text-text hover:bg-hover-strong"
          >
            Clear
          </button>
        </div>
        <StatRow label="Messages" value={totalMessages.toLocaleString()} />
        <ul className="mt-3 divide-y divide-border/50 border-t border-border/50">
          {selectedGroupRows.map((g) => (
            <li key={g.conversationId} className="py-2">
              <div className="truncate text-[13px] font-medium text-text">
                {g.namedTitle?.trim() || g.title || "Group message"}
              </div>
              <div className="mt-0.5 text-[11px] text-muted tabular-nums">
                {formatDateRange(g.dateStart, g.dateEnd, " – ")}
              </div>
            </li>
          ))}
        </ul>
      </InspectorShell>
    );
  }

  if (hasContactSelection) {
    return (
      <InspectorShell title="Selection">
        <div className="mb-3 flex items-center justify-between gap-2">
          <p className="text-[13px] font-semibold text-text">
            {selectedContacts.length} contact
            {selectedContacts.length === 1 ? "" : "s"}
          </p>
          <button
            type="button"
            onClick={onClearContactSelection}
            className="rounded-md bg-hover px-2 py-1 text-[11px] text-text hover:bg-hover-strong"
          >
            Clear
          </button>
        </div>
        <ul className="divide-y divide-border/50 border-t border-border/50">
          {selectedContacts.map((c) => (
            <li
              key={c.id}
              className="flex items-center justify-between gap-3 py-2"
            >
              <span className="min-w-0 truncate text-[13px] text-text">
                {c.displayName}
              </span>
              <span className="shrink-0 text-[12px] text-muted tabular-nums">
                {c.preferredHandle
                  ? formatPhoneDisplay(c.preferredHandle)
                  : ""}
              </span>
            </li>
          ))}
        </ul>
        <p className="mt-3 text-[12px] text-muted">
          Shared group messages for this selection appear in the tree.
        </p>
      </InspectorShell>
    );
  }

  if (activeThread?.startsWith("gfull-") && (openConversation || groupThreadMeta)) {
    const g = openConversation;
    // Only a real group title — skip participant-derived fallbacks already listed below.
    const namedTitle =
      g?.namedTitle?.trim() || groupThreadMeta?.namedTitle?.trim() || null;
    const dateStart = g?.dateStart ?? groupThreadMeta?.dateStart ?? null;
    const dateEnd = g?.dateEnd ?? groupThreadMeta?.dateEnd ?? null;
    const participants = inspectorParticipants(g, groupThreadMeta);
    const participantCount =
      g != null && g.participantCount > 0
        ? g.participantCount
        : participants.length;
    const messageCount =
      g?.messageCount ?? groupThreadMeta?.messageCount ?? 0;
    const attachmentCount = groupThreadMeta?.attachmentCount ?? 0;
    const canEditParticipants = typeof onParticipantClick === "function";

    return (
      <InspectorShell title="Conversation">
        <div className={SECTION_LABEL}>Group</div>
        {namedTitle ? (
          <h2 className="text-[15px] font-semibold leading-snug text-text">
            {namedTitle}
          </h2>
        ) : null}
        <div
          className={`${namedTitle ? "mt-2" : ""} flex items-baseline justify-between gap-2`}
        >
          <span className="text-[12px] font-semibold tracking-wide text-muted uppercase">
            Participants
          </span>
          <span className="text-[13px] text-muted tabular-nums">
            {participantCount.toLocaleString()}
          </span>
        </div>
        {participants.length > 0 && (
          <ul className="mt-1.5 list-disc space-y-1.5 pl-4">
            {participants.map((p, idx) => {
              const label = (p.name || p.handle).trim() || "Unknown";
              const display =
                p.handle && label !== p.handle
                  ? label
                  : p.handle
                    ? formatPhoneDisplay(p.handle)
                    : label;
              return (
                <li
                  key={`${p.handle || p.name}-${idx}`}
                  className="text-[14px] leading-snug break-all"
                >
                  {canEditParticipants ? (
                    <EditableContactField
                      onEdit={(rect) => onParticipantClick(p, rect)}
                      title={
                        p.contactId != null
                          ? `Edit ${display}`
                          : `Create contact for ${display}`
                      }
                      className="text-[14px] text-text"
                    >
                      {display}
                    </EditableContactField>
                  ) : (
                    <span className="text-muted">{display}</span>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        <div className="mt-3 border-t border-border/50 pt-2">
          <StatRow label="Messages" value={messageCount.toLocaleString()} />
          <StatRow
            label="Photos & files"
            value={attachmentCount.toLocaleString()}
          />
          {dateStart ? (
            <StatRow label="First message" value={formatDate(dateStart)} />
          ) : null}
          {dateEnd ? (
            <StatRow label="Last message" value={formatDate(dateEnd)} />
          ) : null}
        </div>
        <YearBreakdown yearly={conversationYearly} />
      </InspectorShell>
    );
  }

  if (activeThread === "dm") {
    const name =
      detail?.displayName ||
      focusedContact?.displayName ||
      directThreadMeta?.title ||
      "Direct";
    const handles = detail?.handles?.length ? detail.handles : [];
    const phones =
      handles.length > 0
        ? []
        : focusedContact?.phones?.length
          ? focusedContact.phones
          : focusedContact?.preferredHandle
            ? [focusedContact.preferredHandle]
            : [];
    const labels = (detail?.labels ?? focusedContact?.labels ?? []).filter(
      Boolean,
    );
    const yearlyMessages = yearly.reduce((s, y) => s + y.messageCount, 0);
    const yearlyAttachments = yearly.reduce((s, y) => s + y.attachmentCount, 0);
    const dmMessages =
      yearly.length > 0
        ? yearlyMessages
        : (directThreadMeta?.messageCount ?? 0);
    const dmAttachments =
      yearly.length > 0
        ? yearlyAttachments
        : (directThreadMeta?.attachmentCount ?? 0);
    const range =
      yearly.length > 0
        ? {
            start: yearly.reduce(
              (min, y) => (y.dateStart < min ? y.dateStart : min),
              yearly[0]!.dateStart,
            ),
            end: yearly.reduce(
              (max, y) => (y.dateEnd > max ? y.dateEnd : max),
              yearly[0]!.dateEnd,
            ),
          }
        : directThreadMeta?.dateStart && directThreadMeta?.dateEnd
          ? {
              start: directThreadMeta.dateStart,
              end: directThreadMeta.dateEnd,
            }
          : null;

    const canEdit = !!onEditContact;

    return (
      <InspectorShell title="Conversation">
        <div className={SECTION_LABEL}>Direct</div>
        {canEdit ? (
          <EditableContactField
            onEdit={() => onEditContact()}
            className="text-[15px] font-semibold text-text"
          >
            {name}
          </EditableContactField>
        ) : (
          <h2 className="text-[15px] font-semibold text-text">{name}</h2>
        )}
        <HandlesSection
          handles={handles}
          fallback={phones}
          canEdit={canEdit}
          onEdit={canEdit ? onEditContact : undefined}
        />
        {labels.length > 0 && (
          <div className="mt-2">
            <LabelChips labels={labels} />
          </div>
        )}
        <div className="mt-3 border-t border-border/50 pt-2">
          <StatRow label="Messages" value={dmMessages.toLocaleString()} />
          <StatRow
            label="Photos & files"
            value={dmAttachments.toLocaleString()}
          />
          {range?.start ? (
            <StatRow label="First message" value={formatDate(range.start)} />
          ) : null}
          {range?.end ? (
            <StatRow label="Last message" value={formatDate(range.end)} />
          ) : null}
        </div>
        <YearBreakdown yearly={yearly} />
      </InspectorShell>
    );
  }

  if (detail || focusedContact) {
    const c = detail;
    const name = c?.displayName || focusedContact?.displayName || "Contact";
    const handles = c?.handles?.length ? c.handles : [];
    const phones =
      handles.length > 0
        ? []
        : focusedContact?.preferredHandle
          ? [focusedContact.preferredHandle]
          : [];
    const labels = (c?.labels ?? []).filter(Boolean);
    const dmCount = c?.messageCount ?? 0;
    const groupCount = c?.groupMessageCount ?? groupChats.length;
    const rangeStart = c?.dateStart ?? null;
    const rangeEnd = c?.dateEnd ?? null;
    const canEdit = !!onEditContact;

    return (
      <InspectorShell title="Contact">
        <div className="mb-2 min-w-0">
          {canEdit ? (
            <EditableContactField
              onEdit={() => onEditContact()}
              className="text-[15px] font-semibold text-text"
            >
              {name}
            </EditableContactField>
          ) : (
            <h2 className="min-w-0 text-[15px] font-semibold text-text">
              {name}
            </h2>
          )}
        </div>
        <div className="mb-3">
          <HandlesSection
            handles={handles}
            fallback={phones}
            canEdit={canEdit}
            onEdit={canEdit ? onEditContact : undefined}
          />
        </div>
        {labels.length > 0 && (
          <div className="mb-3">
            <LabelChips labels={labels} />
          </div>
        )}
        <div className="border-t border-border/50 pt-2">
          <StatRow label="Direct messages" value={dmCount.toLocaleString()} />
          <StatRow label="Group conversations" value={groupCount} />
          {rangeStart ? (
            <StatRow label="First message" value={formatDate(rangeStart)} />
          ) : null}
          {rangeEnd ? (
            <StatRow label="Last message" value={formatDate(rangeEnd)} />
          ) : null}
        </div>
        <YearBreakdown yearly={yearly} />
      </InspectorShell>
    );
  }

  return (
    <InspectorShell title="Details">
      <p className="text-[13px] leading-relaxed text-muted">{emptyGuidance}</p>
    </InspectorShell>
  );
}

function InspectorShell({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <aside className="flex h-full min-h-0 w-full flex-col bg-sidebar">
      <div className="flex h-[45px] shrink-0 items-center border-b border-border px-3">
        <h1 className="truncate text-[13px] font-semibold text-text">{title}</h1>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3 [scrollbar-gutter:stable]">
        {children}
      </div>
    </aside>
  );
}
