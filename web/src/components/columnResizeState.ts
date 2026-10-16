import { createContext } from "react";

/** True while any column width (nav or list) is being dragged. */
export const ColumnResizeStateContext = createContext(false);

/** Report that a column drag started or ended. */
export const ColumnResizeSetContext = createContext<(dragging: boolean) => void>(() => {});
