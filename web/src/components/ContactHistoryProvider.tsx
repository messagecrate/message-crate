import { type ReactNode, useCallback, useMemo, useRef, useState } from "react";
import { useAuth } from "../hooks/useAuth";
import { useResetOnChange } from "../hooks/useResetOnChange";
import { useRouteMutation } from "../hooks/useRouteQuery";
import { useSetContactGroupMembers } from "../hooks/useSetContactGroupMembers";
import {
  actionText,
  EMPTY_HISTORY,
  groupOp,
  type HistoryAction,
  type HistoryContact,
  type HistoryDirection,
  type HistoryEntry,
  type HistoryEvent,
  type HistoryStacks,
  historyReducer,
  partialFailureText,
  undoneText,
} from "../lib/contactHistory";
import { restoreContact, trashContact } from "../lib/serverApi";
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
  /** Contacts (trash) or Contact Groups (membership) whose request went through. */
  doneCount: number;
  failedCount: number;
};

type RunVars = { action: HistoryAction; direction: HistoryDirection };

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
  /** Changes with the account, so a run that settles after the change records nothing. */
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
      if (action.kind === "trash") {
        const send = direction === "do" ? trashContact : restoreContact;
        const went: HistoryContact[] = [];
        for (const contact of action.contacts) {
          try {
            await send(contact.id);
            went.push(contact);
          } catch {
            // Counted below; the rest are still sent.
          }
        }
        return {
          done: went.length > 0 ? { kind: "trash", contacts: went } : null,
          doneCount: went.length,
          failedCount: action.contacts.length - went.length,
        };
      }
      const op = groupOp(action, direction);
      const went = [];
      for (const change of action.changes) {
        const ids = change.contacts.map((c) => c.id);
        try {
          await setGroupMembers.mutateAsync({
            name: change.group,
            patch: op === "add" ? { add: ids } : { remove: ids },
          });
          went.push(change);
        } catch {
          // Counted below; the rest are still sent.
        }
      }
      return {
        done: went.length > 0 ? { ...action, changes: went } : null,
        doneCount: went.length,
        failedCount: action.changes.length - went.length,
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
        if (outcome.failedCount > 0) {
          setReport(partialFailureText(action, "do", outcome.doneCount, outcome.failedCount));
        }
      }),
    [apply, exclusive, run, showToast],
  );

  const undoEntry = stacks.undo.at(-1) ?? null;
  const redoEntry = stacks.redo.at(-1) ?? null;

  /**
   * Undo the entry on top of the undo stack, or redo the one on top of the
   * redo stack. A run that went through moves the entry to the other stack,
   * unless a new action landed meanwhile: the linear model then has nothing
   * to redo, so the entry leaves both stacks. A run that failed in part drops
   * the entry and says what happened in the banner.
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
        if (outcome.failedCount === 0) {
          const moved = pushes.current === pushesBefore;
          apply(
            moved
              ? { type: from === "undo" ? "undone" : "redone", id: entry.id }
              : { type: "drop", id: entry.id },
          );
          showToast(from === "undo" ? undoneText(action) : actionText(action), false);
          return;
        }
        apply({ type: "drop", id: entry.id });
        setToast(null);
        setReport(partialFailureText(action, direction, outcome.doneCount, outcome.failedCount));
      }),
    [apply, exclusive, run, showToast],
  );
  const undo = useCallback(() => step("undo"), [step]);
  const redo = useCallback(() => step("redo"), [step]);

  const clear = useCallback(() => {
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
