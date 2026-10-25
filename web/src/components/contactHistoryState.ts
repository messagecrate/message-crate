import { createContext } from "react";
import type {
  HistoryAction,
  HistoryContact,
  HistoryEntry,
  MergedContact,
} from "../lib/contactHistory";

/** The toast after an action, an undo or a redo. A new `id` restarts its timer. */
export type HistoryToast = {
  id: number;
  text: string;
  /** Only the toast right after an action offers Undo. */
  undoable: boolean;
};

/** What `ContactHistoryProvider` gives the Contacts list, its menu and the toast. */
export type ContactHistory = {
  /** The entry Undo would reverse, or null when there is none. */
  undoEntry: HistoryEntry | null;
  /** The entry Redo would send again, or null until something was undone. */
  redoEntry: HistoryEntry | null;
  /** An undo, a redo or Delete Contacts is sending its requests. */
  busy: boolean;
  toast: HistoryToast | null;
  /** What the last undo, redo or Delete Contacts could not do, for the list's error banner. */
  report: string | null;
  /** Record an action the Contacts list already sent, and show its toast. */
  push: (action: HistoryAction) => void;
  /** Move these contacts to the Trash one by one, and record the ones that went. */
  deleteContacts: (contacts: HistoryContact[]) => Promise<void>;
  /**
   * Merge `merged` into `target`, and record the merge when it went through.
   * Answers what the server refused it with, or null when it went through or
   * another run was already in flight.
   */
  mergeContacts: (target: HistoryContact, merged: MergedContact[]) => Promise<Error | null>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  /** Forget every entry, as a delete for good does. */
  clear: () => void;
  dismissToast: () => void;
};

/** Set by `ContactHistoryProvider`. */
export const ContactHistoryContext = createContext<ContactHistory | null>(null);
