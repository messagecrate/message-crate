"use client";

import type { ContactListItem } from "@/lib/types";
import {
  useRef,
  useState,
  type ChangeEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { BrowseContactRow } from "./BrowseContactRow";
import { ListHistoryMenu, type ListHistoryMenuItem } from "./history";
import { IconHoverTarget } from "./IconHoverLabel";
import { PencilIcon, XIcon } from "./icons";
import { PaneSearchField } from "./PaneSearchField";
import { SortByMenu, type SortMode, type SortOrder } from "./SortByMenu";

export function BrowseContactList({
  sectionLabel,
  selectAllRef,
  allGroupSelected,
  visibleCount,
  sortedCount,
  query,
  onQueryChange,
  onToggleSelectAll,
  onNewContact,
  onImportVcf,
  labelsMenu,
  onEdit,
  editDisabled = false,
  onTrashContact,
  deleteDisabled = false,
  sort,
  sortOrder,
  onSortChange,
  grouped,
  contactId,
  contextMenuId = null,
  selectedIds,
  onSelectColumnClick,
  onNamePhoneClick,
  onContextMenu,
}: {
  sectionLabel: string;
  selectAllRef: RefObject<HTMLInputElement | null>;
  allGroupSelected: boolean;
  visibleCount: number;
  sortedCount: number;
  query: string;
  onQueryChange: (q: string) => void;
  onToggleSelectAll: () => void;
  onNewContact: (anchorEl: HTMLElement) => void;
  /** Upload a .vcf and import contacts (Contacts section). */
  onImportVcf?: (file: File) => Promise<void>;
  /** Icon-only LabelsMenu element rendered first in the toolbar cluster. */
  labelsMenu?: ReactNode;
  onEdit?: (anchorEl: HTMLElement) => void;
  editDisabled?: boolean;
  onTrashContact?: () => void;
  deleteDisabled?: boolean;
  sort: SortMode;
  sortOrder: SortOrder;
  onSortChange: (next: { sort: SortMode; order: SortOrder }) => void;
  grouped: [string, ContactListItem[]][];
  contactId: number | null;
  /** Right-clicked contact while its context menu is open. */
  contextMenuId?: number | null;
  selectedIds: Set<number>;
  onSelectColumnClick: (id: number, e: MouseEvent) => void;
  onNamePhoneClick: (
    id: number,
    e: MouseEvent | { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean },
  ) => void;
  onContextMenu: (id: number, x: number, y: number) => void;
}) {
  const vcfInputRef = useRef<HTMLInputElement>(null);
  const [vcfImporting, setVcfImporting] = useState(false);
  const onVcfPicked = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file || !onImportVcf) return;
    setVcfImporting(true);
    try {
      await onImportVcf(file);
    } finally {
      setVcfImporting(false);
    }
  };

  const menuItems: ListHistoryMenuItem[] = [
    {
      key: "new-contact",
      label: "New",
      icon: <NewContactIcon className="size-5 shrink-0 opacity-80" />,
      onClick: (triggerEl) => {
        if (triggerEl) onNewContact(triggerEl);
      },
    },
    ...(onImportVcf
      ? [
          {
            key: "import-vcf",
            label: vcfImporting ? "Importing…" : "Import VCF",
            icon: <ImportVcfIcon className="size-5 shrink-0 opacity-80" />,
            disabled: vcfImporting,
            onClick: () => {
              vcfInputRef.current?.click();
            },
          } satisfies ListHistoryMenuItem,
        ]
      : []),
    ...(onEdit
      ? [
          {
            key: "edit",
            label: "Edit",
            icon: <PencilIcon className="size-5 shrink-0 opacity-80" />,
            disabled: editDisabled,
            onClick: (triggerEl) => {
              if (triggerEl) onEdit(triggerEl);
            },
          } satisfies ListHistoryMenuItem,
        ]
      : []),
    ...(onTrashContact
      ? [
          {
            key: "delete",
            label:
              selectedIds.size > 1 ? "Delete contacts" : "Delete contact",
            icon: <XIcon className="size-5 shrink-0 opacity-80" />,
            disabled: deleteDisabled,
            danger: true,
            onClick: () => onTrashContact(),
          } satisfies ListHistoryMenuItem,
        ]
      : []),
  ];

  return (
    <aside className="flex h-full min-h-0 w-full flex-col bg-sidebar">
      {onImportVcf && (
        <input
          ref={vcfInputRef}
          type="file"
          accept=".vcf,.vcard,text/vcard,text/x-vcard"
          className="hidden"
          onChange={(e) => void onVcfPicked(e)}
        />
      )}

      <div className="flex h-[45px] shrink-0 items-center border-b border-border px-3">
        <PaneSearchField
          value={query}
          onChange={onQueryChange}
          placeholder="Filter contacts"
        />
      </div>
      <div className="flex h-[45px] shrink-0 items-center justify-between overflow-visible border-b border-border px-3">
        <label className="flex min-w-0 items-center gap-2">
          <IconHoverTarget label="Select all" placement="bottom">
            <input
              ref={selectAllRef}
              type="checkbox"
              checked={allGroupSelected}
              disabled={visibleCount === 0}
              aria-label={`Select all ${sectionLabel}`}
              onChange={onToggleSelectAll}
              className="checkbox-list"
            />
          </IconHoverTarget>
          <span className="truncate text-[13px] text-muted tabular-nums">
            {selectedIds.size > 0 ? selectedIds.size : ""}
          </span>
        </label>
        <div className="flex shrink-0 items-center gap-1.5 overflow-visible">
          {labelsMenu}
          <SortByMenu sort={sort} order={sortOrder} onChange={onSortChange} />
          <ListHistoryMenu items={menuItems} />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable]">
        {sortedCount === 0 && (
          <p className="px-3 py-4 text-[12px] text-muted">No matches</p>
        )}
        {grouped.map(([letter, items]) => (
          <div key={letter || "all"}>
            {!query.trim() && letter && (
              <div className="sticky top-0 z-10 border-b border-border bg-sidebar px-3 py-1 text-[11px] font-semibold text-muted">
                {letter}
              </div>
            )}
            {items.map((c, i) => {
              const menuTarget =
                contextMenuId != null && c.id === contextMenuId;
              const active = c.id === contactId || menuTarget;
              const checked = selectedIds.has(c.id);
              const selectionActive = selectedIds.size >= 1;
              return (
                <BrowseContactRow
                  key={c.id}
                  contact={c}
                  active={active}
                  checked={checked}
                  selectionActive={selectionActive}
                  showInsetDivider={i < items.length - 1}
                  onSelectColumnClick={onSelectColumnClick}
                  onNamePhoneClick={onNamePhoneClick}
                  onContextMenu={onContextMenu}
                />
              );
            })}
          </div>
        ))}
      </div>
    </aside>
  );
}

export function NewContactIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <circle cx="7.25" cy="8" r="3" />
      <path d="M2.25 19.25c.65-3 2.85-4.75 5-4.75s4.35 1.75 5 4.75" />
      <path d="M19 9v6M16 12h6" strokeWidth="2" />
    </svg>
  );
}

function ImportVcfIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M5 19h14" />
    </svg>
  );
}
