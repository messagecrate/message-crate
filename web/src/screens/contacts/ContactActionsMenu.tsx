import { Header, Menu, MenuItem, MenuSection, MenuTrigger, Popover } from "react-aria-components";
import {
  EllipsisIcon,
  ExportIcon,
  MergeIcon,
  RedoIcon,
  TrashIcon,
  UndoIcon,
} from "../../components/icons";
import PlainButton from "../../components/PlainButton";
import { useContactHistory } from "../../hooks/useContactHistory";
import { actionMenuName } from "../../lib/contactHistory";
import { focusRingClass, menuItemClass, menuPopoverClass } from "../../lib/uiStyles";

const sectionHeaderClass = "px-3 pb-1.5 text-[0.75rem] font-semibold text-text";

/**
 * The ⋯ menu beside the Contacts list's sort button (#3244): Export Contacts
 * and Delete Contacts under a header that names what they act on, then Undo
 * and Redo (#3234).
 *
 * Delete Contacts moves the checked contacts to the Trash, where Restore
 * brings them back, so nothing on this menu deletes for good. Merge Contacts
 * (#3237) makes the checked contacts one; Undo takes it back until the next
 * merge.
 */
export default function ContactActionsMenu({
  scope,
  canDelete,
  exporting,
  onExport,
  onDelete,
  canMerge,
  mergeHint,
  onMerge,
}: {
  /** "2 selected", "This list" or "All contacts". */
  scope: string;
  /** Some rows are checked. */
  canDelete: boolean;
  exporting: boolean;
  onExport: () => void;
  onDelete: () => void;
  /** Two to ten rows are checked, and the account may delete. */
  canMerge: boolean;
  /** Why Merge Contacts is off when the account may not merge, shown on hover. */
  mergeHint?: string;
  onMerge: () => void;
}) {
  const history = useContactHistory();
  const undoLabel = history.undoEntry ? `Undo ${actionMenuName(history.undoEntry.action)}` : "Undo";
  const redoLabel = history.redoEntry ? `Redo ${actionMenuName(history.redoEntry.action)}` : "Redo";

  return (
    <MenuTrigger>
      <PlainButton
        aria-label="Contact actions"
        title="Contact actions"
        className={`flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border border-border bg-elevated text-muted hover:text-text aria-expanded:text-text ${focusRingClass}`}
      >
        <EllipsisIcon size={16} />
      </PlainButton>
      <Popover
        placement="bottom end"
        offset={4}
        data-mc-overlay=""
        className={`${menuPopoverClass} min-w-[13rem] rounded-xl py-2`}
      >
        <Menu aria-label="Contact actions" shouldFocusWrap className="outline-none">
          <MenuSection>
            <Header className={sectionHeaderClass}>{scope}</Header>
            <MenuItem
              id="export"
              textValue="Export Contacts"
              isDisabled={exporting}
              onAction={onExport}
              className={`${menuItemClass} text-text`}
            >
              <ExportIcon size={14} />
              Export Contacts
            </MenuItem>
            <MenuItem
              id="merge"
              textValue="Merge Contacts"
              isDisabled={!canMerge || history.busy}
              onAction={onMerge}
              className={`${menuItemClass} text-text`}
            >
              <MergeIcon size={14} />
              <span title={mergeHint}>Merge Contacts</span>
            </MenuItem>
            <MenuItem
              id="delete"
              textValue="Delete Contacts"
              isDisabled={!canDelete}
              onAction={onDelete}
              className={`${menuItemClass} text-danger`}
            >
              <TrashIcon size={14} />
              Delete Contacts
            </MenuItem>
          </MenuSection>
          <MenuSection className="mt-1.5 block border-t border-border pt-1.5">
            <MenuItem
              id="undo"
              textValue={undoLabel}
              isDisabled={!history.undoEntry || history.busy}
              onAction={() => void history.undo()}
              className={`${menuItemClass} text-text`}
            >
              <UndoIcon size={14} />
              {undoLabel}
            </MenuItem>
            <MenuItem
              id="redo"
              textValue={redoLabel}
              isDisabled={!history.redoEntry || history.busy}
              onAction={() => void history.redo()}
              className={`${menuItemClass} text-text`}
            >
              <RedoIcon size={14} />
              {redoLabel}
            </MenuItem>
          </MenuSection>
        </Menu>
      </Popover>
    </MenuTrigger>
  );
}
