import { describe, expect, it, vi } from "vitest";
import { desktopPathStat, PERMISSION_DENIED } from "../test/pathStats";
import { accountRunDirectories, buildSourceFingerprint } from "./importRun";

const listEveryImport = vi.hoisted(() => vi.fn());
const invokePathStat = vi.hoisted(() => vi.fn());

vi.mock("./serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./serverApi")>()),
  listEveryImport: (...a: unknown[]) => listEveryImport(...a),
}));

vi.mock("./tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauri")>()),
  invokePathStat: (...a: unknown[]) => invokePathStat(...a),
}));

describe("accountRunDirectories", () => {
  // A run's directory is on whichever computer ran it, so a deleted account's
  // directories are named only when they are on this one (#1491).
  it("names each run's directory once, and only when it is on this computer", async () => {
    listEveryImport.mockResolvedValue([
      { id: 3, run_dir: "/staging/iphone" },
      { id: 2, run_dir: null },
      { id: 1, run_dir: "/staging/iphone" },
      { id: 0, run_dir: "/other-computer/android" },
    ]);
    invokePathStat.mockImplementation(async (path: string) => ({
      exists: path === "/staging/iphone",
      isFile: false,
      isDirectory: path === "/staging/iphone",
      sizeBytes: 0,
      modifiedUnixMs: null,
      unreadable: null,
    }));

    expect(await accountRunDirectories()).toEqual({ found: ["/staging/iphone"], unreadable: [] });
    expect(invokePathStat).toHaveBeenCalledTimes(2);
  });

  // A directory the app may not read may still be there with the deleted
  // account's data, so it is named with the system's reason, not skipped (#2651).
  it("reports a run directory the app may not read, with the system's reason", async () => {
    listEveryImport.mockResolvedValue([
      { id: 2, run_dir: "/staging/iphone" },
      { id: 1, run_dir: "/Users/u/Documents/staging" },
      { id: 0, run_dir: "/mnt/broken" },
    ]);
    invokePathStat.mockImplementation(async (path: string) => {
      if (path === "/Users/u/Documents/staging") {
        return desktopPathStat({ exists: false, isFile: false, unreadable: PERMISSION_DENIED });
      }
      if (path === "/mnt/broken") {
        return desktopPathStat({
          exists: false,
          isFile: false,
          unreadable: { kind: "other", reason: "Input/output error (os error 5)" },
        });
      }
      return desktopPathStat({ isFile: false, isDirectory: true });
    });

    expect(await accountRunDirectories()).toEqual({
      found: ["/staging/iphone"],
      unreadable: [
        {
          path: "/Users/u/Documents/staging",
          reason:
            "Message Crate isn't allowed to read it. The system says: Operation not permitted (os error 1). Give Message Crate access to it, such as Full Disk Access in System Settings on a Mac.",
        },
        {
          path: "/mnt/broken",
          reason:
            "Message Crate could not read it. The system says: Input/output error (os error 5).",
        },
      ],
    });
  });
});

describe("buildSourceFingerprint", () => {
  it("records the path, size, and mtime of the backup", () => {
    expect(
      buildSourceFingerprint(
        "/Users/u/Backup/abc",
        desktopPathStat({
          isFile: false,
          isDirectory: true,
          sizeBytes: 4096,
          modifiedUnixMs: 1_756_512_000_000,
        }),
      ),
    ).toEqual({
      path: "/Users/u/Backup/abc",
      size_bytes: 4096,
      modified_unix_ms: 1_756_512_000_000,
    });
  });

  it("records nothing for a backup the app may not read, so a later resume is not told it changed", () => {
    expect(
      buildSourceFingerprint(
        "/Users/u/Library/Messages",
        desktopPathStat({
          exists: false,
          isFile: false,
          sizeBytes: 0,
          modifiedUnixMs: null,
          unreadable: PERMISSION_DENIED,
        }),
      ),
    ).toBeNull();
  });
});
