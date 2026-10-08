import type { ReactNode } from "react";
import { RIGHT_PANE_MIN_WIDTH } from "./columnRowWidth";
import { LIST_TOOLBAR_CLASS } from "./ListRangeHeader";
import { useRightToolbar } from "./useRightToolbar";

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
