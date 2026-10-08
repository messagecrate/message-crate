import type { ReactNode } from "react";
import ColumnResizeHandle from "./ColumnResizeHandle";
import { useReportColumnResizing } from "./columnResizeState";
import { LIST_COLUMN_MIN_WIDTH } from "./columnRowWidth";
import { useColumnResize } from "./useColumnResize";

const DEFAULT_WIDTH = 300;
const MAX_WIDTH = 560;
const STORAGE_KEY = "listColumnWidth:v1";

export default function ListColumn({ children }: { children: ReactNode }) {
  const onDraggingChange = useReportColumnResizing();
  const { width, dragging, handleHover, handleProps } = useColumnResize({
    storageKey: STORAGE_KEY,
    defaultWidth: DEFAULT_WIDTH,
    minWidth: LIST_COLUMN_MIN_WIDTH,
    maxWidth: MAX_WIDTH,
    onDraggingChange,
  });

  return (
    <div
      data-list-column
      style={{
        // Shrinks toward its minimum in a narrow window, and no further: the
        // page scrolls sideways below that (#1722).
        flex: `0 1 ${width}px`,
        minWidth: `${LIST_COLUMN_MIN_WIDTH}px`,
        maxWidth: `${width}px`,
        width: `${width}px`,
      }}
      className="relative flex h-full flex-col overflow-hidden border-r border-border bg-panel text-text"
    >
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div>

      <ColumnResizeHandle
        ariaLabel="Resize list column"
        width={width}
        minWidth={LIST_COLUMN_MIN_WIDTH}
        maxWidth={MAX_WIDTH}
        dragging={dragging}
        handleHover={handleHover}
        handleProps={handleProps}
      />
    </div>
  );
}
