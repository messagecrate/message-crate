/**
 * The login, as `api.ts` sends it: the server address every request goes to,
 * the session token it carries, and the account id cache entries are named
 * after.
 *
 * `AuthProvider` holds the login in React state, and `api.ts` is read outside
 * React, so every change is pushed across here.
 */
import { setAccountId, setBaseUrl, setToken } from "./api";

export interface ApiSession {
  serverUrl: string;
  token: string | null;
  accountId: number | null;
}

/**
 * Point `api.ts` at `session` now, before the next request. A field left out
 * keeps the value it has.
 */
export function applyApiSession(session: Partial<ApiSession>): void {
  if (session.serverUrl !== undefined) setBaseUrl(session.serverUrl);
  if (session.token !== undefined) setToken(session.token);
  if (session.accountId !== undefined) setAccountId(session.accountId);
}
