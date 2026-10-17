import { describe, expect, it, vi } from "vitest";
import { clearAllMembers } from "./membership";

describe("clearAllMembers", () => {
  it("sends one write for names that differ only in letter case", async () => {
    const removeName = vi.fn().mockResolvedValue(undefined);
    await clearAllMembers([["Work", "Family"], ["work"]], removeName);
    expect(removeName.mock.calls.map(([name]) => name)).toEqual(["Work", "Family"]);
  });
});
