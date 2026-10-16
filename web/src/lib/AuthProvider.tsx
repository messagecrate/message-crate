import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getToken } from "./api";
import { applyApiSession, useApiSession } from "./apiSession";
import { AuthContext, type AuthState, type LoginLifetime } from "./authContext";
import { type AccountRunDirectories, deleteRunDirectories } from "./importRun";
import { LogoutPromptContext, type LogoutPromptState } from "./logoutPromptContext";
import { isUploadRunning, pauseRunningUpload } from "./runningUpload";
import {
  checkSavedLogin,
  forgetSavedLogin,
  loadSavedLogin,
  savedLoginLifetime,
  saveLogin,
  saveServerAddress,
  startingAuthState,
} from "./savedLogin";
import { logout as serverLogout } from "./serverApi";
import { SessionQueryClientProvider } from "./sessionQueryClient";
import { onSessionRefused } from "./sessionRefusal";
import { warmAccountProfile } from "./useAccountProfile";
import { useLogoutOnWindowClose } from "./windowCloseLogout";

/**
 * How long logout waits for a running Upload to pause before it revokes the
 * session anyway (#1491). An Upload that does not stop must not keep the person
 * logged in; the Upload then resumes from what its journal recorded as sent.
 */
const UPLOAD_PAUSE_LIMIT_MS = 15_000;

/** Max time to wait for the server logout request before clearing local state. */
const LOGOUT_TIMEOUT_MS = 2000;

/** AbortSignal that fires after {@link LOGOUT_TIMEOUT_MS}. */
function logoutTimeoutSignal(): AbortSignal {
  if (typeof AbortSignal !== "undefined" && typeof AbortSignal.timeout === "function") {
    return AbortSignal.timeout(LOGOUT_TIMEOUT_MS);
  }
  const controller = new AbortController();
  setTimeout(() => controller.abort(), LOGOUT_TIMEOUT_MS);
  return controller.signal;
}

/**
 * Holds login state for the app and shares it through `useAuth`.
 *
 * It also holds what logging out has to show, for `LogoutDialog` to read
 * from `useLogoutPrompt`; `AuthShell` places that dialog, not this provider.
 *
 * The rest of the login lives beside it: the query client in
 * `sessionQueryClient.tsx`, keeping `api.ts` on the login in `apiSession.ts`,
 * the saved login in `savedLogin.ts`, and logging out when the desktop
 * window closes in `windowCloseLogout.ts`.
 */
export function AuthProvider({ children }: { children: ReactNode }) {
  return (
    <SessionQueryClientProvider>
      <SessionProvider>{children}</SessionProvider>
    </SessionQueryClientProvider>
  );
}

function SessionProvider({ children }: { children: ReactNode }) {
  // Talking to the client directly rather than through `routeQuery`'s hooks,
  // which read `useAuth` from this provider.
  const queryClient = useQueryClient();
  const resetRouteCache = useCallback(() => {
    queryClient.clear();
  }, [queryClient]);
  const [restored, setRestored] = useState(false);
  // Incremented on login and logout so an older profile request is ignored.
  const authEpoch = useRef(0);
  const [state, setState] = useState<AuthState>(() => {
    const starting = startingAuthState();
    // Apply before children mount. Otherwise Contact Groups loads without
    // a token, fails, and the sidebar stays on "No group" only.
    if (starting.isAuthenticated) applyApiSession(starting);
    return starting;
  });

  useApiSession(state);

  // Check that the restored token still works.
  useEffect(() => {
    const { serverUrl, token, accountId } = state;
    if (!state.isAuthenticated || restored || token === null || accountId === null) return;
    const login = { serverUrl, token, accountId };

    let cancelled = false;
    const validate = async () => {
      const check = await checkSavedLogin(login);
      if (cancelled) return;
      if (check === "valid") {
        // Warm the profile before the app renders. Whether this account still
        // owes profile setup is read from it, so fetching it here keeps the
        // guards from deciding against an empty cache.
        await warmAccountProfile(queryClient, login.accountId);
        if (!cancelled) setRestored(true);
        return;
      }
      // A login the server gave no answer about stays saved for
      // `retrySavedLogin`. Either way this session shows the login screen.
      authEpoch.current++;
      applyApiSession({ token: null, accountId: null });
      if (check === "refused") forgetSavedLogin();
      setState((s) => ({
        ...s,
        token: null,
        accountId: null,
        isAuthenticated: false,
      }));
      setRestored(true);
    };
    validate();
    return () => {
      cancelled = true;
    };
  }, [state, restored, queryClient]);

  const setServer = useCallback((url: string) => {
    applyApiSession({ serverUrl: url });
    saveServerAddress(url);
    setState((s) => ({ ...s, serverUrl: url }));
  }, []);

  const retrySavedLogin = useCallback((serverUrl: string) => {
    const saved = loadSavedLogin();
    if (!saved || saved.serverUrl !== serverUrl) return;
    // Back to the state the app starts in, which runs the check above again.
    setState({ ...saved, isAuthenticated: true });
    setRestored(false);
  }, []);

  const login = useCallback(
    async (
      serverUrl: string,
      token: string,
      accountId: number,
      lifetime: LoginLifetime = "kept",
    ) => {
      const epoch = ++authEpoch.current;
      // One call, and it cannot be incomplete: every cached entry is named
      // with the account that filled it, so this only releases memory.
      resetRouteCache();
      // The profile is the account's own row, addressed by this id, so the
      // client must know it before the fetch below.
      applyApiSession({ serverUrl, token, accountId });

      // Fetch the profile before the app renders: it carries whether this
      // account still owes profile setup or a password change, and the guards
      // read it from there rather than from anything decided here.
      await warmAccountProfile(queryClient, accountId);

      if (authEpoch.current !== epoch) return; // A later login or logout replaced this one.

      const newState: AuthState = {
        serverUrl,
        token,
        accountId,
        isAuthenticated: true,
      };
      saveServerAddress(serverUrl);
      saveLogin(newState, lifetime);
      setState(newState);
      setRestored(true);
    },
    [resetRouteCache, queryClient],
  );

  const updateToken = useCallback((token: string) => {
    applyApiSession({ token });
    setState((s) => {
      if (!s.isAuthenticated) return s;
      const next: AuthState = { ...s, token };
      // The new token is kept where the login it replaces was.
      saveLogin(next, savedLoginLifetime());
      return next;
    });
  }, []);

  /**
   * Forget the login on this side: token, cached data, and the saved login.
   * The server address stays: it is a setting, not part of the login.
   */
  const clearSession = useCallback(() => {
    authEpoch.current++;
    applyApiSession({ token: null, accountId: null });
    resetRouteCache();
    forgetSavedLogin();
    setState((s) => ({
      ...s,
      token: null,
      accountId: null,
      isAuthenticated: false,
    }));
  }, [resetRouteCache]);

  /** Tell the server to end the session, then forget it here. */
  const revokeSession = useCallback(async () => {
    authEpoch.current++;
    // Tell the server to end the session while the token is still set on the API client.
    // Await so close-to-quit can finish (or time out) before the WebView dies.
    if (getToken()) {
      try {
        await serverLogout({ signal: logoutTimeoutSignal() });
      } catch {
        // Server unreachable, 401, or timeout — still clear the local session.
      }
    }
    clearSession();
  }, [clearSession]);

  // Where logout is, for `LogoutDialog`: asking while an Upload runs, waiting
  // while that Upload pauses, or what it has to say once logged out.
  // `answerRef` settles the logout waiting on the question; `logOutNowRef`
  // ends the wait.
  const [prompt, setPrompt] = useState<LogoutPromptState | null>(null);
  const answerRef = useRef<(logOut: boolean) => void>(() => {});
  const logOutNowRef = useRef<() => void>(() => {});

  /**
   * Pause the running Upload. `paused` resolves to whether it paused before
   * the limit and before the person pressed **Log out now**; `ended` once it
   * has ended, however long that takes. A pause that fails counts as done:
   * the session is revoked either way.
   */
  const pauseWithinLimit = useCallback(() => {
    const ended = pauseRunningUpload().catch(() => {});
    const paused = new Promise<boolean>((resolve) => {
      const settle = (done: boolean) => {
        clearTimeout(timer);
        logOutNowRef.current = () => {};
        resolve(done);
      };
      const timer = setTimeout(() => settle(false), UPLOAD_PAUSE_LIMIT_MS);
      logOutNowRef.current = () => settle(false);
      void ended.then(() => settle(true));
    });
    return { paused, ended };
  }, []);

  const logout = useCallback(
    async ({
      ask = true,
      deletedAccountDirectories,
    }: {
      ask?: boolean;
      deletedAccountDirectories?: AccountRunDirectories;
    } = {}) => {
      const uploadRunning = isUploadRunning();
      if (ask && uploadRunning) {
        const logOut = await new Promise<boolean>((resolve) => {
          answerRef.current = resolve;
          setPrompt({ kind: "asking" });
        });
        if (!logOut) return;
      }
      if (uploadRunning) {
        setPrompt({ kind: "pausing", accountDeleted: deletedAccountDirectories !== undefined });
      }
      const pause = pauseWithinLimit();
      let paused = true;
      try {
        paused = await pause.paused;
        await revokeSession();
      } finally {
        setPrompt(null);
      }
      if (deletedAccountDirectories) {
        // The run went with the account, so nothing resumes; its directories go
        // too, and one that stays is named rather than left without a word.
        // An Upload that did not pause in time may still write into its
        // directory, so the directories go once it has ended. The deleted account's
        // token is refused, so that is soon.
        await pause.ended;
        // One Message Crate may not read cannot be deleted either, so it is
        // named too, apart from the rest: it may not be on this computer.
        const undeleted = await deleteRunDirectories(deletedAccountDirectories.found);
        const { unreadable } = deletedAccountDirectories;
        if (undeleted.length > 0 || unreadable.length > 0) {
          setPrompt({ kind: "leftDirectories", undeleted, unreadable });
        }
      } else if (!paused) {
        setPrompt({ kind: "notPaused" });
      }
    },
    [revokeSession, pauseWithinLimit],
  );

  // The server has refused the token, so the session is already over there
  // and there is nothing to tell it. With no token set, the 401 came from a
  // request made before login, and there is no session here to end either.
  // An Upload that is running is paused all the same: it sends the
  // same token, so it would only record every remaining conversation as
  // failed, and the run stays resumable.
  // A session the server refused to an Import Run, its Upload or its own
  // calls, ends the same way, unless a later login has replaced the token the
  // run sent: a refusal that outlived a logout says nothing about the session
  // after it.
  useEffect(() => {
    const end = (refusedToken?: string) => {
      const token = getToken();
      if (!token || (refusedToken !== undefined && refusedToken !== token)) return;
      void pauseRunningUpload();
      clearSession();
    };
    return onSessionRefused(end);
  }, [clearSession]);

  useLogoutOnWindowClose(revokeSession);

  const logoutPrompt = useMemo(
    () => ({
      state: prompt,
      logOut: () => answerRef.current(true),
      goBack: () => {
        setPrompt(null);
        answerRef.current(false);
      },
      logOutNow: () => logOutNowRef.current(),
      dismiss: () => setPrompt(null),
    }),
    [prompt],
  );

  return (
    <AuthContext.Provider
      value={{ ...state, login, logout, updateToken, setServer, retrySavedLogin }}
    >
      <LogoutPromptContext.Provider value={logoutPrompt}>{children}</LogoutPromptContext.Provider>
    </AuthContext.Provider>
  );
}
