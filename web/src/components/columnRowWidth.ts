import { LEFT_PANEL_DEFAULT_WIDTH } from "./leftPanelWidth";

/**
 * Widths of the three-column row on Conversations, Contacts and Trash: the
 * navigation panel, the list column and the right pane. A window narrower
 * than the row scrolls it sideways rather than squeezing a column, because
 * phone-width layouts come later (#1722).
 */

/** The narrowest the list column gets, by a drag or by a narrow window. */
export const LIST_COLUMN_MIN_WIDTH = 220;

/** The narrowest the right pane gets. */
export const RIGHT_PANE_MIN_WIDTH = 320;

/**
 * The narrowest a narrow window makes the navigation panel beside a list: its
 * default width. A narrower stored width stays as it is.
 */
export const COLUMN_ROW_LEFT_PANEL_FLOOR = LEFT_PANEL_DEFAULT_WIDTH;

/**
 * The 1 px right border the navigation panel and the list column each paint
 * outside their width.
 */
const COLUMN_BORDER_WIDTH = 1;

/** The row's minimum width, below which the page scrolls sideways. */
export const COLUMN_ROW_MIN_WIDTH =
  COLUMN_ROW_LEFT_PANEL_FLOOR +
  LIST_COLUMN_MIN_WIDTH +
  2 * COLUMN_BORDER_WIDTH +
  RIGHT_PANE_MIN_WIDTH;
