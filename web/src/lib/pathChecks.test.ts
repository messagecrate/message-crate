import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DENIED_STAT,
  DIRECTORY_STAT,
  FILE_STAT,
  MISSING_STAT,
  NEITHER_STAT,
} from "../test/pathStats";
import {
  checkOptionalPath,
  checkRequiredPath,
  type ImportPathStat,
  PATH_MISSING,
  type PathKind,
  probeImportPath,
} from "./pathChecks";

const invokePathStat = vi.hoisted(() => vi.fn());
vi.mock("./tauri", () => ({
  invokePathStat: (...a: unknown[]) => invokePathStat(...a),
}));

type Key = "field";

function check(path: string, stat: ImportPathStat | null, expected: PathKind) {
  const errors: Partial<Record<Key, string>> = {};
  checkOptionalPath(path, stat, errors, "field", { expected, kindError: "Wrong kind." });
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

describe("a path the app could not read", () => {
  const denied =
    "Message Crate isn't allowed to read this path. The system says: Operation not permitted (os error 1). On a Mac, give Message Crate Full Disk Access in System Settings, under Privacy & Security.";

  it("says the app is not allowed to read it, with the reason and the fix, rather than that it is missing", () => {
    expect(check("/tmp/x", DENIED_STAT, "directory")).toEqual({ field: denied });
    const errors: Partial<Record<Key, string>> = {};
    checkRequiredPath(DENIED_STAT, errors, "field", { expected: "file", kindError: "Wrong kind." });
    expect(errors.field).toBe(denied);
  });

  it("says it could not read a path the system refused for another reason", () => {
    const stat: ImportPathStat = {
      ...DENIED_STAT,
      unreadable: { kind: "other", reason: "Too many levels of symbolic links." },
    };
    expect(check("/tmp/x", stat, "file")).toEqual({
      field:
        "Message Crate could not read this path. The system says: Too many levels of symbolic links.",
    });
  });
});

describe("probeImportPath", () => {
  beforeEach(() => {
    invokePathStat.mockReset();
  });

  it("does not check an empty path", async () => {
    expect(await probeImportPath("  ")).toBeNull();
    expect(invokePathStat).not.toHaveBeenCalled();
  });

  it("checks the trimmed path", async () => {
    invokePathStat.mockResolvedValue(FILE_STAT);
    expect(await probeImportPath(" /tmp/chat.db ")).toEqual(FILE_STAT);
    expect(invokePathStat).toHaveBeenCalledWith("/tmp/chat.db");
  });

  it("reads a failed check as a path it could not check, not as a missing or unreadable one", async () => {
    invokePathStat.mockRejectedValue(new Error("ipc down"));
    const stat = await probeImportPath("/tmp/chat.db");
    expect(check("/tmp/chat.db", stat, "file")).toEqual({
      field: "Message Crate could not check this path. The check failed with: ipc down.",
    });
  });
});
