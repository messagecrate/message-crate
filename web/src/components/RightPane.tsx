import type { ReactNode } from "react";
import { LIST_TOOLBAR_CLASS } from "./ListRangeHeader";
import { useRightToolbar } from "./useRightToolbar";

/** The narrowest the right pane gets: a narrower window scrolls the row under the header sideways (#1722). */
export const RIGHT_PANE_MIN_WIDTH = 320;

/** Remaining width: toolbar row, then the drawer, selection list, or placeholder. */
export default function RightPane({ children }: { children: ReactNode }) {
  const { toolbar } = useRightToolbar();
  return (
    <div
      data-right-pane
      style={{ minWidth: `${RIGHT_PANE_MIN_WIDTH}px` }}
      className="flex min-h-0 flex-1 shrink-0 flex-col bg-bg"
    >
      <div className={LIST_TOOLBAR_CLASS}>{toolbar}</div>
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div>
    </div>
  );
}
