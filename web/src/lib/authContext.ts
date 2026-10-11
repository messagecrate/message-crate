/**
 * The logged-in account, as every screen and query reads it.
 *
 * Its own module, importing neither `routeQuery` nor `useAccountProfile`,
 * because both read `useAuth` and `auth.tsx` imports both: `auth.tsx` holds
 * the provider that fills this context.
 */
import { createContext, useContext } from "react";

export interface AuthState {
  serverUrl: string;
  token: string | null;
  accountId: number | null;
  isAuthenticated: boolean;
}

export interface AuthContextValue extends AuthState {
  login: (serverUrl: string, token: string, accountId: number) => Promise<void>;
  /** Save a new session token after the user changes their password. */
  updateToken: (token: string) => void;
  /**
   * Revoke the server session (best-effort) and clear the saved login.
   *
   * An Upload that is running is paused first, and the session is revoked
   * once the pause is recorded: the Upload sends this session's token. Unless
   * `ask` is false, logout first asks whether to pause it, and does nothing
   * when the person goes back. It waits at most `UPLOAD_PAUSE_LIMIT_MS`
   * (`auth.tsx`) for the pause, less when the person presses **Log out now**, and then
   * revokes the session anyway and says the Upload resumes from what it sent.
   *
   * `deletedAccountDirectories` is given when the account has just been deleted:
   * the directories of its Import Runs on this computer, deleted once the session is
   * revoked. One that cannot be deleted is named in a notice.
   */
  logout: (options?: {
    ask?: boolean;
    deletedAccountDirectories?: readonly string[];
  }) => Promise<void>;
  setServer: (url: string) => void;
  /**
   * Check the saved login again, after a startup check the server never
   * answered. `serverUrl` is the address just found reachable: a login saved
   * for any other address is left alone, so a token only ever goes to the
   * server that issued it. Does nothing when no login is saved.
   */
  retrySavedLogin: (serverUrl: string) => void;
}

export const AuthContext = createContext<AuthContextValue | null>(null);

/** Current login state. Must be called under AuthProvider. */
export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
