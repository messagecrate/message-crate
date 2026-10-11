import { describe, expect, it } from "vitest";
import { checkOptionalPath, type ImportPathStat, PATH_MISSING, type PathKind } from "./pathChecks";

const dir: ImportPathStat = { exists: true, isFile: false, isDirectory: true };
const file: ImportPathStat = { exists: true, isFile: true, isDirectory: false };
const missing: ImportPathStat = { exists: false, isFile: false, isDirectory: false };

type Key = "field";

function check(path: string, stat: ImportPathStat | null, expected: PathKind) {
  const errors: Partial<Record<Key, string>> = {};
  checkOptionalPath(path, stat, errors, "field", "Wrong kind.", expected);
  return errors;
}

describe("checkOptionalPath", () => {
  it("leaves an empty or blank path alone, whatever the check found", () => {
    expect(check("", missing, "directory")).toEqual({});
    expect(check("   ", missing, "file")).toEqual({});
  });

  it("says nothing while the path has not been checked yet", () => {
    expect(check("/tmp/x", null, "directory")).toEqual({});
  });

  it("says a path that does not exist does not exist", () => {
    expect(check("/tmp/x", missing, "directory")).toEqual({ field: PATH_MISSING });
    expect(check("/tmp/x", missing, "file")).toEqual({ field: PATH_MISSING });
  });

  it("gives the field's own message for a file where a directory is needed", () => {
    expect(check("/tmp/x", file, "directory")).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", dir, "directory")).toEqual({});
  });

  it("gives the field's own message for a directory where a file is needed", () => {
    expect(check("/tmp/x", dir, "file")).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", file, "file")).toEqual({});
  });
});
