import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { deleteAllMessagesForAccount } from "@/lib/messagesWrite";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

export async function DELETE() {
  if (!writesAvailable()) return writesNotAvailable();
  try {
    return await withAccountHandler(async (accountId) => {
      const deleted = deleteAllMessagesForAccount(accountId);
      return NextResponse.json({ ok: true, ...deleted });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    return NextResponse.json(
      { error: "Couldn’t delete your messages." },
      { status: 500 },
    );
  }
}
