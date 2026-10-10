import { describe, expect, it } from "vitest";
import { checkOptionalPath, PATH_MISSING, type PathStat } from "./pathChecks";

const dir: PathStat = { exists: true, isFile: false, isDirectory: true };
const file: PathStat = { exists: true, isFile: true, isDirectory: false };
const missing: PathStat = { exists: false, isFile: false, isDirectory: false };

type Key = "field";

function check(path: string, stat: PathStat | null, expectDirectory: boolean) {
  const errors: Partial<Record<Key, string>> = {};
  checkOptionalPath(path, stat, errors, "field", "Wrong kind.", expectDirectory);
  return errors;
}

describe("checkOptionalPath", () => {
  it("leaves an empty or blank path alone, whatever the check found", () => {
    expect(check("", missing, true)).toEqual({});
    expect(check("   ", missing, false)).toEqual({});
  });

  it("says nothing while the path has not been checked yet", () => {
    expect(check("/tmp/x", null, true)).toEqual({});
  });

  it("says a path that does not exist does not exist", () => {
    expect(check("/tmp/x", missing, true)).toEqual({ field: PATH_MISSING });
    expect(check("/tmp/x", missing, false)).toEqual({ field: PATH_MISSING });
  });

  it("gives the field's own message for a file where a directory is needed", () => {
    expect(check("/tmp/x", file, true)).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", dir, true)).toEqual({});
  });

  it("gives the field's own message for a directory where a file is needed", () => {
    expect(check("/tmp/x", dir, false)).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", file, false)).toEqual({});
  });
});
