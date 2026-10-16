import { useContext } from "react";
import { ColumnResizeSetContext } from "../components/columnResizeState";

/** Stable callback for LeftPanel / ListColumn to publish drag state. */
export function useReportColumnResizing(): (dragging: boolean) => void {
  return useContext(ColumnResizeSetContext);
}
