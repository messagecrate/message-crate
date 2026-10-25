import { describe, expect, it } from "vitest";
import {
  actionMenuName,
  actionText,
  EMPTY_HISTORY,
  groupFailureText,
  HISTORY_LIMIT,
  type HistoryAction,
  type HistoryEntry,
  type HistoryStacks,
  historyReducer,
  trashFailureText,
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
    s = historyReducer(s, { type: "redone", entry: entry(2) });
    expect(ids(s.undo)).toEqual([1, 2]);
    expect(s.redo).toEqual([]);
  });

  it("puts a redone entry back on the undo stack after a newer action", () => {
    let s = pushAll([1]);
    s = historyReducer(s, { type: "undone", id: 1 });
    s = pushAll([2], s);
    s = historyReducer(s, { type: "redone", entry: entry(1) });
    expect(ids(s.undo)).toEqual([2, 1]);
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

  it("keeps one merge entry, because the server keeps only the latest merge", () => {
    const merge = (id: number): HistoryEntry => ({
      id,
      action: { kind: "merge", target: mother, merged: [{ id: 10 + id, label: "x", named: true }] },
    });
    let s = historyReducer(EMPTY_HISTORY, { type: "push", entry: merge(1) });
    s = pushAll([2], s);
    s = historyReducer(s, { type: "push", entry: merge(3) });
    expect(ids(s.undo)).toEqual([2, 3]);
    // A redo that lands after a newer merge is a merge too, and drops it.
    s = historyReducer(s, { type: "undone", id: 3 });
    s = historyReducer(s, { type: "push", entry: merge(4) });
    s = historyReducer(s, { type: "redone", entry: merge(3) });
    expect(ids(s.undo)).toEqual([2, 3]);
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
    [
      {
        kind: "merge",
        target: mother,
        merged: [
          { id: 2, label: "Nick Koch", named: true },
          { id: 3, label: "+12155550112", named: false },
        ],
      },
      "Merged 3 contacts into Mother",
      "Undo Merge Contacts",
      "Unmerged Nick Koch and 1 contact with no name",
    ],
    [
      {
        kind: "merge",
        target: mother,
        merged: [
          { id: 2, label: "+12155550112", named: false },
          { id: 3, label: "+12155550113", named: false },
        ],
      },
      "Merged 3 contacts into Mother",
      "Undo Merge Contacts",
      "Unmerged 2 contacts with no name",
    ],
    [
      {
        kind: "merge",
        target: mother,
        merged: [
          { id: 2, label: "Nick", named: true },
          { id: 3, label: "Nicky", named: true },
        ],
      },
      "Merged 3 contacts into Mother",
      "Undo Merge Contacts",
      "Unmerged 2 contacts",
    ],
  ];

  it.each(cases)("names %j", (action, done, menu, undone) => {
    expect(actionText(action)).toBe(done);
    expect(`Undo ${actionMenuName(action)}`).toBe(menu);
    expect(undoneText(action)).toBe(undone);
  });

  it("says what a partial restore did and did not do", () => {
    expect(trashFailureText("undo", 2, 1)).toBe(
      "Restored 2 contacts. 1 is no longer in the Trash.",
    );
    expect(trashFailureText("do", 1, 2)).toBe("Moved 1 contact to Trash. 2 could not be moved.");
  });

  it("names each refused Contact Group on a line of its own, with its reason", () => {
    const ada = { id: 2, label: "Ada" };
    expect(
      groupFailureText(
        "add",
        [{ group: "Work", contacts: [mother, ada] }],
        [
          { group: "Family", contacts: [mother], reason: "Contact Group not found" },
          { group: "Club", contacts: [mother, ada], reason: "the server did not answer" },
        ],
      ),
    ).toBe(
      [
        "Added 2 contacts to 1 of 3 Contact Groups.",
        "Could not add Mother to Family: Contact Group not found",
        "Could not add 2 contacts to Club: the server did not answer",
      ].join("\n"),
    );
    expect(
      groupFailureText("remove", [], [{ group: "Family", contacts: [mother], reason: "busy" }]),
    ).toBe("Could not remove Mother from Family: busy");
  });
});
