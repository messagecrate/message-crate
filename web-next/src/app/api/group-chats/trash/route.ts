import {
  permanentlyDeleteConversation,
  restoreConversation,
  trashConversation,
} from "@/lib/conversationsWrite";
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
  let body: { conversationId?: number };
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }
  const conversationId = Number(body.conversationId);
  if (!Number.isFinite(conversationId)) {
    return NextResponse.json(
      { error: "conversationId required" },
      { status: 400 },
    );
  }
  try {
    return await withAccountHandler(async () => {
      trashConversation(conversationId);
      return NextResponse.json({ ok: true, conversationId });
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
  let body: { conversationId?: number; permanent?: boolean };
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }
  const conversationId = Number(body.conversationId);
  if (!Number.isFinite(conversationId)) {
    return NextResponse.json(
      { error: "conversationId required" },
      { status: 400 },
    );
  }
  try {
    return await withAccountHandler(async () => {
      if (body.permanent) {
        permanentlyDeleteConversation(conversationId);
        return NextResponse.json({
          ok: true,
          conversationId,
          permanent: true,
        });
      }
      restoreConversation(conversationId);
      return NextResponse.json({ ok: true, conversationId });
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
