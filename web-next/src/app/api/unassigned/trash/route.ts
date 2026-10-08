import {
  permanentlyDeleteHandle,
  restoreHandle,
  trashHandle,
} from "@/lib/handlesWrite";
import type { IdentityType } from "@/lib/handleKind";
import { ensureUnknownContacts } from "@/lib/contactsWrite";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { isHandleType } from "../../contacts/handles-body";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

type TrashBody = { handle?: string; handle_type?: unknown; permanent?: boolean };

function parseBody(body: TrashBody): {
  handle: string;
  handleType: IdentityType | undefined;
} | null {
  const handle = body.handle?.trim() ?? "";
  if (!handle) return null;
  const rawType = body.handle_type;
  if (rawType !== undefined && !isHandleType(rawType)) {
    throw new Error("invalid handle_type");
  }
  return { handle, handleType: rawType as IdentityType | undefined };
}

export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: TrashBody;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }
  let parsed: { handle: string; handleType: IdentityType | undefined } | null = null;
  try {
    parsed = parseBody(body);
  } catch {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }
  if (!parsed) {
    return NextResponse.json({ error: "handle required" }, { status: 400 });
  }
  const { handle, handleType } = parsed;
  try {
    return await withAccountHandler(async () => {
      trashHandle(handle, handleType);
      return NextResponse.json({ ok: true, handle });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "trash failed";
    return NextResponse.json(
      { error: message },
      { status: 400 },
    );
  }
}

export async function DELETE(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: TrashBody;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }
  let parsed: { handle: string; handleType: IdentityType | undefined } | null = null;
  try {
    parsed = parseBody(body);
  } catch {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }
  if (!parsed) {
    return NextResponse.json({ error: "handle required" }, { status: 400 });
  }
  const { handle, handleType } = parsed;
  try {
    return await withAccountHandler(async () => {
      if (body.permanent) {
        permanentlyDeleteHandle(handle, handleType);
        return NextResponse.json({ ok: true, handle, permanent: true });
      }
      restoreHandle(handle, handleType);
      // Promote restored unassigned handles to nameless contacts (no Unassigned UI).
      ensureUnknownContacts();
      return NextResponse.json({ ok: true, handle });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message =
      err instanceof Error
        ? err.message
        : body.permanent
          ? "delete forever failed"
          : "restore failed";
    return NextResponse.json(
      { error: message },
      { status: 400 },
    );
  }
}
