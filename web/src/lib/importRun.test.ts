import { describe, expect, it, vi } from "vitest";
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
    }));

    expect(await accountRunDirectories()).toEqual(["/staging/iphone"]);
    expect(invokePathStat).toHaveBeenCalledTimes(2);
  });
});

describe("buildSourceFingerprint", () => {
  it("records the path, size, and mtime of the backup", () => {
    expect(
      buildSourceFingerprint("/Users/u/Backup/abc", {
        exists: true,
        isFile: false,
        isDirectory: true,
        sizeBytes: 4096,
        modifiedUnixMs: 1_756_512_000_000,
      }),
    ).toEqual({
      path: "/Users/u/Backup/abc",
      size_bytes: 4096,
      modified_unix_ms: 1_756_512_000_000,
    });
  });
});
