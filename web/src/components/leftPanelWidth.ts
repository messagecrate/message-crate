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
 * The widest the navigation panel may be in a window `windowWidth` pixels
 * wide: `LEFT_PANEL_MAX_WIDTH`, or the window's share when that is less, and
 * never below `LEFT_PANEL_MIN_WIDTH`.
 */
export function leftPanelMaxWidth(windowWidth: number): number {
  return Math.max(
    LEFT_PANEL_MIN_WIDTH,
    Math.min(LEFT_PANEL_MAX_WIDTH, Math.floor(windowWidth * LEFT_PANEL_WINDOW_SHARE)),
  );
}
