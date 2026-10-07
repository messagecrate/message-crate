import { getContact } from "@/lib/db";
import { patchContact, type ContactPatch } from "@/lib/contactsWrite";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { NextResponse } from "next/server";
import { parseHandlesBody } from "../handles-body";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

type Params = { params: Promise<{ id: string }> };

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

export async function GET(_req: Request, { params }: Params) {
  const { id: idStr } = await params;
  const id = Number(idStr);
  if (!Number.isFinite(id)) {
    return NextResponse.json({ error: "invalid id" }, { status: 400 });
  }

  try {
    return await withAccountHandler(async () => {
      const contact = await getContact(id);
      if (!contact) {
        return NextResponse.json({ error: "not found" }, { status: 404 });
      }
      return NextResponse.json({ contact });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "load failed";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}

export async function PATCH(req: Request, { params }: Params) {
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

  const patch: ContactPatch = {};
  const labelsBody = body.labels;
  if (
    Array.isArray(labelsBody) &&
    labelsBody.every((t) => typeof t === "string")
  ) {
    patch.labels = labelsBody.map((t) => t.trim()).filter(Boolean);
  }
  if (body.preferredName === null || typeof body.preferredName === "string") {
    patch.preferredName = body.preferredName;
  }
  if (body.firstName === null || typeof body.firstName === "string") {
    patch.firstName = body.firstName;
  }
  if (body.lastName === null || typeof body.lastName === "string") {
    patch.lastName = body.lastName;
  }
  if (
    Array.isArray(body.phones) &&
    body.phones.every((p) => typeof p === "string")
  ) {
    patch.phones = body.phones.map((p) => p.trim()).filter(Boolean);
  }

  let handles;
  try {
    handles = parseHandlesBody(body);
  } catch {
    return NextResponse.json({ error: "invalid handle_type" }, { status: 400 });
  }
  if (handles !== undefined) {
    patch.handles = handles;
  }

  if (
    patch.labels === undefined &&
    patch.preferredName === undefined &&
    patch.firstName === undefined &&
    patch.lastName === undefined &&
    patch.phones === undefined &&
    patch.handles === undefined
  ) {
    return NextResponse.json(
      {
        error:
          "labels, preferredName, phones, and/or handles required",
      },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async () => {
      const contact = patchContact(id, patch);
      return NextResponse.json({ contact });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "update failed";
    const status = message.includes("not found") ? 404 : 500;
    return NextResponse.json({ error: message }, { status });
  }
}
