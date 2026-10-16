import {
  clearAccountCookieOptions,
  clearSessionCookieOptions,
  getSessionTokenFromCookies,
} from "@/lib/session";
import { clearMemo, vaultFetch } from "@/lib/vault/client";
import { cookies } from "next/headers";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

/** End the Session (`DELETE /v1/session`) and clear the cookies. */
export async function POST() {
  const token = await getSessionTokenFromCookies();
  if (token) {
    try {
      await vaultFetch("/v1/session", { method: "DELETE", token });
    } catch {
      // The cookie is cleared either way; a dead token is harmless.
    }
  }
  clearMemo();
  const store = await cookies();
  store.set(clearAccountCookieOptions());
  store.set(clearSessionCookieOptions());
  return NextResponse.json({ ok: true });
}
