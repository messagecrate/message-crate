import { describe, expect, it } from "vitest";
import {
  actionMenuName,
  actionText,
  EMPTY_HISTORY,
  HISTORY_LIMIT,
  type HistoryAction,
  type HistoryEntry,
  type HistoryStacks,
  historyReducer,
  partialFailureText,
  undoneText,
} from "./contactHistory";

const mother = { id: 1, label: "Mother" };
const three = [mother, { id: 2, label: "Ada" }, { id: 3, label: "Grace" }];

function entry(id: number): HistoryEntry {
  return { id, action: { kind: "trash", contacts: [{ id, label: `C${id}` }] } };
}

function pushAll(ids: number[], from: HistoryStacks = EMPTY_HISTORY): HistoryStacks {
  return ids.reduce((s, id) => historyReducer(s, { type: "push", entry: entry(id) }), from);
}

const ids = (list: HistoryEntry[]) => list.map((e) => e.id);

describe("historyReducer", () => {
  it("keeps the newest 20 entries and drops the oldest", () => {
    const s = pushAll(Array.from({ length: HISTORY_LIMIT + 2 }, (_, i) => i + 1));
    expect(s.undo).toHaveLength(HISTORY_LIMIT);
    expect(s.undo[0]?.id).toBe(3);
    expect(s.undo.at(-1)?.id).toBe(HISTORY_LIMIT + 2);
  });

  it("moves an undone entry to the redo stack and back on redo", () => {
    let s = pushAll([1, 2]);
    s = historyReducer(s, { type: "undone", id: 2 });
    expect(ids(s.undo)).toEqual([1]);
    expect(ids(s.redo)).toEqual([2]);
    s = historyReducer(s, { type: "redone", id: 2 });
    expect(ids(s.undo)).toEqual([1, 2]);
    expect(s.redo).toEqual([]);
  });

  it("discards what was undone when a new action comes after the undo", () => {
    let s = pushAll([1, 2, 3]);
    s = historyReducer(s, { type: "undone", id: 3 });
    s = historyReducer(s, { type: "undone", id: 2 });
    s = pushAll([4], s);
    expect(ids(s.undo)).toEqual([1, 4]);
    expect(s.redo).toEqual([]);
  });

  it("drops an entry whose undo failed in part from both stacks", () => {
    let s = pushAll([1, 2]);
    s = historyReducer(s, { type: "undone", id: 2 });
    s = historyReducer(s, { type: "drop", id: 2 });
    s = historyReducer(s, { type: "drop", id: 1 });
    expect(s).toEqual(EMPTY_HISTORY);
  });

  it("clears both stacks, as a delete for good does", () => {
    let s = pushAll([1, 2]);
    s = historyReducer(s, { type: "undone", id: 2 });
    expect(historyReducer(s, { type: "clear" })).toEqual(EMPTY_HISTORY);
  });
});

describe("the words", () => {
  const cases: Array<[HistoryAction, string, string, string]> = [
    [
      { kind: "trash", contacts: three },
      "Moved 3 contacts to Trash",
      "Undo Delete Contacts",
      "Restored 3 contacts",
    ],
    [
      { kind: "trash", contacts: [mother] },
      "Moved Mother to Trash",
      "Undo Delete Contacts",
      "Restored Mother",
    ],
    [
      { kind: "group", op: "add", changes: [{ group: "Family", contacts: three.slice(0, 2) }] },
      "Added 2 contacts to Family",
      "Undo Add to Contact Group",
      "Removed 2 contacts from Family",
    ],
    [
      { kind: "group", op: "remove", changes: [{ group: "Family", contacts: [mother] }] },
      "Removed Mother from Family",
      "Undo Remove from Contact Group",
      "Added Mother to Family",
    ],
    [
      {
        kind: "group",
        op: "remove",
        changes: [
          { group: "Family", contacts: [mother] },
          { group: "Work", contacts: [mother] },
        ],
      },
      "Removed Mother from 2 Contact Groups",
      "Undo Remove from Contact Group",
      "Added Mother to 2 Contact Groups",
    ],
  ];

  it.each(cases)("names %j", (action, done, menu, undone) => {
    expect(actionText(action)).toBe(done);
    expect(`Undo ${actionMenuName(action)}`).toBe(menu);
    expect(undoneText(action)).toBe(undone);
  });

  it("says what a partial restore did and did not do", () => {
    const action: HistoryAction = { kind: "trash", contacts: three };
    expect(partialFailureText(action, "undo", 2, 1)).toBe(
      "Restored 2 contacts; 1 is no longer in the Trash",
    );
    expect(partialFailureText(action, "do", 1, 2)).toBe(
      "Moved 1 contact to Trash; 2 could not be moved",
    );
  });
});
