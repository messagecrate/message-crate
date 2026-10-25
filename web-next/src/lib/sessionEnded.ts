import type { NextRequest } from "next/server";
import { NextResponse } from "next/server";

import { SESSION_COOKIE } from "@/lib/accountCookie";
import {
  clearAccountCookieOptions,
  clearSessionCookieOptions,
} from "@/lib/session";

/**
 * Where a page sends the browser when the server refuses the Session in the
 * `mv_session` cookie. A page cannot clear a cookie while it renders, so the
 * route handler here clears it and sends the browser to `/login`.
 */
export const SESSION_ENDED_PATH = "/api/auth/session-ended";

/** Asks the server about the token (`GET /v1/session`). */
export type CheckSession = (token: string) => Promise<Response>;

/**
 * A redirect to `pathname` on the host the browser asked. The `Location` is
 * relative because a route handler's `nextUrl` names `localhost` when the
 * browser asked for `127.0.0.1`, and cookies are kept per host.
 */
function redirectTo(pathname: string): NextResponse {
  return new NextResponse(null, { status: 307, headers: { location: pathname } });
}

/**
 * Clear both session cookies and send the browser to `/login` when there is
 * no token or the server refuses it (`401`). A token the server still takes
 * is left as it is and the browser goes back to `/`, so a link to this route
 * from another site cannot sign a person out.
 */
export async function sessionEndedResponse(
  request: NextRequest,
  checkSession: CheckSession,
): Promise<NextResponse> {
  const token = request.cookies.get(SESSION_COOKIE)?.value?.trim();
  if (token) {
    const answer = await checkSession(token).catch(() => null);
    if (answer?.status !== 401) return redirectTo("/");
  }
  const res = redirectTo("/login");
  res.cookies.set(clearAccountCookieOptions());
  res.cookies.set(clearSessionCookieOptions());
  return res;
}
