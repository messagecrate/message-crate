/**
 * Shared left-nav section layout: label column + trailing glyph column.
 * The trailing column is 1.5rem so it matches NavGlyphButton `size-6` (24px).
 */
export const NAV_SECTION_GRID_CLASS = "grid w-full grid-cols-[minmax(0,1fr)_1.5rem] items-center";

/** Fixed 15px slot for heading chevrons and row leading icons. */
export const NAV_LEADING_GLYPH_CLASS = "flex size-[15px] shrink-0 items-center justify-center";

/** Label row: 15px leading slot, then `gap-2`, then the title. */
export const NAV_LEADING_ROW_CLASS = "flex min-w-0 items-center gap-2";

/** Nested icon row: indent so the icon lines up with the section title. */
export const NAV_NESTED_ROW_CLASS = `${NAV_LEADING_ROW_CLASS} self-stretch pl-[calc(15px+0.5rem)]`;

/** Group / tag row shell: shared grid + hover/active fill. */
export function navGlyphRowClass(active: boolean): string {
  return `group relative box-border ${NAV_SECTION_GRID_CLASS} min-h-7 rounded border-none px-0 py-0.5 text-left text-[0.875rem] text-text hover:bg-hover ${
    active ? "bg-hover font-semibold" : "bg-transparent font-normal"
  }`;
}

/** A row shell that is itself the button, such as Import or **No Contact Group**. */
export function navGlyphButtonRowClass(active: boolean): string {
  return `${navGlyphRowClass(active)} cursor-pointer`;
}
