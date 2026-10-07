import { createContact, deleteContacts } from "@/lib/contactsWrite";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { parseHandlesBody } from "./handles-body";
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

  const preferredName =
    body.preferredName === null || typeof body.preferredName === "string"
      ? body.preferredName
      : undefined;
  const firstName =
    body.firstName === null || typeof body.firstName === "string"
      ? body.firstName
      : undefined;
  const lastName =
    body.lastName === null || typeof body.lastName === "string"
      ? body.lastName
      : undefined;
  const phones =
    Array.isArray(body.phones) && body.phones.every((p) => typeof p === "string")
      ? body.phones.map((p) => p.trim()).filter(Boolean)
      : undefined;
  const labelsBody = body.labels;
  const labels =
    Array.isArray(labelsBody) && labelsBody.every((t) => typeof t === "string")
      ? labelsBody.map((t) => t.trim()).filter(Boolean)
      : undefined;

  let handles;
  try {
    handles = parseHandlesBody(body);
  } catch {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }

  try {
    return await withAccountHandler(async () => {
      const contact = createContact({
        preferredName,
        firstName,
        lastName,
        // Typed handles win over the legacy string list.
        handles,
        phones,
        labels,
      });
      return NextResponse.json({ contact });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "create failed";
    const status = message.includes("required") || message.includes("already belongs")
      ? 400
      : 500;
    return NextResponse.json({ error: message }, { status });
  }
}

export async function DELETE(req: Request) {
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
      ? (body.ids as number[])
      : null;
  if (!ids || ids.length === 0) {
    return NextResponse.json({ error: "ids required" }, { status: 400 });
  }

  try {
    return await withAccountHandler(async () => {
      const deleted = deleteContacts(ids);
      return NextResponse.json({ deleted });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "delete failed";
    const status = message.includes("not found") ? 404 : 500;
    return NextResponse.json({ error: message }, { status });
  }
}
