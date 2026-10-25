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
  /**
   * The entry's redo went through: it goes back on top of the undo stack,
   * even when a new action meanwhile cleared the redo stack it came from,
   * because the redo was an action too.
   */
  | { type: "redone"; entry: HistoryEntry }
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
    case "redone":
      return {
        undo: capped([...state.undo.filter((e) => e.id !== event.entry.id), event.entry]),
        redo: state.redo.filter((e) => e.id !== event.entry.id),
      };
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
 * What the error banner says when some of a trash entry's requests failed.
 * `done` and `failed` count contacts.
 */
export function trashFailureText(
  direction: HistoryDirection,
  done: number,
  failed: number,
): string {
  const doneWords = done === 1 ? "1 contact" : `${done} contacts`;
  if (direction === "undo") {
    const verb = failed === 1 ? "is" : "are";
    return `Restored ${doneWords}. ${failed} ${verb} no longer in the Trash.`;
  }
  return `Moved ${doneWords} to Trash. ${failed} could not be moved.`;
}

/** A Contact Group whose membership request was refused, and the reason given for it. */
export type RefusedGroupChange = GroupChange & { reason: string };

/**
 * What the error banner says when some of a group entry's requests failed:
 * one line per refused Contact Group, ending in the reason it was refused,
 * so a Contact Group deleted meanwhile and a lost connection read
 * differently (#3252). A first line counts the Contact Groups that went
 * through, when any did. `op` is the request that was sent, already turned
 * around for an undo (`groupOp`).
 */
export function groupFailureText(
  op: "add" | "remove",
  done: readonly GroupChange[],
  refused: readonly RefusedGroupChange[],
): string {
  const lines: string[] = [];
  if (done.length > 0) {
    const contacts = who(groupContacts(done));
    const groups = `${done.length} of ${done.length + refused.length} Contact Groups`;
    lines.push(
      op === "add" ? `Added ${contacts} to ${groups}.` : `Removed ${contacts} from ${groups}.`,
    );
  }
  for (const change of refused) {
    const contacts = who(change.contacts);
    const what =
      op === "add"
        ? `add ${contacts} to ${change.group}`
        : `remove ${contacts} from ${change.group}`;
    lines.push(`Could not ${what}: ${change.reason}`);
  }
  return lines.join("\n");
}
