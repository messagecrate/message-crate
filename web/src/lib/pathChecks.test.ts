import { describe, expect, it } from "vitest";
import { DIRECTORY_STAT, FILE_STAT, MISSING_STAT, NEITHER_STAT } from "../test/pathStats";
import { checkOptionalPath, type ImportPathStat, PATH_MISSING, type PathKind } from "./pathChecks";

type Key = "field";

function check(path: string, stat: ImportPathStat | null, expected: PathKind) {
  const errors: Partial<Record<Key, string>> = {};
  checkOptionalPath(path, stat, errors, "field", "Wrong kind.", expected);
  return errors;
}

describe("checkOptionalPath", () => {
  it("leaves an empty or blank path alone, whatever the check found", () => {
    expect(check("", MISSING_STAT, "directory")).toEqual({});
    expect(check("   ", MISSING_STAT, "file")).toEqual({});
  });

  it("says nothing while the path has not been checked yet", () => {
    expect(check("/tmp/x", null, "directory")).toEqual({});
  });

  it("says a path that does not exist does not exist", () => {
    expect(check("/tmp/x", MISSING_STAT, "directory")).toEqual({ field: PATH_MISSING });
    expect(check("/tmp/x", MISSING_STAT, "file")).toEqual({ field: PATH_MISSING });
  });

  it("gives the field's own message for a file where a directory is needed", () => {
    expect(check("/tmp/x", FILE_STAT, "directory")).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", DIRECTORY_STAT, "directory")).toEqual({});
  });

  it("gives the field's own message for a directory where a file is needed", () => {
    expect(check("/tmp/x", DIRECTORY_STAT, "file")).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", FILE_STAT, "file")).toEqual({});
  });

  it("gives the field's own message for a path that is neither a file nor a directory", () => {
    expect(check("/tmp/x", NEITHER_STAT, "directory")).toEqual({ field: "Wrong kind." });
    expect(check("/tmp/x", NEITHER_STAT, "file")).toEqual({ field: "Wrong kind." });
  });
});
