/** @vitest-environment jsdom */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Providers } from "../test/providers";
import {
  PATH_PROBE_DEBOUNCE_MS,
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

vi.mock("./tauri-check", () => ({ isTauri: () => desktop.isTauri }));

vi.mock("./tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauri")>()),
  invokePathStat: (path: string) => desktop.pathStat(path),
  invokeIosBackupEncrypted: (path: string) => desktop.encrypted(path),
}));

const DIRECTORY = { exists: true, isFile: false, isDirectory: true };
const FILE = { exists: true, isFile: true, isDirectory: false };
const MISSING = { exists: false, isFile: false, isDirectory: false };

/** Let the debounce run out and the desktop's answers arrive. */
async function settle() {
  await act(() => vi.advanceTimersByTimeAsync(PATH_PROBE_DEBOUNCE_MS));
  // The query starts on the render the debounce caused, and its answer
  // reaches the hook on a timer of its own.
  await act(() => vi.advanceTimersByTimeAsync(1));
}

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
    ({ source, backupPath }: ImessageProps) => useImessagePathStats(source, backupPath, "", ""),
    { initialProps: initial, wrapper: Providers },
  );
}

describe("useImessagePathStats", () => {
  it("checks a typed path once, after the typing stops", async () => {
    desktop.paths.set("/backup", "directory");
    const view = renderImessage({ source: "imessage-ios", backupPath: "" });
    await settle();
    desktop.pathStat.mockClear();

    for (const typed of ["/b", "/ba", "/backup"]) {
      view.rerender({ source: "imessage-ios", backupPath: typed });
      await act(() => vi.advanceTimersByTimeAsync(PATH_PROBE_DEBOUNCE_MS - 50));
    }
    expect(desktop.pathStat).not.toHaveBeenCalled();

    await settle();
    expect(desktop.pathStat.mock.calls).toEqual([["/backup"]]);
    expect(view.result.current.backup).toEqual(DIRECTORY);
    expect(view.result.current.backupEncrypted).toBe(true);
  });

  it("keeps the last answer while a path is typed, and starts another method empty", async () => {
    desktop.paths.set("/backup", "directory");
    const view = renderImessage({ source: "imessage-ios", backupPath: "/backup" });
    await settle();
    expect(view.result.current.backup).toEqual(DIRECTORY);

    view.rerender({ source: "imessage-ios", backupPath: "/backup/x" });
    expect(view.result.current.backup).toEqual(DIRECTORY);
    await settle();
    expect(view.result.current.backup).toEqual(MISSING);

    view.rerender({ source: "imessage-macos", backupPath: "/backup/x" });
    expect(view.result.current.backup).toBeNull();
    await settle();
    expect(view.result.current.backup).toEqual(MISSING);
    // Only an iPhone backup is asked whether it is encrypted.
    expect(view.result.current.backupEncrypted).toBeNull();
  });

  it("asks nothing in the browser, which has no desktop process", async () => {
    desktop.isTauri = false;
    const view = renderImessage({ source: "imessage-ios", backupPath: "/backup" });
    await settle();
    expect(desktop.pathStat).not.toHaveBeenCalled();
    expect(view.result.current.backup).toBeNull();
  });
});

type WhatsappProps = { source: string; backupPath: string };

function renderWhatsapp(initial: WhatsappProps) {
  return renderHook(
    ({ source, backupPath }: WhatsappProps) => useWhatsappPathStats(source, backupPath, "", "", ""),
    { initialProps: initial, wrapper: Providers },
  );
}

describe("useWhatsappPathStats", () => {
  it("finds the crypt file in the backup directory, and empties at once when the path changes", async () => {
    desktop.paths.set("/wa", "directory");
    desktop.paths.set("/wa/msgstore.db.crypt15", "file");
    const view = renderWhatsapp({ source: "whatsapp-android", backupPath: "/wa" });
    await settle();
    expect(view.result.current.backup).toEqual(DIRECTORY);
    expect(view.result.current.cryptName).toBe("msgstore.db.crypt15");
    expect(view.result.current.backupEncrypted).toBeNull();

    view.rerender({ source: "whatsapp-android", backupPath: "/wa2" });
    expect(view.result.current.backup).toBeNull();
    expect(view.result.current.cryptName).toBeNull();
    await settle();
    expect(view.result.current.backup).toEqual(MISSING);
    expect(view.result.current.cryptName).toBeNull();
  });

  it("asks again for paths that come back, since the files can change on disk", async () => {
    // A client that keeps answers fresh for half a minute, as the app's does.
    const client = new QueryClient({
      defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    desktop.paths.set("/wa", "directory");
    const view = renderHook(
      ({ backupPath }: { backupPath: string }) =>
        useWhatsappPathStats("whatsapp-android", backupPath, "", "", ""),
      { initialProps: { backupPath: "/wa" }, wrapper },
    );
    await settle();
    expect(view.result.current.cryptName).toBeNull();

    view.rerender({ backupPath: "/wa/" });
    await settle();
    desktop.paths.set("/wa/msgstore.db.crypt15", "file");
    view.rerender({ backupPath: "/wa" });
    await settle();
    expect(view.result.current.cryptName).toBe("msgstore.db.crypt15");
  });

  it("asks an iPhone backup whether it is encrypted", async () => {
    desktop.paths.set("/ios", "directory");
    const view = renderWhatsapp({ source: "whatsapp-ios", backupPath: "/ios" });
    await settle();
    expect(desktop.encrypted).toHaveBeenCalledWith("/ios");
    expect(view.result.current.backupEncrypted).toBe(true);
  });
});
