import { dataCardBodyCellClass, dataCardHeaderCellClass } from "../DataCard";

export const thClass = `${dataCardHeaderCellClass} !px-0`;
export const tdClass = dataCardBodyCellClass;
export const tdCenterClass = tdClass;
export const tdLeftClass = `${tdClass} !px-1 !text-left overflow-hidden`;
export const tdRightClass = `${tdClass} text-right`;
/** Tight left pad for left-aligned headers. */
export const thLeftClass = `${thClass} !px-1 !pr-0 !text-left overflow-hidden`;
export const thRightClass = `${thClass} text-right`;
export const mutedClass = "text-[0.813rem] leading-snug text-muted";
/** Trash: show on row hover; on keyboard, when the button itself is focus-visible.
 * Avoid row focus-within — table row focus after click would leave trash stuck on. */
export const rowActionsRevealClass =
  "opacity-100 [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/handle-row:opacity-100 [@media(hover:hover)]:group-data-hovered/handle-row:opacity-100 [@media(hover:hover)]:has-[:focus-visible]:opacity-100";
