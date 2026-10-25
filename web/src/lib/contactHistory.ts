/**
 * Undo and Redo for actions on the Contacts list: what an entry holds, the
 * two stacks, and the words the toast, the menu and the error banner use.
 *
 * Nothing here sends a request or holds state. `ContactHistoryProvider`
 * keeps the stacks with `historyReducer` and sends each entry's requests.
 * An undo is the request a person would send to reverse the action, because
 * the server has no undo route (#3234), except for Merge Contacts, whose undo
 * is the unmerge route (#3237).
 */

/** A contact as an entry records it: its id for the request, its label for the words. */
export type HistoryContact = { id: number; label: string };

/** A contact a merge took, and whether it has a name, for the words of its undo. */
export type MergedContact = HistoryContact & { named: boolean };

/** One Contact Group and the contacts an entry put in it or took out of it. */
export type GroupChange = { group: string; contacts: HistoryContact[] };

/** An action on the Contacts list that Undo can reverse. */
export type HistoryAction =
  /** Delete Contacts: the contacts were moved to the Trash. */
  | { kind: "trash"; contacts: HistoryContact[] }
  /** New Contact: the contact was added. Undo moves it to the Trash, and Redo restores it. */
  | { kind: "create"; contact: HistoryContact }
  /**
   * A Contact Group membership change. One change for a toggle on one group;
   * several when one press took the contacts out of several groups.
   */
  | { kind: "group"; op: "add" | "remove"; changes: GroupChange[] }
  /**
   * Merge Contacts: `merged` went into `target`. Only the account's most
   * recent merge can be undone, so the stacks keep one merge entry at most.
   */
  | { kind: "merge"; target: HistoryContact; merged: MergedContact[] };

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
  /**
   * The entry's undo went through: it moves to the redo stack, holding
   * `action`, what the undo did, when that was less than the whole entry.
   */
  | { type: "undone"; id: number; action?: HistoryAction }
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

/**
 * `list` without the merge entries other than `entry`, when `entry` is a
 * merge: the server keeps only the account's most recent merge, so an older
 * one can no longer be undone (#3237).
 */
function withoutOlderMerges(list: HistoryEntry[], entry: HistoryEntry): HistoryEntry[] {
  if (entry.action.kind !== "merge") return list;
  return list.filter((e) => e.action.kind !== "merge" || e.id === entry.id);
}

/** The stacks after one event. A linear model: a new action discards the redo stack. */
export function historyReducer(state: HistoryStacks, event: HistoryEvent): HistoryStacks {
  switch (event.type) {
    case "push": {
      return {
        undo: capped(withoutOlderMerges([...state.undo, event.entry], event.entry)),
        redo: [],
      };
    }
    case "undone": {
      const entry = state.undo.find((e) => e.id === event.id);
      if (!entry) return state;
      const moved = event.action ? { ...entry, action: event.action } : entry;
      return { undo: state.undo.filter((e) => e !== entry), redo: [...state.redo, moved] };
    }
    case "redone":
      return {
        undo: capped(
          withoutOlderMerges(
            [...state.undo.filter((e) => e.id !== event.entry.id), event.entry],
            event.entry,
          ),
        ),
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

/** An action that moves contacts to or out of the Trash, and the contacts it moves. */
export type TrashMove = { to: "trash" | "restore"; contacts: HistoryContact[] };

/**
 * The Trash move an action makes in `direction`, or null for a Contact Group
 * change. Delete Contacts moves its contacts to the Trash and its undo takes
 * them out; New Contact's undo moves the new contact to the Trash and its
 * redo takes it out.
 */
export function trashMove(action: HistoryAction, direction: HistoryDirection): TrashMove | null {
  if (action.kind === "trash") {
    return { to: direction === "do" ? "trash" : "restore", contacts: action.contacts };
  }
  if (action.kind === "create") {
    return { to: direction === "do" ? "restore" : "trash", contacts: [action.contact] };
  }
  return null;
}

/**
 * The contacts a merge took, as an undo names them: the named ones as `who`
 * does, then the ones with no name counted, "Nick Koch and 1 contact with no
 * name".
 */
function unmergedWho(merged: readonly MergedContact[]): string {
  const named = merged.filter((c) => c.named);
  const nameless = merged.length - named.length;
  const parts: string[] = [];
  if (named.length > 0) parts.push(who(named));
  if (nameless > 0) {
    parts.push(nameless === 1 ? "1 contact with no name" : `${nameless} contacts with no name`);
  }
  return parts.join(" and ");
}

/** What the toast says after the action, and after a redo of it: "Moved 3 contacts to Trash". */
export function actionText(action: HistoryAction): string {
  if (action.kind === "trash") return `Moved ${who(action.contacts)} to Trash`;
  if (action.kind === "create") return `Added ${action.contact.label}`;
  if (action.kind === "merge") {
    return `Merged ${action.merged.length + 1} contacts into ${action.target.label}`;
  }
  return groupText(action.op, action.changes);
}

/** What the toast says after an undo: "Restored 3 contacts". */
export function undoneText(action: HistoryAction): string {
  if (action.kind === "trash") return `Restored ${who(action.contacts)}`;
  if (action.kind === "create") return `Moved ${action.contact.label} to Trash`;
  if (action.kind === "merge") return `Unmerged ${unmergedWho(action.merged)}`;
  return groupText(groupOp(action, "undo"), action.changes);
}

/** The menu's name for the action, after "Undo" or "Redo". It carries no count. */
export function actionMenuName(action: HistoryAction): string {
  if (action.kind === "trash") return "Delete Contacts";
  if (action.kind === "create") return "New Contact";
  if (action.kind === "merge") return "Merge Contacts";
  return action.op === "add" ? "Add to Contact Group" : "Remove from Contact Group";
}

/**
 * What an undo of a merge adds when the server left contacts out: a contact
 * with no name whose identities all left the merged contact since the merge.
 */
export function notRestoredText(n: number): string {
  return n === 1
    ? "1 contact with no name had no identity left and was not brought back"
    : `${n} contacts with no name had no identity left and were not brought back`;
}

/**
 * What the error banner says when a merge's request was refused: its undo
 * (the unmerge) or its redo (the merge sent again), ending in the reason.
 */
export function mergeFailureText(direction: HistoryDirection, reason: string): string {
  return direction === "undo"
    ? `Could not unmerge the contacts: ${reason}`
    : `Could not merge the contacts again: ${reason}`;
}

/**
 * What the error banner says when some of a Trash move's contacts did not
 * move. `done` and `failed` count contacts.
 */
export function trashFailureText(to: TrashMove["to"], done: number, failed: number): string {
  const doneWords = done === 1 ? "1 contact" : `${done} contacts`;
  if (to === "restore") {
    const verb = failed === 1 ? "is" : "are";
    const missing = `${failed} ${verb} no longer in the Trash.`;
    return done === 0 ? missing : `Restored ${doneWords}. ${missing}`;
  }
  if (done === 0) {
    return failed === 1
      ? "Could not move 1 contact to Trash."
      : `Could not move ${failed} contacts to Trash.`;
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
