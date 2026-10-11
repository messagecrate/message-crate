import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DENIED_STAT,
  DIRECTORY_STAT,
  desktopPathStat,
  FILE_STAT,
  MISSING_STAT,
  NEITHER_STAT,
  PERMISSION_DENIED,
} from "../test/pathStats";
import {
  checkOptionalPath,
  checkRequiredPath,
  type DesktopOs,
  describeUnreadable,
  type ImportPathStat,
  PATH_MISSING,
  type PathKind,
  probePath,
  unknownPathMessage,
} from "./pathChecks";

const invokePathStat = vi.hoisted(() => vi.fn());
const invokeHomeDir = vi.hoisted(() => vi.fn());
vi.mock("./tauri", () => ({
  invokePathStat: (...a: unknown[]) => invokePathStat(...a),
  invokeHomeDir: (...a: unknown[]) => invokeHomeDir(...a),
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
    "Message Crate isn't allowed to read this path. The system says: Operation not permitted (os error 1). Give Message Crate access to it, such as Full Disk Access in System Settings.";

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

// Full Disk Access is a Mac setting, so only a Mac is sent to it (#2650).
describe("a permission refusal on each operating system", () => {
  const refusal = (os: DesktopOs | null) =>
    unknownPathMessage({ kind: "permission_denied", reason: "Access is denied.", os });
  const said = "Message Crate isn't allowed to read this path. The system says: Access is denied.";

  it("sends a Mac to Full Disk Access", () => {
    expect(refusal("macos")).toBe(
      `${said} Give Message Crate access to it, such as Full Disk Access in System Settings.`,
    );
  });

  it("sends Windows to the Security tab of the path's Properties", () => {
    expect(refusal("windows")).toBe(
      `${said} Check that your Windows user may read it, on the Security tab of its Properties.`,
    );
  });

  it("asks Linux to check the user's own permission", () => {
    expect(refusal("linux")).toBe(`${said} Check that your Linux user may read it.`);
  });

  it("says nothing about access when the operating system is not known", () => {
    expect(refusal(null)).toBe(said);
  });

  it("uses it as the subject when the path is already named", () => {
    expect(
      unknownPathMessage(
        { kind: "permission_denied", reason: "Permission denied (os error 13)", os: "linux" },
        "it",
      ),
    ).toBe(
      "Message Crate isn't allowed to read it. The system says: Permission denied (os error 13). Check that your Linux user may read it.",
    );
  });
});

describe("describeUnreadable", () => {
  beforeEach(() => {
    invokeHomeDir.mockReset();
  });

  it("adds the operating system the desktop app names to a permission refusal", async () => {
    invokeHomeDir.mockResolvedValue({ path: "C:\\Users\\sam", os: "windows" });
    expect(await describeUnreadable(PERMISSION_DENIED)).toEqual({
      ...PERMISSION_DENIED,
      os: "windows",
    });
  });

  it("leaves the operating system unknown when the desktop app cannot say", async () => {
    invokeHomeDir.mockRejectedValue(new Error("Could not determine the user home directory"));
    expect(await describeUnreadable(PERMISSION_DENIED)).toEqual({ ...PERMISSION_DENIED, os: null });
  });

  it("leaves an operating system the app does not ship for unknown", async () => {
    invokeHomeDir.mockResolvedValue({ path: "/home/sam", os: "freebsd" });
    expect(await describeUnreadable(PERMISSION_DENIED)).toEqual({ ...PERMISSION_DENIED, os: null });
  });

  it("does not ask for the operating system for any other refusal", async () => {
    const other = { kind: "other" as const, reason: "Input/output error (os error 5)" };
    expect(await describeUnreadable(other)).toEqual(other);
    expect(invokeHomeDir).not.toHaveBeenCalled();
  });
});

describe("probePath", () => {
  beforeEach(() => {
    invokePathStat.mockReset();
    invokeHomeDir.mockReset();
  });

  it("says how to give access on the operating system that refused", async () => {
    invokePathStat.mockResolvedValue(
      desktopPathStat({ exists: false, isFile: false, unreadable: PERMISSION_DENIED }),
    );
    invokeHomeDir.mockResolvedValue({ path: "/home/sam", os: "linux" });
    const stat = await probePath("/home/sam/backup");
    expect(check("/home/sam/backup", stat, "directory")).toEqual({
      field:
        "Message Crate isn't allowed to read this path. The system says: Operation not permitted (os error 1). Check that your Linux user may read it.",
    });
  });

  it("does not check an empty path", async () => {
    expect(await probePath("  ")).toBeNull();
    expect(invokePathStat).not.toHaveBeenCalled();
  });

  it("checks the trimmed path", async () => {
    invokePathStat.mockResolvedValue(FILE_STAT);
    expect(await probePath(" /tmp/chat.db ")).toEqual(FILE_STAT);
    expect(invokePathStat).toHaveBeenCalledWith("/tmp/chat.db");
  });

  it("reads a failed check as a path it could not check, not as a missing or unreadable one", async () => {
    invokePathStat.mockRejectedValue(new Error("ipc down"));
    const stat = await probePath("/tmp/chat.db");
    expect(check("/tmp/chat.db", stat, "file")).toEqual({
      field: "Message Crate could not check this path. The check failed with: ipc down.",
    });
  });
});
