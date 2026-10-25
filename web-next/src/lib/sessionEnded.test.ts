import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { NextRequest } from "next/server";

import { ACCOUNT_COOKIE, SESSION_COOKIE } from "./accountCookie";
import {
  type CheckSession,
  SESSION_ENDED_PATH,
  sessionEndedResponse,
} from "./sessionEnded";

function request(cookies: Record<string, string>): NextRequest {
  const cookie = Object.entries(cookies)
    .map(([name, value]) => `${name}=${value}`)
    .join("; ");
  return new NextRequest(`http://127.0.0.1:3000${SESSION_ENDED_PATH}`, {
    headers: cookie ? { cookie } : {},
  });
}

/** The `Set-Cookie` lines that clear a cookie, by name. */
function cleared(res: Response): string[] {
  return res.headers
    .getSetCookie()
    .filter((line) => /Max-Age=0/i.test(line))
    .map((line) => line.split("=", 1)[0]!)
    .sort();
}

// The `Location` is a path alone, so the browser stays on the host it asked.
describe("sessionEndedResponse", () => {
  it("clears both session cookies and goes to /login when the server refuses the token", async () => {
    const asked: string[] = [];
    const check: CheckSession = async (token) => {
      asked.push(token);
      return new Response(null, { status: 401 });
    };
    const res = await sessionEndedResponse(
      request({ [ACCOUNT_COOKIE]: "7", [SESSION_COOKIE]: "mc-user-stale" }),
      check,
    );
    assert.deepEqual(asked, ["mc-user-stale"]);
    assert.equal(res.status, 307);
    assert.equal(res.headers.get("location"), "/login");
    assert.deepEqual(cleared(res), [ACCOUNT_COOKIE, SESSION_COOKIE].sort());
  });

  it("clears the cookies without asking when there is no token", async () => {
    let asked = false;
    const res = await sessionEndedResponse(request({ [ACCOUNT_COOKIE]: "7" }), async () => {
      asked = true;
      return new Response(null, { status: 200 });
    });
    assert.equal(asked, false);
    assert.equal(res.headers.get("location"), "/login");
    assert.deepEqual(cleared(res), [ACCOUNT_COOKIE, SESSION_COOKIE].sort());
  });

  it("keeps a token the server still takes and goes back to /", async () => {
    const res = await sessionEndedResponse(
      request({ [ACCOUNT_COOKIE]: "7", [SESSION_COOKIE]: "mc-user-live" }),
      async () => new Response("{}", { status: 200 }),
    );
    assert.equal(res.headers.get("location"), "/");
    assert.deepEqual(res.headers.getSetCookie(), []);
  });
});
