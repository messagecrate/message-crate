import {
  addHandleToContact,
  removeHandleFromContact,
} from "@/lib/contactsWrite";
import type { IdentityType } from "@/lib/handleKind";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { isHandleType } from "../../handles-body";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

type Params = { params: Promise<{ id: string }> };

function parseHandleBody(
  body: Record<string, unknown>,
): { raw: string; handle_type?: IdentityType } | null {
  // Typed form: { raw, handle_type } (legacy: bare `handle`/`phone` strings).
  const raw =
    typeof body.raw === "string"
      ? body.raw.trim()
      : typeof body.handle === "string"
        ? body.handle.trim()
        : typeof body.phone === "string"
          ? body.phone.trim()
          : "";
  if (!raw) return null;
  const rawType = body.handle_type;
  if (rawType !== undefined && !isHandleType(rawType)) {
    throw new Error("invalid handle_type");
  }
  return {
    raw,
    handle_type: rawType as IdentityType | undefined,
  };
}

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

export async function POST(req: Request, { params }: Params) {
  if (!writesAvailable()) return writesNotAvailable();
  const { id: idStr } = await params;
  const id = Number(idStr);
  if (!Number.isFinite(id)) {
    return NextResponse.json({ error: "invalid id" }, { status: 400 });
  }

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  let handle: { raw: string; handle_type?: IdentityType } | null = null;
  try {
    handle = parseHandleBody(body);
  } catch {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }
  if (!handle) {
    return NextResponse.json({ error: "handle required" }, { status: 400 });
  }

  try {
    return await withAccountHandler(async () => {
      const contact = addHandleToContact(id, handle.raw, handle.handle_type);
      return NextResponse.json({ contact });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "update failed";
    const status = message.includes("not found")
      ? 404
      : message.includes("already belongs")
        ? 409
        : 500;
    return NextResponse.json({ error: message }, { status });
  }
}

export async function DELETE(req: Request, { params }: Params) {
  if (!writesAvailable()) return writesNotAvailable();
  const { id: idStr } = await params;
  const id = Number(idStr);
  if (!Number.isFinite(id)) {
    return NextResponse.json({ error: "invalid id" }, { status: 400 });
  }

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  let handle: { raw: string; handle_type?: IdentityType } | null = null;
  try {
    handle = parseHandleBody(body);
  } catch {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }
  if (!handle) {
    return NextResponse.json({ error: "handle required" }, { status: 400 });
  }

  try {
    return await withAccountHandler(async () => {
      const contact = removeHandleFromContact(id, handle.raw, handle.handle_type);
      return NextResponse.json({ contact });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "update failed";
    const status = message.includes("not found")
      ? 404
      : message.includes("not on contact") || message.includes("cannot remove")
        ? 400
        : 500;
    return NextResponse.json({ error: message }, { status });
  }
}
