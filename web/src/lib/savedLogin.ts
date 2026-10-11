/**
 * The login saved between starts, and the check that it still works.
 *
 * Two settings are kept: the saved login (server address, token, account id)
 * and the server address on its own. Passwords are never stored.
 */
import { ApiError } from "./api";
import type { AuthState } from "./authContext";
import { type ParsedPersistedAuth, parsePersistedAuth } from "./authGuards";
import { getSession } from "./serverApi";
import { readPref, removePref, writePref } from "./storage";

const SAVED_LOGIN_KEY = "message-crate-auth";

/**
 * The server address, a setting of its own. It is kept apart from the saved
 * login so that logging out, or a session the server refuses, leaves it as
 * it is: the next start opens on the login card for the same Message Crate,
 * and the desktop app starts its own only when this is its own address
 * (#1972).
 */
const SERVER_ADDRESS_KEY = "message-crate-server-address";

/** The saved login, or null when none is saved or it cannot be read. */
export function loadSavedLogin(): ParsedPersistedAuth | null {
  const raw = readPref(SAVED_LOGIN_KEY);
  if (!raw) return null;
  return parsePersistedAuth(raw);
}

/** Save `state` as the login for the next start. */
export function saveLogin(state: AuthState): void {
  writePref(
    SAVED_LOGIN_KEY,
    JSON.stringify({
      serverUrl: state.serverUrl,
      token: state.token,
      accountId: state.accountId,
    }),
  );
}

/** Delete the saved login. The server address stays: it is a setting. */
export function forgetSavedLogin(): void {
  removePref(SAVED_LOGIN_KEY);
}

/**
 * The saved server address. Empty when none is saved, or when the saved one
 * is the website's own origin.
 */
export function loadServerAddress(): string {
  return readPref(SERVER_ADDRESS_KEY) ?? "";
}

/** Save the server address for the next start. */
export function saveServerAddress(url: string): void {
  writePref(SERVER_ADDRESS_KEY, url);
}

/**
 * The login the app starts with: the saved login when it was saved for the
 * saved server address, else logged out at that address. A login saved for
 * another address is left alone, so a token only ever goes to the server
 * that issued it. An empty address means "same host as this page".
 */
export function startingAuthState(): AuthState {
  const serverUrl = loadServerAddress();
  const saved = loadSavedLogin();
  if (saved && saved.serverUrl === serverUrl) {
    return { ...saved, isAuthenticated: true };
  }
  return { serverUrl, token: null, accountId: null, isAuthenticated: false };
}

/**
 * What the server said about a saved login: it still works, it refused it,
 * or it gave no answer about it.
 */
export type SavedLoginCheck = "valid" | "refused" | "unanswered";

/**
 * Ask the server whether the session `api.ts` now sends still works.
 *
 * Only the server can say a token is no longer valid, and it says so with a
 * 401. A request nothing answered, or a 502 from a proxy, says nothing about
 * the token.
 */
export async function checkSavedLogin(): Promise<SavedLoginCheck> {
  try {
    await getSession();
    return "valid";
  } catch (err) {
    return err instanceof ApiError && err.status === 401 ? "refused" : "unanswered";
  }
}
