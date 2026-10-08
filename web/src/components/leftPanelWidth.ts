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
 * The widest a window `windowWidth` pixels wide lets the navigation panel be:
 * the window's share, never below `floor`. A width is shown as the smaller of
 * this and the stored width, which `LEFT_PANEL_MAX_WIDTH` already bounds.
 * The floor is `LEFT_PANEL_MIN_WIDTH` on a screen with no list, so Settings
 * keeps room at phone width (#1718), and `COLUMN_ROW_LEFT_PANEL_FLOOR` beside
 * a list, where the page scrolls sideways instead (#1722).
 */
export function leftPanelWindowMaxWidth(
  windowWidth: number,
  floor: number = LEFT_PANEL_MIN_WIDTH,
): number {
  return Math.max(floor, Math.floor(windowWidth * LEFT_PANEL_WINDOW_SHARE));
}
