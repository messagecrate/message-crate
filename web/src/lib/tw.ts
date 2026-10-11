/** Shared Tailwind class strings that follow the theme colors in theme.css. */

/**
 * Inset hairline under a list row.
 *
 * One line per row, never a matching line on top: in a virtualized list the
 * rows are absolutely positioned, so a top line and the previous row's bottom
 * line only land on the same pixel while every measured height is exact. When
 * one is off they separate and the list shows doubled rules.
 *
 * The inset is a fixed 8px, never a percentage: a percentage is resolved
 * against the row's own width, so dragging the column width slid both ends of
 * every rule horizontally.
 */
export const listRowDividerClass =
  "relative after:pointer-events-none after:absolute after:inset-x-2 after:bottom-0 after:h-px after:bg-border";

/** Lighter/thinner hairline under each contact row (one line between neighbors). */
export const listRowDividersThinClass =
  "relative after:pointer-events-none after:absolute after:inset-x-2 after:bottom-0 after:h-px after:origin-center after:scale-y-50 after:bg-border/40";

/**
 * Right gutter for a scroll element inside a resizable panel.
 *
 * ColumnResizeHandle is an invisible strip pinned to the panel's right edge.
 * Without this gutter it lands on top of the scrollbar, which then cannot be
 * grabbed and shows the col-resize cursor everywhere. Insetting the scroller
 * moves the scrollbar clear of the handle so both stay usable.
 *
 * Every panel that renders a ColumnResizeHandle needs this on its scrolling
 * child, and the two widths must stay equal — a gutter narrower than the handle
 * leaves part of the scrollbar covered.
 */
export const resizeHandleGutterClass = "mr-2";
