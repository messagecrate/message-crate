/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "../../lib/routeQuery";
import { advanceTimersAndSettle } from "../../test/fakeTimers";
import { Providers } from "../../test/providers";
import {
  PATH_PROBE_DEBOUNCE_MS,
  useBackupPathStats,
  useImessagePathStats,
  useWhatsappPathStats,
} from "./useBackupPathStats";

const desktop = vi.hoisted(() => ({
  isTauri: true,
  /** The paths on this computer, each a directory or a file. */
  paths: new Map<string, "directory" | "file">(),
  pathStat: vi.fn(),
  encrypted: vi.fn(),
}));

vi.mock("../../lib/tauriCheck", () => ({ isTauri: () => desktop.isTauri }));

vi.mock("../../lib/desktop/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/desktop/fs")>()),
  invokePathStat: (path: string) => desktop.pathStat(path),
}));

vi.mock("../../lib/desktop/backup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/desktop/backup")>()),
  invokeIosBackupEncrypted: (path: string) => desktop.encrypted(path),
}));

const DIRECTORY = { exists: true, isFile: false, isDirectory: true };
const FILE = { exists: true, isFile: true, isDirectory: false };
const MISSING = { exists: false, isFile: false, isDirectory: false };

beforeEach(() => {
  vi.useFakeTimers();
  desktop.isTauri = true;
  desktop.paths.clear();
  desktop.pathStat.mockImplementation(async (path: string) => {
    const kind = desktop.paths.get(path);
    return kind === "directory" ? DIRECTORY : kind === "file" ? FILE : MISSING;
  });
  desktop.encrypted.mockResolvedValue(true);
});

afterEach(() => {
  vi.useRealTimers();
  desktop.pathStat.mockReset();
  desktop.encrypted.mockReset();
});

type ImessageProps = { source: string; backupPath: string };

function renderImessage(initial: ImessageProps) {
  return renderHook(
    ({ source, backupPath }: ImessageProps) =>
      useImessagePathStats({ source, backupPath, attachmentRoot: "", appleContacts: "" }),
    { initialProps: initial, wrapper: Providers },
  );
}

describe("useImessagePathStats", () => {
  it("checks a typed path once, after the typing stops", async () => {
    desktop.paths.set("/backup", "directory");
    const view = renderImessage({ source: "imessage-ios", backupPath: "" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    desktop.pathStat.mockClear();

    for (const typed of ["/b", "/ba", "/backup"]) {
      view.rerender({ source: "imessage-ios", backupPath: typed });
      await act(() => vi.advanceTimersByTimeAsync(PATH_PROBE_DEBOUNCE_MS - 50));
    }
    expect(desktop.pathStat).not.toHaveBeenCalled();

    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(desktop.pathStat.mock.calls).toEqual([["/backup"]]);
    expect(view.result.current.backup).toEqual(DIRECTORY);
    expect(view.result.current.backupEncrypted).toBe(true);
  });

  it("keeps the last answer while a path is typed, and starts another method empty", async () => {
    desktop.paths.set("/backup", "directory");
    const view = renderImessage({ source: "imessage-ios", backupPath: "/backup" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(DIRECTORY);

    view.rerender({ source: "imessage-ios", backupPath: "/backup/x" });
    expect(view.result.current.backup).toEqual(DIRECTORY);
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(MISSING);

    view.rerender({ source: "imessage-macos", backupPath: "/backup/x" });
    expect(view.result.current.backup).toBeNull();
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(MISSING);
    // Only an iPhone backup is asked whether it is encrypted.
    expect(view.result.current.backupEncrypted).toBeNull();
  });

  it("asks nothing in the browser, which has no desktop process", async () => {
    desktop.isTauri = false;
    const view = renderImessage({ source: "imessage-ios", backupPath: "/backup" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(desktop.pathStat).not.toHaveBeenCalled();
    expect(view.result.current.backup).toBeNull();
  });
});

type WhatsappProps = { source: string; backupPath: string };

function renderWhatsapp(initial: WhatsappProps) {
  return renderHook(
    ({ source, backupPath }: WhatsappProps) =>
      useWhatsappPathStats({ source, backupPath, contactsDb: "", media: "", db: "" }),
    { initialProps: initial, wrapper: Providers },
  );
}

describe("useWhatsappPathStats", () => {
  it("finds the crypt file in the backup directory, and empties at once when the path changes", async () => {
    desktop.paths.set("/wa", "directory");
    desktop.paths.set("/wa/msgstore.db.crypt15", "file");
    const view = renderWhatsapp({ source: "whatsapp-android", backupPath: "/wa" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(DIRECTORY);
    expect(view.result.current.cryptName).toBe("msgstore.db.crypt15");
    expect(view.result.current.backupEncrypted).toBeNull();

    view.rerender({ source: "whatsapp-android", backupPath: "/wa2" });
    expect(view.result.current.backup).toBeNull();
    expect(view.result.current.cryptName).toBeNull();
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(MISSING);
    expect(view.result.current.cryptName).toBeNull();
  });

  it("asks again for paths that come back, since the files can change on disk", async () => {
    // The app's own client, which keeps other answers fresh for a while.
    const client = createQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    desktop.paths.set("/wa", "directory");
    const view = renderHook(
      ({ backupPath }: { backupPath: string }) =>
        useWhatsappPathStats({
          source: "whatsapp-android",
          backupPath,
          contactsDb: "",
          media: "",
          db: "",
        }),
      { initialProps: { backupPath: "/wa" }, wrapper },
    );
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.cryptName).toBeNull();

    view.rerender({ backupPath: "/wa/" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    desktop.paths.set("/wa/msgstore.db.crypt15", "file");
    view.rerender({ backupPath: "/wa" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.cryptName).toBe("msgstore.db.crypt15");
  });

  it("asks an iPhone backup whether it is encrypted", async () => {
    desktop.paths.set("/ios", "directory");
    const view = renderWhatsapp({ source: "whatsapp-ios", backupPath: "/ios" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(desktop.encrypted).toHaveBeenCalledWith("/ios");
    expect(view.result.current.backupEncrypted).toBe(true);
  });
});

describe("useBackupPathStats", () => {
  type BackupPathProps = { source: string; backupPath: string };

  // SMS Backup & Restore takes a file and the other Android sources a
  // directory, so the form needs to know which one the path is (#2029).
  it("checks an Android SMS backup path, and asks nothing for another source", async () => {
    desktop.paths.set("/sms", "directory");
    const view = renderHook((paths: BackupPathProps) => useBackupPathStats(paths), {
      initialProps: { source: "sms-backup-restore", backupPath: "/sms" },
      wrapper: Providers,
    });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(DIRECTORY);

    desktop.pathStat.mockClear();
    view.rerender({ source: "imessage-ios", backupPath: "/sms" });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(desktop.pathStat).not.toHaveBeenCalled();
    expect(view.result.current.backup).toBeNull();
  });

  // #3167: a mistyped iMazing or OpenExtract path made a failed run.
  it.each(["imazing", "openextract"])("checks the %s backup path", async (source) => {
    desktop.paths.set("/export", "directory");
    const view = renderHook((paths: BackupPathProps) => useBackupPathStats(paths), {
      initialProps: { source, backupPath: "/export" },
      wrapper: Providers,
    });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(DIRECTORY);
  });

  // The old path's kind says nothing about the new one, so Import must not
  // start on it while the new path waits for its check.
  it("empties at once when the path changes", async () => {
    desktop.paths.set("/sms.xml", "file");
    desktop.paths.set("/sms", "directory");
    const view = renderHook((paths: BackupPathProps) => useBackupPathStats(paths), {
      initialProps: { source: "sms-backup-restore", backupPath: "/sms.xml" },
      wrapper: Providers,
    });
    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(FILE);

    view.rerender({ source: "sms-backup-restore", backupPath: "/sms" });
    expect(view.result.current.backup).toBeNull();

    await advanceTimersAndSettle(PATH_PROBE_DEBOUNCE_MS);
    expect(view.result.current.backup).toEqual(DIRECTORY);
  });
});
