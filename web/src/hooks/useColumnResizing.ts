import { useContext } from "react";
import { ColumnResizeStateContext } from "../components/columnResizeState";

/** True while any column width (nav or list) is being dragged. */
export function useColumnResizing(): boolean {
  return useContext(ColumnResizeStateContext);
}
