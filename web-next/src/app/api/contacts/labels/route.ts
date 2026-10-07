import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { setContactsLabelMembership } from "@/lib/contactsWrite";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  const ids =
    Array.isArray(body.ids) &&
    body.ids.every((id) => typeof id === "number" && Number.isFinite(id))
      ? [...new Set(body.ids as number[])]
      : null;
  const name = typeof body.name === "string" ? body.name.trim() : "";
  const enable = typeof body.enable === "boolean" ? body.enable : null;
  if (!ids?.length || !name || enable == null) {
    return NextResponse.json(
      { error: "ids, name, and enable required" },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      const changed = setContactsLabelMembership(ids, name, enable);
      return NextResponse.json({ changed });
    });
  } catch (err) {
    if (err instanceof Error && err.message === "Not signed in") {
      return unauthorizedResponse();
    }
    const message = err instanceof Error ? err.message : "update failed";
    return NextResponse.json(
      { error: message },
      { status: 500 },
    );
  }
}
