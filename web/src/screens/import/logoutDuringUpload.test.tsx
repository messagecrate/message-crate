/** @vitest-environment jsdom */

// Logging out while an Upload runs (#1155). The Upload holds the session token
// it started with, so revoking the session under it would have every later
// request refused and every remaining conversation recorded as failed. Logout
// asks first, then pauses the Upload, waits for the Upload to halt and the pause
// to be recorded, and only then revokes the session.
//
// The other way round, a session the server refuses to any call the Import
// Run makes ends the session here too, and the run stays where the server has
// it for the next login (#1491 for the Upload, #1677 for the run's own calls).
//
// The Import Run is built over fakes with `createImportRunService`, as in
// `useImportJob.test.tsx`. The login is the app's own `AuthProvider`, and what
// it calls itself stays faked at its module: the server's logout and account
// profile in `lib/serverApi`, and deleting a deleted account's run directories
// through `lib/desktop/runDir`.
//
// Every test is told when what it waits for has happened: a fake settles a
// promise the test awaits, or the test awaits the logout or the run itself.

import { act, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AuthShell from "../../components/AuthShell";
import { ApiError, getAccountId, getBaseUrl, getToken } from "../../lib/api";
import { useAuth } from "../../lib/authContext";
import type { DesktopJobResult } from "../../lib/desktop/job";
import type { UploadFinishedReport } from "../../lib/desktop/upload";
import { isUploadRunning } from "../../lib/runningUpload";
import { deferred } from "../../test/deferred";
import { advanceTimersAndSettle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { importForm } from "../../test/importForm";
import { setupUser } from "../../test/user";
import { createImportRunStore, initialImportRunState } from "./importRunStore";
import type { ImportRunDeps } from "./run/context";
import { createImportRunService, type ImportRunService } from "./run/service";
import { useImportJob } from "./useImportJob";

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
let finishUpload: ((result: DesktopJobResult) => void) | null = null;
/** Settled once the Upload has started. */
let uploadStarted = deferred();
/** Settled once the run is asked to stop, which logout does once it pauses the Upload. */
let cancelAsked = deferred();
/** Settled once logout has told the server to end the session. */
let sessionRevoked = deferred();

/**
 * The one fake for deleting a run directory. `AuthProvider` reaches it
 * through `lib/desktop/runDir` when it deletes a deleted account's Import Runs, and the
 * run reaches it through its `ImportRunDeps`. It records the call in `calls`.
 */
async function fakeDeleteRunDir({ run_dir }: { run_dir: string }): Promise<void> {
  calls.push(`deleted ${run_dir}`);
  await deleteRunDirMock(run_dir);
}

vi.mock("../../lib/desktop/runDir", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/desktop/runDir")>()),
  invokeDeleteRunDir: fakeDeleteRunDir,
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  logout: async () => {
    calls.push("session revoked");
    serverLogoutMock();
    sessionRevoked.resolve();
  },
  getAccountProfile: async () => ({ phones: [], emails: [] }),
}));

/** A dependency the paths under test never reach. */
function notCalled(name: string): () => never {
  return () => {
    throw new Error(`${name} is not called in these tests`);
  };
}

/**
 * What the run works through: the desktop app's commands and the server's
 * Import Run routes as fakes, and the session `AuthProvider` logged in.
 */
function fakeDeps(): ImportRunDeps {
  return {
    desktop: {
      isTauri: () => true,
      awaitDesktopJob: async (_job, invokeFn) => {
        await invokeFn();
        calls.push("upload started");
        uploadStarted.resolve();
        return new Promise<DesktopJobResult>((resolve) => {
          finishUpload = resolve;
        });
      },
      invokeCancel: async () => {
        calls.push("cancel");
        cancelAsked.resolve();
        cancelMock();
      },
      onDesktopJobEvents: notCalled("onDesktopJobEvents"),
      invokeCreateRunDir: notCalled("invokeCreateRunDir"),
      invokeDeleteRunDir: fakeDeleteRunDir,
      invokeExtract: notCalled("invokeExtract"),
      invokeImessageBackupIdentities: notCalled("invokeImessageBackupIdentities"),
      invokePathStat: notCalled("invokePathStat"),
      invokeReadImportRunRecord: async () => null,
      invokeSaveImportRunRecord: async () => {
        calls.push("pause recorded");
        saveRunRecordMock();
      },
      invokeStartImportRunLog: notCalled("invokeStartImportRunLog"),
      invokeSummarizeStaging: notCalled("invokeSummarizeStaging"),
      invokeToolsStatus: notCalled("invokeToolsStatus"),
      invokeTranscodeStaging: notCalled("invokeTranscodeStaging"),
      invokeUpload: async () => {},
    },
    server: {
      getServerState: notCalled("getServerState"),
      createImport: notCalled("createImport"),
      completeImport: (...args) => completeImportMock(...args),
      setImportStage: (...args) => setImportStageMock(...args),
      discardImportRun: (...args) => discardImportRunMock(...args),
      getActiveImportRun: notCalled("getActiveImportRun"),
      token: () => getToken(),
      accountId: () => getAccountId(),
      baseUrl: () => getBaseUrl(),
    },
    store: createImportRunStore(initialImportRunState([])),
    now: () => 0,
  };
}

/** The run under test, built fresh for each test. */
let service: ImportRunService;
/** The run the test started, which settles once the run has stopped. */
let running: Promise<void> | null = null;

/** A fresh run and fresh fakes, for both groups of tests below. */
function resetRun(): void {
  localStorage.clear();
  service = createImportRunService(fakeDeps());
  running = null;
  calls.length = 0;
  finishUpload = null;
  uploadStarted = deferred();
  cancelAsked = deferred();
  sessionRevoked = deferred();
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
}

/** Let a run a test left running stop, so the next test's logout does not ask. */
async function endRun(): Promise<void> {
  finishUpload?.({ summary: "Upload", report: pausedReport() });
  await act(async () => {
    await running;
  });
  expect(isUploadRunning()).toBe(false);
}

const PAUSING = "Pausing the Upload. You are logged out once it has paused, or after 15 seconds.";
const DELETED_PAUSING =
  "Your account is deleted. Stopping its Upload before you are logged out, which takes at most 15 seconds.";
const NOT_PAUSED =
  "You were logged out before the Upload had paused. When you log in again, it resumes from what it had sent.";
const ASKING = "An Upload is running. Logging out pauses it; you can resume it after you log in.";

const form = importForm();

/** What `run.rs` reports when the cancel flag pauses the Upload after 200 of 681. */
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

/**
 * The Import screen's hook beside the login state, under the real AuthProvider
 * and the logout dialog, as `App.tsx` renders them.
 */
async function logIn(): Promise<{ current: Screen }> {
  const { result } = renderHook((): Screen => ({ job: useImportJob(service), auth: useAuth() }), {
    wrapper: AuthShell,
  });
  await act(() => result.current.auth.login("http://127.0.0.1:8080", "session-token", 7));
  return result;
}

/** Log in, and resume the paused Upload of run 42 without waiting for it. */
async function resumeUpload(): Promise<{ current: Screen }> {
  const result = await logIn();
  act(() => {
    running = result.current.job.startImport(form, {
      at: "upload",
      run: {
        runId: 42,
        runDir: "/home/sam/staging-iphone",
      },
    });
  });
  return result;
}

/** Log in, and start a resumed Upload that runs until the test settles it. */
async function startUpload(): Promise<{ current: Screen }> {
  const result = await resumeUpload();
  await act(() => uploadStarted.promise);
  return result;
}

/**
 * What the account menu's Log out does: it does not wait for the logout. The
 * logout's promise is returned for the test to await once it has answered
 * what logout asks.
 */
function pressLogOut(
  result: { current: Screen },
  options?: Parameters<Screen["auth"]["logout"]>[0],
): Promise<void> {
  let loggedOut: Promise<void> = Promise.resolve();
  act(() => {
    loggedOut = result.current.auth.logout(options);
  });
  return loggedOut;
}

describe("logging out during an Upload", () => {
  beforeEach(() => {
    resetRun();
    // The Upload pauses when the cancel flag is set, the way `run.rs` does.
    cancelMock.mockImplementation(() =>
      finishUpload?.({ summary: "Upload", report: pausedReport() }),
    );
  });

  // An Upload a test left running would make the next test's logout ask.
  afterEach(endRun);

  it("asks first, pauses the Upload, and revokes the session only once the pause is recorded", async () => {
    const user = setupUser();
    const result = await startUpload();

    const loggedOut = pressLogOut(result);

    expect(screen.getByText(ASKING)).toBeTruthy();
    // Nothing happens until the person chooses.
    expect(calls).not.toContain("cancel");
    expect(serverLogoutMock).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Log out" }));
    await act(() => loggedOut);

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

    const loggedOut = pressLogOut(result);
    await user.click(screen.getByRole("button", { name: "Go back" }));
    await act(() => loggedOut);

    expect(screen.queryByText(ASKING)).toBeNull();
    expect(cancelMock).not.toHaveBeenCalled();
    expect(serverLogoutMock).not.toHaveBeenCalled();
    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(result.current.job.running).toBe(true);
  });

  it("logs out without asking when no Upload runs", async () => {
    const result = await logIn();

    await act(() => pressLogOut(result));

    expect(serverLogoutMock).toHaveBeenCalled();
    expect(screen.queryByText(/An Upload is running/)).toBeNull();
    expect(cancelMock).not.toHaveBeenCalled();
  });

  it("logs out after 15 seconds when the Upload does not pause, and says it resumes from what it sent", async () => {
    const result = await startUpload();
    // An Upload that does not stop when asked.
    cancelMock.mockImplementation(() => {});
    startFakeClock();
    try {
      const user = setupUser();
      const loggedOut = pressLogOut(result);
      await user.click(screen.getByRole("button", { name: "Log out" }));
      await act(() => cancelAsked.promise);

      expect(screen.getByText(PAUSING)).toBeTruthy();
      expect(screen.getByRole("button", { name: "Log out now" })).toBeTruthy();
      await advanceTimersAndSettle(14_000);
      expect(serverLogoutMock).not.toHaveBeenCalled();
      expect(result.current.auth.isAuthenticated).toBe(true);

      await advanceTimersAndSettle(1_500);
      await act(() => loggedOut);

      expect(serverLogoutMock).toHaveBeenCalled();
      expect(result.current.auth.isAuthenticated).toBe(false);
      expect(getToken()).toBeNull();
      expect(screen.getByText(NOT_PAUSED)).toBeTruthy();
      expect(calls).not.toContain("pause recorded");
    } finally {
      stopFakeClock();
    }
  });

  it("logs out at once on Log out now, without waiting for the Upload to pause", async () => {
    const user = setupUser();
    const result = await startUpload();
    cancelMock.mockImplementation(() => {});

    const loggedOut = pressLogOut(result);
    await user.click(screen.getByRole("button", { name: "Log out" }));
    await act(() => cancelAsked.promise);
    await user.click(screen.getByRole("button", { name: "Log out now" }));
    await act(() => loggedOut);

    expect(serverLogoutMock).toHaveBeenCalled();
    expect(result.current.auth.isAuthenticated).toBe(false);
    expect(screen.getByText(NOT_PAUSED)).toBeTruthy();
    expect(screen.queryByText(PAUSING)).toBeNull();
  });

  it("ends the session when the server refuses the Upload's session, and records no failure", async () => {
    const result = await startUpload();

    // What run.rs reports when a 401 pauses the Upload after 200 of 681.
    finishUpload?.({ summary: "Upload", report: { ...pausedReport(), session_refused: true } });
    await act(async () => {
      await running;
    });

    expect(result.current.auth.isAuthenticated).toBe(false);
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
        deletedAccountDirectories: { found: ["/home/sam/staging-iphone"], unreadable: [] },
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

    const loggedOut = pressLogOut(result, {
      ask: false,
      deletedAccountDirectories: { found: ["/home/sam/staging-iphone"], unreadable: [] },
    });
    await act(() => cancelAsked.promise);
    await user.click(screen.getByRole("button", { name: "Log out now" }));
    await act(() => sessionRevoked.promise);

    // The Upload may still write into its directory: nothing is deleted yet.
    expect(calls).not.toContain("deleted /home/sam/staging-iphone");

    finishUpload?.({ summary: "Upload", report: { ...pausedReport(), session_refused: true } });
    await act(() => loggedOut);

    expect(calls).toContain("deleted /home/sam/staging-iphone");
    expect(calls.indexOf("deleted /home/sam/staging-iphone")).toBeGreaterThan(
      calls.indexOf("pause recorded"),
    );
  });

  it("says the account is deleted while its Upload pauses", async () => {
    const result = await startUpload();
    cancelMock.mockImplementation(() => {});

    const loggedOut = pressLogOut(result, {
      ask: false,
      deletedAccountDirectories: { found: [], unreadable: [] },
    });
    await act(() => cancelAsked.promise);

    expect(screen.getByText(DELETED_PAUSING)).toBeTruthy();
    expect(screen.queryByText(PAUSING)).toBeNull();

    finishUpload?.({ summary: "Upload", report: pausedReport() });
    await act(() => loggedOut);
  });

  it("leaves a later login alone when an Upload it outlived is refused", async () => {
    const user = setupUser();
    const result = await startUpload();
    cancelMock.mockImplementation(() => {});

    const loggedOut = pressLogOut(result);
    await user.click(screen.getByRole("button", { name: "Log out" }));
    await act(() => cancelAsked.promise);
    await user.click(screen.getByRole("button", { name: "Log out now" }));
    await act(() => loggedOut);
    expect(result.current.auth.isAuthenticated).toBe(false);
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));

    // The old Upload meets its revoked session only now.
    finishUpload?.({ summary: "Upload", report: { ...pausedReport(), session_refused: true } });
    await act(async () => {
      await running;
    });
    expect(isUploadRunning()).toBe(false);

    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(getToken()).toBe("next-session-token");
  });

  it("names a directory of a deleted account's Import Runs it could not delete", async () => {
    deleteRunDirMock.mockRejectedValueOnce(new Error("permission denied"));
    const result = await logIn();

    await act(() =>
      result.current.auth.logout({
        ask: false,
        deletedAccountDirectories: {
          found: ["/home/sam/staging-iphone", "/home/sam/staging-android"],
          unreadable: [],
        },
      }),
    );

    const notice = screen.getByRole("dialog");
    expect(notice).toHaveTextContent("Message Crate could not delete");
    expect(notice).toHaveTextContent("/home/sam/staging-iphone: permission denied");
    expect(notice).not.toHaveTextContent("/home/sam/staging-android");
    expect(calls).toContain("deleted /home/sam/staging-android");
  });

  it("names a directory of a deleted account's Import Runs it may not read, and deletes the rest", async () => {
    const result = await logIn();

    await act(() =>
      result.current.auth.logout({
        ask: false,
        deletedAccountDirectories: {
          found: ["/home/sam/staging-android"],
          unreadable: [
            {
              path: "/Users/sam/Documents/staging-iphone",
              reason:
                "Message Crate isn't allowed to read it. The system says: Operation not permitted (os error 1). Give Message Crate access to it, such as Full Disk Access in System Settings.",
            },
          ],
        },
      }),
    );

    // It may not be on this computer at all, so the notice does not say it
    // could not delete it.
    const notice = screen.getByRole("dialog", {
      name: "Directories of Import Runs that may be left on this computer",
    });
    expect(notice).not.toHaveTextContent("Message Crate could not delete");
    expect(notice).toHaveTextContent(
      "Message Crate could not tell whether these directories of the deleted account's Import Runs are on this computer.",
    );
    expect(notice).toHaveTextContent(
      "/Users/sam/Documents/staging-iphone: Message Crate isn't allowed to read it. The system says: Operation not permitted (os error 1).",
    );
    expect(notice).not.toHaveTextContent("/home/sam/staging-android");
    expect(calls).toContain("deleted /home/sam/staging-android");
    expect(calls).not.toContain("deleted /Users/sam/Documents/staging-iphone");
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

describe("a session the server refuses to the Import Run's own calls (#1677)", () => {
  beforeEach(resetRun);

  afterEach(endRun);

  it("ends the session when the Upload's stage change is refused, and keeps the run for the next login", async () => {
    setImportStageMock.mockRejectedValue(sessionRefusal());
    const result = await resumeUpload();

    await act(async () => {
      await running;
    });

    expect(result.current.auth.isAuthenticated).toBe(false);
    expect(getToken()).toBeNull();
    expect(result.current.job.running).toBe(false);
    // The Upload never starts, and the run is neither completed nor deleted.
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
    const result = await startUpload();

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
    await act(async () => {
      await running;
    });

    expect(result.current.auth.isAuthenticated).toBe(false);
    expect(isUploadRunning()).toBe(false);
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

    expect(result.current.auth.isAuthenticated).toBe(false);
    // The run is still open on the server, so its directory stays for the
    // next login to offer it again.
    expect(deleteRunDirMock).not.toHaveBeenCalled();
  });

  it("leaves a later login alone when a refusal arrives after it", async () => {
    let refuse: (error: unknown) => void = () => {};
    const stageAsked = deferred();
    setImportStageMock.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          refuse = reject;
          stageAsked.resolve();
        }),
    );
    const result = await resumeUpload();
    await act(() => stageAsked.promise);
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));

    refuse(sessionRefusal());
    await act(async () => {
      await running;
    });

    expect(result.current.job.running).toBe(false);
    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(getToken()).toBe("next-session-token");
  });
});
