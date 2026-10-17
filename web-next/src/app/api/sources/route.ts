import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { listSources } from "@/lib/vault/account";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

/** Import source ids, from `GET /v1/session`. */
export async function GET() {
  try {
    return await withAccountHandler(async () => {
      const sources = (await listSources()).map((id) => ({ id }));
      return NextResponse.json({ sources });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "load failed";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
