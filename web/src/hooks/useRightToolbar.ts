import { useContext } from "react";
import { RightToolbarContext } from "../components/rightToolbarState";

export function useRightToolbar() {
  return useContext(RightToolbarContext);
}

/** Register the groups or tags menu for the right-pane toolbar. */
export function useSetRightToolbar() {
  return useRightToolbar().setToolbar;
}
