import { INVALID_CREDENTIALS } from "@/lib/accounts";
import {
  accountCookieOptions,
  sessionCookieOptions,
} from "@/lib/session";
import {
  clearMemo,
  failureMessage,
  vaultFetch,
  type Schemas,
} from "@/lib/vault/client";
import { cookies } from "next/headers";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

/** Sign in through `POST /v1/session`; the session token goes in a cookie. */
export async function POST(req: Request) {
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  const username = typeof body.username === "string" ? body.username.trim() : "";
  const password = typeof body.password === "string" ? body.password : "";

  if (!username) {
    return NextResponse.json({ error: INVALID_CREDENTIALS }, { status: 401 });
  }

  let res: Response;
  try {
    res = await vaultFetch("/v1/session", {
      method: "POST",
      body: { username, password },
      token: null,
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : "vault unreachable";
    return NextResponse.json(
      { error: `Couldn’t reach the vault: ${message}` },
      { status: 502 },
    );
  }
  if (res.status === 401 || res.status === 403) {
    return NextResponse.json({ error: INVALID_CREDENTIALS }, { status: 401 });
  }
  if (!res.ok) {
    return NextResponse.json(
      { error: await failureMessage(res) },
      { status: res.status },
    );
  }

  const session = (await res.json()) as Schemas["CreateSessionResponse"];
  clearMemo();
  const store = await cookies();
  store.set(accountCookieOptions(String(session.account_id)));
  store.set(sessionCookieOptions(session.token));

  return NextResponse.json({
    id: session.account_id,
    username: session.username,
  });
}
