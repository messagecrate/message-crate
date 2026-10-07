"use client";

import { formatPhoneDisplay } from "@/lib/phoneE164";
import type { ContactListItem } from "@/lib/types";
import { isDeletionUiBlocked } from "@/lib/v1Capabilities";
import type { RefObject } from "react";
import { NewContactIcon } from "./BrowseContactList";
import {
  ChevronRightIcon,
  PencilIcon,
  PeopleGroupIcon,
  XIcon,
} from "./icons";

export type BrowseContactCtxMenuState = {
  id: number;
  x: number;
  y: number;
};

export function BrowseContactCtxMenu({
  menuRef,
  ctxMenu,
  saving,
  groupTrashSaving,
  hasSelection,
  /** How many contacts delete will remove (1 when only the row under the cursor). */
  deleteCount = 1,
  contactCreating,
  contactEditing,
  isNameless,
  onMouseEnterItem,
  onNewContact,
  onEdit,
  onMergeInto,
  onLabelsEnter,
  onLabelsLeave,
  onDelete,
}: {
  menuRef: RefObject<HTMLDivElement | null>;
  ctxMenu: BrowseContactCtxMenuState;
  saving: boolean;
  groupTrashSaving: boolean;
  hasSelection: boolean;
  deleteCount?: number;
  contactCreating: boolean;
  contactEditing: boolean;
  isNameless: boolean;
  onMouseEnterItem: () => void;
  onNewContact: (anchorEl: HTMLElement) => void;
  onEdit: (anchorEl: HTMLElement) => void;
  onMergeInto: () => void;
  onLabelsEnter: (anchor: DOMRect) => void;
  onLabelsLeave: () => void;
  onDelete: () => void;
}) {
  const deleteLabel =
    deleteCount > 1 ? "Delete contacts" : "Delete contact";

  return (
    <div
      ref={menuRef}
      className="fixed z-[100] min-w-[180px] rounded-lg border border-border bg-popover py-1 shadow-xl"
      style={{ left: ctxMenu.x, top: ctxMenu.y }}
    >
      <button
        type="button"
        disabled={saving || contactCreating || contactEditing}
        className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-[13px] text-text hover:bg-hover-strong disabled:opacity-40"
        onMouseEnter={onMouseEnterItem}
        onClick={(e) => onNewContact(e.currentTarget)}
      >
        <NewContactIcon className="size-5 shrink-0 opacity-80" />
        New contact
      </button>
      <button
        type="button"
        disabled={
          saving || hasSelection || contactCreating || contactEditing
        }
        className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-[13px] text-text hover:bg-hover-strong disabled:opacity-40"
        onMouseEnter={onMouseEnterItem}
        onClick={(e) => onEdit(e.currentTarget)}
      >
        <PencilIcon className="size-5 shrink-0 opacity-80" />
        Edit
      </button>
      {isNameless && !hasSelection && (
        <button
          type="button"
          disabled={
            isDeletionUiBlocked() ||
            saving ||
            contactCreating ||
            contactEditing
          }
          className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-[13px] text-text hover:bg-hover-strong disabled:opacity-40"
          onMouseEnter={onMouseEnterItem}
          onClick={onMergeInto}
        >
          <PeopleGroupIcon className="size-5 shrink-0 opacity-80" />
          Merge into…
        </button>
      )}
      <button
        type="button"
        disabled={saving || contactCreating || contactEditing}
        className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-[13px] text-text hover:bg-hover-strong disabled:opacity-40"
        onMouseEnter={(e) => {
          if (saving || contactCreating || contactEditing) return;
          onLabelsEnter(e.currentTarget.getBoundingClientRect());
        }}
        onMouseLeave={onLabelsLeave}
      >
        <PeopleGroupIcon className="size-5 shrink-0 opacity-80" />
        <span className="min-w-0 flex-1">Labels</span>
        <ChevronRightIcon className="size-3.5 shrink-0 opacity-70" />
      </button>
      <div className="my-1 border-t border-border/60" />
      <button
        type="button"
        disabled={isDeletionUiBlocked() || saving || groupTrashSaving}
        className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-[13px] text-text hover:bg-red-500/15 hover:text-red-300 disabled:opacity-50"
        onMouseEnter={onMouseEnterItem}
        onClick={onDelete}
      >
        <XIcon className="size-5 shrink-0 opacity-80" />
        {deleteLabel}
      </button>
    </div>
  );
}

export function BrowseMergeIntoPanel({
  panelRef,
  x,
  y,
  query,
  onQueryChange,
  targets,
  saving,
  onSelect,
}: {
  panelRef: RefObject<HTMLDivElement | null>;
  x: number;
  y: number;
  query: string;
  onQueryChange: (query: string) => void;
  targets: ContactListItem[];
  saving: boolean;
  onSelect: (id: number) => void;
}) {
  return (
    <div
      ref={panelRef}
      className="fixed z-[100] w-72 rounded-lg border border-border bg-popover shadow-xl"
      style={{ left: x, top: y }}
    >
      <div className="border-b border-border px-3 py-2 text-[12px] font-semibold text-text">
        Merge into contact
      </div>
      <input
        autoFocus
        value={query}
        onChange={(e) => onQueryChange(e.target.value)}
        placeholder="Search named contacts…"
        className="w-full border-b border-border bg-transparent px-3 py-2 text-[13px] text-text outline-none placeholder:text-muted"
      />
      <div className="max-h-64 overflow-y-auto py-1">
        {targets.length === 0 ? (
          <p className="px-3 py-2 text-[12px] text-muted">No matches</p>
        ) : (
          targets.map((c) => (
            <button
              key={c.id}
              type="button"
              disabled={saving}
              onClick={() => onSelect(c.id)}
              className="flex w-full flex-col px-3 py-1.5 text-left hover:bg-hover disabled:opacity-40"
            >
              <span className="truncate text-[13px] text-text">
                {c.displayName}
              </span>
              {c.preferredHandle && (
                <span className="truncate text-[11px] text-muted">
                  {formatPhoneDisplay(c.preferredHandle)}
                </span>
              )}
            </button>
          ))
        )}
      </div>
    </div>
  );
}
