import { describe, expect, it } from "vitest";
import { type ChipTarget, patchChips, withName } from "./nameCollection";

describe("withName", () => {
  it("adds a name that is not there under any spelling", () => {
    expect(withName(["Work"], "Family", true)).toEqual(["Work", "Family"]);
  });

  it("does nothing when the name is already there under another spelling", () => {
    expect(withName(["family"], "Family", true)).toEqual(["family"]);
  });

  it("removes a name regardless of letter case", () => {
    expect(withName(["Family", "Work"], "family", false)).toEqual(["Work"]);
  });
});

describe("patchChips", () => {
  const rowTarget: ChipTarget = { key: ["contacts", "detail"], field: "groups", shape: "row" };
  const pagesTarget: ChipTarget = { key: ["contacts", "list"], field: "groups", shape: "pages" };

  it("leaves a non-pages entry alone when the target expects pages", () => {
    const entry = { id: "1", groups: [] };
    expect(patchChips(entry, pagesTarget, new Set(["1"]), "Family", true)).toBe(entry);
  });

  it("treats a row with no groups field as starting from empty", () => {
    const entry = { id: "1", name: "Ada" };
    expect(patchChips(entry, rowTarget, new Set(["1"]), "Family", true)).toEqual({
      id: "1",
      name: "Ada",
      groups: ["Family"],
    });
  });
});
