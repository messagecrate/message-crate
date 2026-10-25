import { type ReactNode, useCallback, useMemo, useRef, useState } from "react";
import { useAuth } from "../hooks/useAuth";
import { useResetOnChange } from "../hooks/useResetOnChange";
import { useRouteMutation } from "../hooks/useRouteQuery";
import { useSetContactGroupMembers } from "../hooks/useSetContactGroupMembers";
import { errorText } from "../lib/apiErrorMessage";
import {
  actionText,
  EMPTY_HISTORY,
  type GroupChange,
  groupFailureText,
  groupOp,
  type HistoryAction,
  type HistoryContact,
  type HistoryDirection,
  type HistoryEntry,
  type HistoryEvent,
  type HistoryStacks,
  historyReducer,
  type RefusedGroupChange,
  type TrashMove,
  trashFailureText,
  trashMove,
  undoneText,
} from "../lib/contactHistory";
import { TRASHED_CONTACT_IDS_MAX, updateTrashedContacts } from "../lib/serverApi";
import ContactHistoryToast from "./ContactHistoryToast";
import {
  type ContactHistory,
  ContactHistoryContext,
  type HistoryToast,
} from "./contactHistoryState";

/** What one run of an entry's requests did. */
type RunOutcome = {
  /** The part of the action that went through, or null when none of it did. */
  done: HistoryAction | null;
  /** What the error banner says about the requests that failed, or null when none did. */
  failure: string | null;
};

type RunVars = { action: HistoryAction; direction: HistoryDirection };

/**
 * Send one Trash move and answer the contacts it moved. All of them go in
 * one request, which the Audit Trail records as one action; only a move of
 * more than the server takes at once is sent in parts. The server answers
 * the contacts that are in the Trash after a move there, those already there
 * included, and the contacts it took out after a restore. A contact deleted
 * since the action is in neither, so it alone is reported. A refused part
 * moved none of its contacts, and the parts after it are still sent.
 */
async function sendTrashMove(move: TrashMove): Promise<HistoryContact[]> {
  const went: HistoryContact[] = [];
  for (let start = 0; start < move.contacts.length; start += TRASHED_CONTACT_IDS_MAX) {
    const part = move.contacts.slice(start, start + TRASHED_CONTACT_IDS_MAX);
    const ids = part.map((c) => c.id);
    try {
      const answer = await updateTrashedContacts(
        move.to === "trash" ? { add: ids } : { remove: ids },
      );
      const moved = new Set(move.to === "trash" ? answer.trashed : answer.restored);
      went.push(...part.filter((c) => moved.has(c.id)));
    } catch {
      // Counted by the caller; the parts after it are still sent.
    }
  }
  return went;
}

/** The part of a Trash move's `action` that went through, or null when none did. */
function doneTrashAction(action: HistoryAction, went: HistoryContact[]): HistoryAction | null {
  if (went.length === 0) return null;
  if (action.kind === "trash") return { kind: "trash", contacts: went };
  return action;
}

/**
 * The Undo and Redo stack for actions on the Contacts list, and the toast
 * that follows each one (#3234).
 *
 * It sits above the routes, so the stack survives opening a conversation and
 * coming back. It lives in memory only, so a reload forgets it, and it starts
 * again when the logged-in account changes. The AuthGuard above it unmounts
 * it on logout.
 *
 * An undo sends the requests a person would send to reverse the action, one
 * by one, and keeps going past a refusal: the error banner then says what
 * went through and what did not, and the entry leaves the stack.
 */
export default function ContactHistoryProvider({ children }: { children: ReactNode }) {
  const { accountId } = useAuth();
  const [stacks, setStacks] = useState<HistoryStacks>(EMPTY_HISTORY);
  const [toast, setToast] = useState<HistoryToast | null>(null);
  const [report, setReport] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const nextId = useRef(1);
  /** Changes with the account and on `clear`, so a run that settles after either records nothing. */
  const generation = useRef(0);
  /** Set while a run is in flight; a second press while it runs does nothing. */
  const running = useRef(false);
  /**
   * The stacks as of the last event, ahead of the render that shows them, so
   * a key repeat or a second press that arrives before that render still
   * reads the entry the first one left on top.
   */
  const stacksRef = useRef<HistoryStacks>(EMPTY_HISTORY);
  /** Counts pushes, so a run can tell that a new action landed while it was in flight. */
  const pushes = useRef(0);
  const setGroupMembers = useSetContactGroupMembers();

  useResetOnChange([accountId], () => {
    generation.current += 1;
    stacksRef.current = EMPTY_HISTORY;
    setStacks(EMPTY_HISTORY);
    setToast(null);
    setReport(null);
  });

  const apply = useCallback((event: HistoryEvent) => {
    if (event.type === "push") pushes.current += 1;
    stacksRef.current = historyReducer(stacksRef.current, event);
    setStacks(stacksRef.current);
  }, []);

  const showToast = useCallback((text: string, undoable: boolean) => {
    const id = nextId.current++;
    setToast({ id, text, undoable });
  }, []);

  // One mutation for a whole run, so the account's cache is marked stale once
  // the last request settles: the list, the Trash and the Contact Drawer then
  // show what the run did.
  const runMutation = useRouteMutation<RunOutcome, Error, RunVars>({
    mutationFn: async ({ action, direction }) => {
      const move = trashMove(action, direction);
      if (move) {
        const went = await sendTrashMove(move);
        const failed = move.contacts.length - went.length;
        return {
          done: doneTrashAction(action, went),
          failure: failed > 0 ? trashFailureText(move.to, went.length, failed) : null,
        };
      }
      if (action.kind !== "group") return { done: null, failure: null };
      const op = groupOp(action, direction);
      const went: GroupChange[] = [];
      const refused: RefusedGroupChange[] = [];
      for (const change of action.changes) {
        const ids = change.contacts.map((c) => c.id);
        try {
          await setGroupMembers.mutateAsync({
            name: change.group,
            patch: op === "add" ? { add: ids } : { remove: ids },
          });
          went.push(change);
        } catch (err) {
          // Named in the banner with its reason; the rest are still sent.
          refused.push({ ...change, reason: errorText(err) });
        }
      }
      return {
        done: went.length > 0 ? { ...action, changes: went } : null,
        failure: refused.length > 0 ? groupFailureText(op, went, refused) : null,
      };
    },
  });
  const run = runMutation.mutateAsync;

  /** Run `work` unless a run is already in flight, with `busy` set while it runs. */
  const exclusive = useCallback(async (work: (gen: number) => Promise<void>) => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setReport(null);
    try {
      await work(generation.current);
    } finally {
      running.current = false;
      setBusy(false);
    }
  }, []);

  const push = useCallback(
    (action: HistoryAction) => {
      apply({ type: "push", entry: { id: nextId.current++, action } });
      setReport(null);
      showToast(actionText(action), true);
    },
    [apply, showToast],
  );

  const deleteContacts = useCallback(
    (contacts: HistoryContact[]) =>
      exclusive(async (gen) => {
        const action: HistoryAction = { kind: "trash", contacts };
        const outcome = await run({ action, direction: "do" });
        if (gen !== generation.current) return;
        if (outcome.done) {
          apply({ type: "push", entry: { id: nextId.current++, action: outcome.done } });
          showToast(actionText(outcome.done), true);
        }
        if (outcome.failure !== null) setReport(outcome.failure);
      }),
    [apply, exclusive, run, showToast],
  );

  const undoEntry = stacks.undo.at(-1) ?? null;
  const redoEntry = stacks.redo.at(-1) ?? null;

  /**
   * Undo the entry on top of the undo stack, or redo the one on top of the
   * redo stack. A run that went through moves the entry to the other stack.
   * An undo that a new action overtook leaves both stacks instead, because
   * the linear model has nothing to redo after a new action. A run that
   * failed in part drops the entry and says what happened in the banner.
   */
  const step = useCallback(
    (from: "undo" | "redo") =>
      exclusive(async (gen) => {
        const entry: HistoryEntry | undefined = stacksRef.current[from].at(-1);
        if (!entry) return;
        const { action } = entry;
        const direction = from === "undo" ? "undo" : "do";
        const pushesBefore = pushes.current;
        const outcome = await run({ action, direction });
        if (gen !== generation.current) return;
        if (outcome.failure === null) {
          const overtaken = pushes.current !== pushesBefore;
          if (from === "redo") apply({ type: "redone", entry });
          // An undo that a new action overtook leaves nothing to redo.
          else apply(overtaken ? { type: "drop", id: entry.id } : { type: "undone", id: entry.id });
          // The newer action's toast, with its Undo, stays.
          if (!overtaken) {
            showToast(from === "undo" ? undoneText(action) : actionText(action), false);
          }
          return;
        }
        apply({ type: "drop", id: entry.id });
        setToast(null);
        setReport(outcome.failure);
      }),
    [apply, exclusive, run, showToast],
  );
  const undo = useCallback(() => step("undo"), [step]);
  const redo = useCallback(() => step("redo"), [step]);

  const clear = useCallback(() => {
    // A run in flight records nothing once it settles: what it would record
    // may lean on what the delete for good just removed.
    generation.current += 1;
    apply({ type: "clear" });
    setToast(null);
  }, [apply]);

  const dismissToast = useCallback(() => setToast(null), []);

  const value = useMemo<ContactHistory>(
    () => ({
      undoEntry,
      redoEntry,
      busy,
      toast,
      report,
      push,
      deleteContacts,
      undo,
      redo,
      clear,
      dismissToast,
    }),
    [
      undoEntry,
      redoEntry,
      busy,
      toast,
      report,
      push,
      deleteContacts,
      undo,
      redo,
      clear,
      dismissToast,
    ],
  );

  return (
    <ContactHistoryContext.Provider value={value}>
      {children}
      <ContactHistoryToast />
    </ContactHistoryContext.Provider>
  );
}
