import { COLUMN_DIVIDER_WIDTH } from "./columnDivider";

/** Shared nav-column width bounds and persistence key (LeftPanel + AppHeader). */

export const LEFT_PANEL_DEFAULT_WIDTH = 220;
export const LEFT_PANEL_MIN_WIDTH = 160;
export const LEFT_PANEL_MAX_WIDTH = 520;
export const LEFT_PANEL_STORAGE_KEY = "leftPanelWidth:v1";

/** CSS custom property kept in sync with the nav width for the header brand slot. */
export const LEFT_PANEL_WIDTH_VAR = "--left-panel-width";

/**
 * The navigation panel takes at most this share of the window, so a
 * phone-width window keeps room for the page beside it (#1718).
 */
export const LEFT_PANEL_WINDOW_SHARE = 0.5;

/**
 * The widest a window `windowWidth` pixels wide lets the navigation panel be,
 * never below `LEFT_PANEL_MIN_WIDTH`. A width is shown as the smaller of this
 * and the stored width, which `LEFT_PANEL_MAX_WIDTH` already bounds.
 *
 * On a screen with no list the panel takes at most the window's share, so
 * Settings keeps room at phone width (#1718). Beside a list it takes what the
 * columns beside it leave, `besideMinWidth` and its own divider, so a panel
 * stored wide gives way before the row under the header scrolls sideways
 * (#1722).
 */
export function leftPanelWindowMaxWidth(windowWidth: number, besideMinWidth?: number): number {
  const room =
    besideMinWidth === undefined
      ? Math.floor(windowWidth * LEFT_PANEL_WINDOW_SHARE)
      : windowWidth - besideMinWidth - COLUMN_DIVIDER_WIDTH;
  return Math.max(LEFT_PANEL_MIN_WIDTH, room);
}
