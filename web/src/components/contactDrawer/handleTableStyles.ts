import { dataCardBodyCellClass, dataCardHeaderCellClass } from "../DataCard";

export const thClass = `${dataCardHeaderCellClass} !px-0`;
export const tdClass = dataCardBodyCellClass;
export const tdCenterClass = tdClass;
export const tdRightClass = `${tdClass} text-right`;
/** Tight left pad for left-aligned headers. */
export const thLeftClass = `${thClass} !px-1 !pr-0 !text-left overflow-hidden`;
export const thRightClass = `${thClass} text-right`;
export const mutedClass = "text-[0.813rem] leading-snug text-muted";
