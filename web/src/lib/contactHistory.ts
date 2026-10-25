/**
 * Undo and Redo for actions on the Contacts list: what an entry holds, the
 * two stacks, and the words the toast, the menu and the error banner use.
 *
 * Nothing here sends a request or holds state. `ContactHistoryProvider`
 * keeps the stacks with `historyReducer` and sends each entry's requests.
 * An undo is the request a person would send to reverse the action, because
 * the server has no undo route (#3234).
 */

/** A contact as an entry records it: its id for the request, its label for the words. */
export type HistoryContact = { id: number; label: string };

/** One Contact Group and the contacts an entry put in it or took out of it. */
export type GroupChange = { group: string; contacts: HistoryContact[] };

/** An action on the Contacts list that Undo can reverse. */
export type HistoryAction =
  /** Delete Contacts: the contacts were moved to the Trash. */
  | { kind: "trash"; contacts: HistoryContact[] }
  /**
   * A Contact Group membership change. One change for a toggle on one group;
   * several when one press took the contacts out of several groups.
   */
  | { kind: "group"; op: "add" | "remove"; changes: GroupChange[] };

export type HistoryEntry = { id: number; action: HistoryAction };

/** At most this many entries are kept; a new one past it drops the oldest. */
export const HISTORY_LIMIT = 20;

export type HistoryStacks = {
  /** Newest last: Undo takes the last one. */
  undo: HistoryEntry[];
  /** Newest last: Redo takes the last one. */
  redo: HistoryEntry[];
};

export const EMPTY_HISTORY: HistoryStacks = { undo: [], redo: [] };

export type HistoryEvent =
  /** A new action: it goes on top, and whatever was undone can no longer be redone. */
  | { type: "push"; entry: HistoryEntry }
  /** The entry's undo went through: it moves to the redo stack. */
  | { type: "undone"; id: number }
  /** The entry's redo went through: it moves back to the undo stack. */
  | { type: "redone"; id: number }
  /** The entry's undo or redo failed in part: it leaves both stacks. */
  | { type: "drop"; id: number }
  /** A delete for good, or a change of account: nothing can be undone or redone. */
  | { type: "clear" };

/** The newest `HISTORY_LIMIT` entries of `list`. */
function capped(list: HistoryEntry[]): HistoryEntry[] {
  return list.slice(Math.max(0, list.length - HISTORY_LIMIT));
}

/** The stacks after one event. A linear model: a new action discards the redo stack. */
export function historyReducer(state: HistoryStacks, event: HistoryEvent): HistoryStacks {
  switch (event.type) {
    case "push": {
      return { undo: capped([...state.undo, event.entry]), redo: [] };
    }
    case "undone": {
      const entry = state.undo.find((e) => e.id === event.id);
      if (!entry) return state;
      return { undo: state.undo.filter((e) => e !== entry), redo: [...state.redo, entry] };
    }
    case "redone": {
      const entry = state.redo.find((e) => e.id === event.id);
      if (!entry) return state;
      return {
        undo: capped([...state.undo, entry]),
        redo: state.redo.filter((e) => e !== entry),
      };
    }
    case "drop":
      return {
        undo: state.undo.filter((e) => e.id !== event.id),
        redo: state.redo.filter((e) => e.id !== event.id),
      };
    case "clear":
      return EMPTY_HISTORY;
  }
}

/** The two directions an entry's requests can run in. */
export type HistoryDirection = "do" | "undo";

/** The membership request a group action sends in `direction`: its own, or the opposite. */
export function groupOp(
  action: Extract<HistoryAction, { kind: "group" }>,
  direction: HistoryDirection,
): "add" | "remove" {
  if (direction === "do") return action.op;
  return action.op === "add" ? "remove" : "add";
}

/** "Mother" for one contact, "3 contacts" for more. */
function who(contacts: readonly HistoryContact[]): string {
  if (contacts.length === 1 && contacts[0]) return contacts[0].label;
  return `${contacts.length} contacts`;
}

/** Every contact a group action touched, each once. */
function groupContacts(changes: readonly GroupChange[]): HistoryContact[] {
  const seen = new Map<number, HistoryContact>();
  for (const change of changes) for (const c of change.contacts) seen.set(c.id, c);
  return [...seen.values()];
}

/** "Family" for one group, "3 Contact Groups" for more. */
function whichGroups(changes: readonly GroupChange[]): string {
  if (changes.length === 1 && changes[0]) return changes[0].group;
  return `${changes.length} Contact Groups`;
}

function groupText(op: "add" | "remove", changes: readonly GroupChange[]): string {
  const contacts = who(groupContacts(changes));
  const groups = whichGroups(changes);
  return op === "add" ? `Added ${contacts} to ${groups}` : `Removed ${contacts} from ${groups}`;
}

/** What the toast says after the action, and after a redo of it: "Moved 3 contacts to Trash". */
export function actionText(action: HistoryAction): string {
  if (action.kind === "trash") return `Moved ${who(action.contacts)} to Trash`;
  return groupText(action.op, action.changes);
}

/** What the toast says after an undo: "Restored 3 contacts". */
export function undoneText(action: HistoryAction): string {
  if (action.kind === "trash") return `Restored ${who(action.contacts)}`;
  return groupText(groupOp(action, "undo"), action.changes);
}

/** The menu's name for the action, after "Undo" or "Redo". It carries no count. */
export function actionMenuName(action: HistoryAction): string {
  if (action.kind === "trash") return "Delete Contacts";
  return action.op === "add" ? "Add to Contact Group" : "Remove from Contact Group";
}

/**
 * What the error banner says when some of an entry's requests failed.
 * `done` and `failed` count contacts for a trash entry and groups for a
 * group entry, because a membership change is one request per group.
 */
export function partialFailureText(
  action: HistoryAction,
  direction: HistoryDirection,
  done: number,
  failed: number,
): string {
  if (action.kind === "trash") {
    const doneWords = done === 1 ? "1 contact" : `${done} contacts`;
    if (direction === "undo") {
      const verb = failed === 1 ? "is" : "are";
      return `Restored ${doneWords}. ${failed} ${verb} no longer in the Trash.`;
    }
    return `Moved ${doneWords} to Trash. ${failed} could not be moved.`;
  }
  const op = groupOp(action, direction);
  const contacts = who(groupContacts(action.changes));
  const groups = whichGroups(action.changes);
  const what = op === "add" ? `add ${contacts} to ${groups}` : `remove ${contacts} from ${groups}`;
  if (done === 0) return `Could not ${what}.`;
  return `Could not ${what}. ${failed} of ${done + failed} Contact Groups refused the change.`;
}
