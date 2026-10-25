import { useContext } from "react";
import { type ContactHistory, ContactHistoryContext } from "../components/contactHistoryState";

/** The Undo and Redo stack of the Contacts list. Must be called under `ContactHistoryProvider`. */
export function useContactHistory(): ContactHistory {
  const history = useContext(ContactHistoryContext);
  if (!history) throw new Error("useContactHistory must be used within ContactHistoryProvider");
  return history;
}

const NOTHING_TO_CLEAR = () => {};

/**
 * Forget every Undo and Redo entry. A delete for good calls it, because the
 * Trash is what an undo restores from, and a contact or conversation deleted
 * for good is no longer there (#3234).
 *
 * It does nothing outside `ContactHistoryProvider`, because Settings also
 * opens from Owner Home, where there is no Contacts list and no stack.
 */
export function useClearContactHistory(): () => void {
  return useContext(ContactHistoryContext)?.clear ?? NOTHING_TO_CLEAR;
}
