import { cookies } from "next/headers";

import { ACCOUNT_COOKIE, SESSION_COOKIE } from "@/lib/accountCookie";

export { ACCOUNT_COOKIE, SESSION_COOKIE };

const COOKIE_MAX_AGE = 60 * 60 * 24 * 30;

/**
 * Secure cookies for HTTPS deployments.
 * Default: on in production. Override with COOKIE_SECURE=true|false.
 */
function cookieSecure(): boolean {
  const override = process.env.COOKIE_SECURE?.trim().toLowerCase();
  if (override === "true" || override === "1") return true;
  if (override === "false" || override === "0") return false;
  return process.env.NODE_ENV === "production";
}

function cookie(name: string, value: string, maxAge: number) {
  return {
    name,
    value,
    httpOnly: true,
    sameSite: "lax" as const,
    path: "/",
    maxAge,
    secure: cookieSecure(),
  };
}

export function accountCookieOptions(accountId: string) {
  return cookie(ACCOUNT_COOKIE, accountId, COOKIE_MAX_AGE);
}

export function clearAccountCookieOptions() {
  return cookie(ACCOUNT_COOKIE, "", 0);
}

/** The Session token, sent as `Authorization: Bearer` on every `/v1` call. */
export function sessionCookieOptions(token: string) {
  return cookie(SESSION_COOKIE, token, COOKIE_MAX_AGE);
}

export function clearSessionCookieOptions() {
  return cookie(SESSION_COOKIE, "", 0);
}

export async function getAccountIdFromCookies(): Promise<string | null> {
  const store = await cookies();
  const value = store.get(ACCOUNT_COOKIE)?.value?.trim();
  return value || null;
}

export async function getSessionTokenFromCookies(): Promise<string | null> {
  const store = await cookies();
  const value = store.get(SESSION_COOKIE)?.value?.trim();
  return value || null;
}
