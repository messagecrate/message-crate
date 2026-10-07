import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import {
  restoreMessageThreads,
  trashMessageThreads,
  type MessageTrashTargets,
} from "@/lib/messageTrashWrite";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function parseTargets(body: Record<string, unknown>): MessageTrashTargets | null {
  if (
    (body.handles !== undefined &&
      (!Array.isArray(body.handles) ||
        !body.handles.every((handle) => typeof handle === "string"))) ||
    (body.conversationIds !== undefined &&
      (!Array.isArray(body.conversationIds) ||
        !body.conversationIds.every(
          (id) => typeof id === "number" && Number.isFinite(id),
        )))
  ) {
    return null;
  }

  const handles = ((body.handles as string[] | undefined) ?? [])
    .map((handle) => handle.trim())
    .filter(Boolean);
  const conversationIds = (body.conversationIds as number[] | undefined) ?? [];
  if (handles.length + conversationIds.length === 0) return null;
  return { handles, conversationIds };
}

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

async function mutate(req: Request, restore: boolean): Promise<NextResponse> {
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid json" }, { status: 400 });
  }

  const targets = parseTargets(body);
  if (!targets) {
    return NextResponse.json(
      { error: "handles or conversationIds required" },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      const result = restore
        ? restoreMessageThreads(targets)
        : trashMessageThreads(targets);
      return NextResponse.json({ ok: true, ...result });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message =
      err instanceof Error
        ? err.message
        : restore
          ? "restore failed"
          : "trash failed";
    return NextResponse.json(
      { error: message },
      {
        status: message.includes("not found") ? 404 : 400,
      },
    );
  }
}

export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  return mutate(req, false);
}

export async function DELETE(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  return mutate(req, true);
}
