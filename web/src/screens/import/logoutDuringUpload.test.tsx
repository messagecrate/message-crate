/** @vitest-environment jsdom */

// Logging out while an Upload runs (#1155). The Upload holds the session token
// it started with, so revoking the session under it would have every later
// request refused and every remaining conversation recorded as failed. Logout
// asks first, then pauses the Upload, waits for the Upload to halt and the pause
// to be recorded, and only then revokes the session.
//
// The other way round, a session the server refuses to any call the Import
// Run makes ends the session here too, and the run stays where the server has
// it for the next login (#1491 for the push, #1677 for the run's own calls).

import { act, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, getToken } from "../../lib/api";
import type { TauriJobResult, UploadFinishedReport } from "../../lib/tauri";
import { setupUser } from "../../test/user";

/** Every call that matters to the order, in the order it was made. */
const calls: string[] = [];

const serverLogoutMock = vi.fn();
const completeImportMock = vi.fn();
const setImportStageMock = vi.fn();
const discardImportRunMock = vi.fn();
const saveRunRecordMock = vi.fn();
const cancelMock = vi.fn();
const deleteRunDirMock = vi.fn();

/** Settles the running Upload with the report the desktop side sends. */
let finishUpload: ((result: TauriJobResult) => void) | null = null;

vi.mock("../../lib/tauri", () => ({
  awaitTauriJob: async (_job: string, invokeFn: () => Promise<void>) => {
    await invokeFn();
    calls.push("upload started");
    return new Promise<TauriJobResult>((resolve) => {
      finishUpload = resolve;
    });
  },
  invokeCancel: async () => {
    calls.push("cancel");
    cancelMock();
  },
  invokeUpload: async () => {},
  invokeReadImportRunRecord: async () => null,
  invokeSaveImportRunRecord: async () => {
    calls.push("pause recorded");
    saveRunRecordMock();
  },
  invokeDeleteRunDir: async ({ run_dir }: { run_dir: string }) => {
    calls.push(`deleted ${run_dir}`);
    await deleteRunDirMock(run_dir);
  },
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  logout: async () => {
    calls.push("session revoked");
    serverLogoutMock();
  },
  getSession: async () => ({}),
  completeImport: (...args: unknown[]) => completeImportMock(...args),
}));

vi.mock("../../lib/importRun", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/importRun")>()),
  setImportStage: (...args: unknown[]) => setImportStageMock(...args),
  discardImportRun: (...args: unknown[]) => discardImportRunMock(...args),
}));

vi.mock("../../lib/useAccountProfile", () => ({
  fetchAccountProfileFor: async () => ({}),
  useFetchAccountProfile: () => async () => ({ phones: [], emails: [] }),
}));

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => true,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => {
    throw new Error("no window in tests");
  },
}));

const { AuthProvider, useAuth } = await import("../../lib/auth");
const { isUploadRunning } = await import("../../lib/runningUpload");
const { useImportJob, resetImportRun } = await import("./useImportJob");

const MIB = 1024 * 1024;

const PAUSING = "Pausing the Upload. You are logged out once it has paused, or after 15 seconds.";
const DELETED_PAUSING =
  "Your account is deleted. Stopping its Upload before you are logged out, which takes at most 15 seconds.";
const NOT_PAUSED =
  "You were logged out before the Upload had paused. When you log in again, it resumes from what it had sent.";

const form = {
  source: "imessage-ios",
  backupPath: "/backups/iphone.tar",
  backupPassword: "",
  attachmentMedia: "copy" as const,
  maxResolution: "",
  maxFps: "",
  minSizeMb: "",
  ownerPhones: [],
  ownerEmails: [],
  obfuscate: false,
  isAndroidSms: false,
  attachmentRoot: "",
  appleContacts: "",
  whatsappKey: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
  whatsappBusiness: false,
  whatsappOwnerPhone: "",
  timeZone: "America/New_York",
  assetMaxBytes: 512 * MIB,
};

/** What `run.rs` reports when the cancel flag stops the Upload after 200 of 681. */
function pausedReport(): UploadFinishedReport {
  return {
    ok: false,
    cancelled: true,
    session_refused: false,
    messages_attempted: 4_000,
    messages_inserted: 4_000,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 0,
    assets_bytes: 0,
    conversations_ok: 200,
    conversations_total: 681,
    conversations_failed: 0,
    conversations_skipped: 0,
    conversations_cancelled: 481,
    results: [],
  };
}

type Screen = {
  job: ReturnType<typeof useImportJob>;
  auth: ReturnType<typeof useAuth>;
};

/** The Import screen's hook beside the login state, under the real AuthProvider. */
async function logIn(): Promise<{ current: Screen }> {
  const { result } = renderHook((): Screen => ({ job: useImportJob(), auth: useAuth() }), {
    wrapper: AuthProvider,
  });
  await act(() => result.current.auth.login("http://127.0.0.1:8080", "session-token", 7));
  return result;
}

/** Log in, and start a resumed Upload that runs until the test settles it. */
async function startUpload(): Promise<{ current: Screen }> {
  const result = await logIn();
  act(() => {
    void result.current.job.startImport(form, {
      runId: 42,
      runDir: "/home/sam/staging-iphone",
    });
  });
  await waitFor(() => expect(calls).toContain("upload started"));
  return result;
}

/** What the account menu's Log out does: it does not wait for the logout. */
function pressLogOut(result: { current: Screen }): void {
  act(() => {
    void result.current.auth.logout();
  });
}

describe("logging out during an Upload", () => {
  beforeEach(() => {
    localStorage.clear();
    resetImportRun();
    calls.length = 0;
    finishUpload = null;
    serverLogoutMock.mockReset();
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportRunMock.mockReset();
    discardImportRunMock.mockResolvedValue(undefined);
    saveRunRecordMock.mockReset();
    cancelMock.mockReset();
    deleteRunDirMock.mockReset();
    // The Upload stops when the cancel flag is set, the way `run.rs` does.
    cancelMock.mockImplementation(() =>
      finishUpload?.({ summary: "Upload", report: pausedReport() }),
    );
  });

  // An Upload a test left running would make the next test's logout ask.
  afterEach(async () => {
    finishUpload?.({ summary: "Upload", report: pausedReport() });
    await waitFor(() => expect(isUploadRunning()).toBe(false));
  });

  it("asks first, pauses the Upload, and revokes the session only once the pause is recorded", async () => {
    const user = setupUser();
    const result = await startUpload();

    pressLogOut(result);

    expect(
      await screen.findByText(
        "An Upload is running. Logging out pauses it; you can resume it after you log in.",
      ),
    ).toBeTruthy();
    // Nothing happens until the person chooses.
    expect(calls).not.toContain("cancel");
    expect(serverLogoutMock).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Log out" }));

    await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());
    expect(calls).toEqual(["upload started", "cancel", "pause recorded", "session revoked"]);
    // The run stays at its Upload stage: it is paused, not completed, and no
    // conversation was recorded as failed.
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(setImportStageMock.mock.calls.map((call) => call[1])).toEqual(["upload"]);
    expect(getToken()).toBeNull();
    expect(result.current.auth.isAuthenticated).toBe(false);
    // The paused run is the account's (#1085): logged out, the screen shows
    // the form, and the account finds the run again when it logs back in.
    expect(result.current.job.phase).toBe("form");
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));
    expect(result.current.job.summaryView?.status).toBe("paused");
    expect(result.current.job.summaryView?.filesFailed).toBe(0);
  });

  it("leaves the Upload running and the account logged in when the person goes back", async () => {
    const user = setupUser();
    const result = await startUpload();

    pressLogOut(result);
    await user.click(await screen.findByRole("button", { name: "Go back" }));

    await waitFor(() =>
      expect(
        screen.queryByText(
          "An Upload is running. Logging out pauses it; you can resume it after you log in.",
        ),
      ).toBeNull(),
    );
    expect(cancelMock).not.toHaveBeenCalled();
    expect(serverLogoutMock).not.toHaveBeenCalled();
    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(result.current.job.running).toBe(true);
  });

  it("logs out without asking when no Upload runs", async () => {
    const result = await logIn();

    pressLogOut(result);

    await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());
    expect(screen.queryByText(/An Upload is running/)).toBeNull();
    expect(cancelMock).not.toHaveBeenCalled();
  });

  it("logs out after 15 seconds when the Upload does not pause, and says it resumes from what it sent", async () => {
    const result = await startUpload();
    // An Upload that does not stop when asked.
    cancelMock.mockImplementation(() => {});
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = setupUser();
      pressLogOut(result);
      await user.click(await screen.findByRole("button", { name: "Log out" }));

      expect(await screen.findByText(PAUSING)).toBeTruthy();
      expect(screen.getByRole("button", { name: "Log out now" })).toBeTruthy();
      await act(() => vi.advanceTimersByTimeAsync(14_000));
      expect(serverLogoutMock).not.toHaveBeenCalled();
      expect(result.current.auth.isAuthenticated).toBe(true);

      await act(() => vi.advanceTimersByTimeAsync(1_500));

      await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());
      expect(result.current.auth.isAuthenticated).toBe(false);
      expect(getToken()).toBeNull();
      expect(await screen.findByText(NOT_PAUSED)).toBeTruthy();
      expect(calls).not.toContain("pause recorded");
    } finally {
      vi.useRealTimers();
    }
  });

  it("logs out at once on Log out now, without waiting for the Upload to pause", async () => {
    const user = setupUser();
    const result = await startUpload();
    cancelMock.mockImplementation(() => {});

    pressLogOut(result);
    await user.click(await screen.findByRole("button", { name: "Log out" }));
    await user.click(await screen.findByRole("button", { name: "Log out now" }));

    await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());
    expect(result.current.auth.isAuthenticated).toBe(false);
    expect(await screen.findByText(NOT_PAUSED)).toBeTruthy();
    expect(screen.queryByText(PAUSING)).toBeNull();
  });

  it("ends the session when the server refuses the Upload's session, and records no failure", async () => {
    const result = await startUpload();

    // What run.rs reports when a 401 stops the Upload after 200 of 681.
    act(() => {
      finishUpload?.({ summary: "Upload", report: { ...pausedReport(), session_refused: true } });
    });

    await waitFor(() => expect(result.current.auth.isAuthenticated).toBe(false));
    expect(getToken()).toBeNull();
    // The server has already ended the session: there is nothing to revoke.
    expect(serverLogoutMock).not.toHaveBeenCalled();
    expect(completeImportMock).not.toHaveBeenCalled();
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));
    expect(result.current.job.summaryView?.status).toBe("paused");
    expect(result.current.job.summaryView?.filesFailed).toBe(0);
  });

  it("deletes the directories of a deleted account's Import Runs once the Upload has paused and the session is revoked", async () => {
    const result = await startUpload();

    await act(() =>
      result.current.auth.logout({
        ask: false,
        deletedAccountDirectories: ["/home/sam/staging-iphone"],
      }),
    );

    expect(calls).toEqual([
      "upload started",
      "cancel",
      "pause recorded",
      "session revoked",
      "deleted /home/sam/staging-iphone",
    ]);
    // The run went with the account: nothing resumes, and nothing says so.
    expect(screen.queryByText(/resumes/)).toBeNull();
    expect(screen.queryByText(/could not delete/)).toBeNull();
  });

  it("deletes a deleted account's directory only once an Upload that did not pause has ended", async () => {
    const user = setupUser();
    const result = await startUpload();
    // An Upload that does not stop when asked.
    cancelMock.mockImplementation(() => {});

    act(() => {
      void result.current.auth.logout({
        ask: false,
        deletedAccountDirectories: ["/home/sam/staging-iphone"],
      });
    });
    await user.click(await screen.findByRole("button", { name: "Log out now" }));
    await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());

    // The Upload may still write into its directory: nothing is deleted yet.
    expect(calls).not.toContain("deleted /home/sam/staging-iphone");

    act(() => {
      finishUpload?.({ summary: "Upload", report: { ...pausedReport(), session_refused: true } });
    });

    await waitFor(() => expect(calls).toContain("deleted /home/sam/staging-iphone"));
    expect(calls.indexOf("deleted /home/sam/staging-iphone")).toBeGreaterThan(
      calls.indexOf("pause recorded"),
    );
  });

  it("says the account is deleted while its Upload pauses", async () => {
    const result = await startUpload();
    cancelMock.mockImplementation(() => {});

    act(() => {
      void result.current.auth.logout({ ask: false, deletedAccountDirectories: [] });
    });

    expect(await screen.findByText(DELETED_PAUSING)).toBeTruthy();
    expect(screen.queryByText(PAUSING)).toBeNull();
  });

  it("leaves a later login alone when an Upload it outlived is refused", async () => {
    const user = setupUser();
    const result = await startUpload();
    cancelMock.mockImplementation(() => {});

    pressLogOut(result);
    await user.click(await screen.findByRole("button", { name: "Log out" }));
    await user.click(await screen.findByRole("button", { name: "Log out now" }));
    await waitFor(() => expect(result.current.auth.isAuthenticated).toBe(false));
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));

    // The old Upload meets its revoked session only now.
    act(() => {
      finishUpload?.({ summary: "Upload", report: { ...pausedReport(), session_refused: true } });
    });
    await waitFor(() => expect(isUploadRunning()).toBe(false));

    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(getToken()).toBe("next-session-token");
  });

  it("names a directory of a deleted account's Import Runs it could not delete", async () => {
    deleteRunDirMock.mockRejectedValueOnce(new Error("permission denied"));
    const result = await logIn();

    await act(() =>
      result.current.auth.logout({
        ask: false,
        deletedAccountDirectories: ["/home/sam/staging-iphone", "/home/sam/staging-android"],
      }),
    );

    const notice = await screen.findByRole("dialog");
    expect(notice).toHaveTextContent("Message Crate could not delete");
    expect(notice).toHaveTextContent("/home/sam/staging-iphone: permission denied");
    expect(notice).not.toHaveTextContent("/home/sam/staging-android");
    expect(calls).toContain("deleted /home/sam/staging-android");
  });

  it("pauses the Upload without asking when the account was just deleted", async () => {
    const result = await startUpload();

    await act(() => result.current.auth.logout({ ask: false }));

    expect(screen.queryByText(/An Upload is running/)).toBeNull();
    expect(calls).toEqual(["upload started", "cancel", "pause recorded", "session revoked"]);
    expect(completeImportMock).not.toHaveBeenCalled();
  });
});

/** What the server answers a call whose session token it no longer accepts. */
function sessionRefusal(): ApiError {
  return new ApiError(401, "Authentication required.", {
    type: "https://messagecrate.app/problems/authentication-required",
    title: "Authentication required.",
    status: 401,
  });
}

/** Log in, and resume the paused Upload of run 42 without waiting for it. */
async function resumeUpload(): Promise<{ current: Screen }> {
  const result = await logIn();
  act(() => {
    void result.current.job.startImport(form, {
      runId: 42,
      runDir: "/home/sam/staging-iphone",
    });
  });
  return result;
}

describe("a session the server refuses to the Import Run's own calls (#1677)", () => {
  beforeEach(() => {
    localStorage.clear();
    resetImportRun();
    calls.length = 0;
    finishUpload = null;
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportRunMock.mockReset();
    discardImportRunMock.mockResolvedValue(undefined);
    saveRunRecordMock.mockReset();
    cancelMock.mockReset();
    deleteRunDirMock.mockReset();
  });

  afterEach(async () => {
    finishUpload?.({ summary: "Upload", report: pausedReport() });
    await waitFor(() => expect(isUploadRunning()).toBe(false));
  });

  it("ends the session when the Upload's stage change is refused, and keeps the run for the next login", async () => {
    setImportStageMock.mockRejectedValue(sessionRefusal());
    const result = await resumeUpload();

    await waitFor(() => expect(result.current.auth.isAuthenticated).toBe(false));
    expect(getToken()).toBeNull();
    await waitFor(() => expect(result.current.job.running).toBe(false));
    // The push never starts, and the run is neither completed nor deleted.
    expect(calls).not.toContain("upload started");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(deleteRunDirMock).not.toHaveBeenCalled();

    // The next login finds the form, whose resume check offers the run
    // where the server holds it, and no Import Error for the refusal.
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));
    expect(result.current.job.phase).toBe("form");
    expect(result.current.job.summaryView).toBeNull();
  });

  it("ends the session when the run's completion is refused, and records no Import Error for it", async () => {
    completeImportMock.mockRejectedValue(sessionRefusal());
    const result = await resumeUpload();
    await waitFor(() => expect(calls).toContain("upload started"));

    act(() => {
      finishUpload?.({
        summary: "Upload",
        report: {
          ...pausedReport(),
          ok: true,
          cancelled: false,
          conversations_ok: 681,
          conversations_cancelled: 0,
        },
      });
    });

    await waitFor(() => expect(result.current.auth.isAuthenticated).toBe(false));
    await waitFor(() => expect(isUploadRunning()).toBe(false));
    expect(deleteRunDirMock).not.toHaveBeenCalled();
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));
    // The server still holds the run at its Upload, which resumes, finds
    // every message sent, and completes it.
    expect(result.current.job.summaryView?.status).toBe("paused");
    expect(result.current.job.summaryView?.issues).toEqual([]);
  });

  it("ends the session when a Discard is refused, and keeps the run's directory", async () => {
    discardImportRunMock.mockRejectedValue(sessionRefusal());
    const result = await logIn();

    await act(() => result.current.job.discardRun(42, "/home/sam/staging-iphone"));

    await waitFor(() => expect(result.current.auth.isAuthenticated).toBe(false));
    // The run is still open on the server, so its directory stays for the
    // next login to offer it again.
    expect(deleteRunDirMock).not.toHaveBeenCalled();
  });

  it("leaves a later login alone when a refusal arrives after it", async () => {
    let refuse: (error: unknown) => void = () => {};
    setImportStageMock.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          refuse = reject;
        }),
    );
    const result = await resumeUpload();
    await waitFor(() => expect(setImportStageMock).toHaveBeenCalled());
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));

    act(() => refuse(sessionRefusal()));
    await waitFor(() => expect(result.current.job.running).toBe(false));

    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(getToken()).toBe("next-session-token");
  });
});
