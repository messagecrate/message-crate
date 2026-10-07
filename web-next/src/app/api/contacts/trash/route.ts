import {
  permanentlyDeleteTrashedContacts,
  restoreTrashedContacts,
  trashContactMessagesOnly,
  trashContactWithMessages,
} from "@/lib/contactsTrash";
import {
  permanentlyDeleteHandle,
  restoreHandle,
} from "@/lib/handlesWrite";
import type { HandleType } from "@/lib/handleKind";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { isHandleType } from "../handles-body";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function parseIds(body: Record<string, unknown>): number[] | null {
  if (
    !Array.isArray(body.ids) ||
    !body.ids.every((id) => typeof id === "number" && Number.isFinite(id))
  ) {
    return null;
  }
  return body.ids as number[];
}

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
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }

  const ids = parseIds(body);
  if (!ids || ids.length === 0) {
    return NextResponse.json({ error: "ids required" }, { status: 400 });
  }

  const mode = body.mode;
  if (mode !== "contact_and_messages" && mode !== "messages_only") {
    return NextResponse.json(
      { error: "mode must be contact_and_messages or messages_only" },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      if (mode === "contact_and_messages") {
        const count = trashContactWithMessages(ids);
        return NextResponse.json({ ok: true, count, mode });
      }
      const { count, handles } = trashContactMessagesOnly(ids);
      return NextResponse.json({ ok: true, count, mode, handles });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "trash failed";
    const status = message.includes("not found") ? 404 : 400;
    return NextResponse.json({ error: message }, { status });
  }
}

export async function DELETE(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }

  const permanent = body.permanent === true;
  const ids = parseIds(body);
  const handle =
    typeof body.handle === "string" ? body.handle.trim() : "";
  const rawType = body.handle_type;
  if (handle && rawType !== undefined && !isHandleType(rawType)) {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }
  const handleType = (rawType as HandleType | undefined) ?? undefined;

  if (handle) {
    try {
      return await withAccountHandler(async () => {
        if (permanent) {
          permanentlyDeleteHandle(handle, handleType);
          return NextResponse.json({ ok: true, handle, permanent: true });
        }
        restoreHandle(handle, handleType);
        return NextResponse.json({ ok: true, handle });
      });
    } catch (err) {
      const auth = authError(err);
      if (auth) return auth;
      const message =
        err instanceof Error
          ? err.message
          : permanent
            ? "delete forever failed"
            : "restore failed";
      return NextResponse.json(
        { error: message },
        { status: 400 },
      );
    }
  }

  if (!ids || ids.length === 0) {
    return NextResponse.json(
      { error: "ids or handle required" },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      if (permanent) {
        const count = permanentlyDeleteTrashedContacts(ids);
        return NextResponse.json({ ok: true, count, permanent: true });
      }
      const count = restoreTrashedContacts(ids);
      return NextResponse.json({ ok: true, count });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message =
      err instanceof Error
        ? err.message
        : permanent
          ? "delete forever failed"
          : "restore failed";
    const status = message.includes("not in trash") ? 400 : 500;
    return NextResponse.json({ error: message }, { status });
  }
}
