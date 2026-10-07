import { mergeContacts } from "@/lib/contactsWrite";
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

export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  const fromId =
    typeof body.fromId === "number" && Number.isFinite(body.fromId)
      ? body.fromId
      : null;
  const intoId =
    typeof body.intoId === "number" && Number.isFinite(body.intoId)
      ? body.intoId
      : null;
  if (fromId == null || intoId == null) {
    return NextResponse.json(
      { error: "fromId and intoId required" },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      const contact = mergeContacts(fromId, intoId);
      return NextResponse.json({ contact });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "merge failed";
    const status = message.includes("not found") ||
      message.includes("cannot merge") ||
      message.includes("only nameless") ||
      message.includes("must have a name") ||
      message.includes("already belongs")
      ? 400
      : 500;
    return NextResponse.json({ error: message }, { status });
  }
}
