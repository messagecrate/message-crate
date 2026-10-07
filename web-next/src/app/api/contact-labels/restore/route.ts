import { restoreLabel } from "@/lib/contactsWrite";
import { listLabels } from "@/lib/db";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

/** Recreate a deleted group and re-attach member contacts (undo delete group). */
export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: { name?: unknown; memberContactIds?: unknown };
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  if (typeof body.name !== "string") {
    return NextResponse.json({ error: "name required" }, { status: 400 });
  }

  const memberContactIds = Array.isArray(body.memberContactIds)
    ? body.memberContactIds.filter(
        (id): id is number => typeof id === "number" && Number.isFinite(id),
      )
    : [];

  try {
    return await withAccountHandler(async () => {
      const name = restoreLabel(body.name as string, memberContactIds);
      return NextResponse.json({ name, labels: listLabels() });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "restore failed";
    const status = message.includes("already exists") ? 409 : 400;
    return NextResponse.json({ error: message }, { status });
  }
}
