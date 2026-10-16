import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { loadStorageUsage } from "@/lib/vault/account";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

/** Attachment usage, from `GET /v1/accounts/{id}/storage`. */
export async function GET() {
  try {
    return await withAccountHandler(async () => {
      const usage = await loadStorageUsage();
      return NextResponse.json(usage);
    });
  } catch (err) {
    if (err instanceof Error && err.message === "Not signed in") {
      return unauthorizedResponse();
    }
    return NextResponse.json(
      { error: err instanceof Error ? err.message : "Couldn’t load usage." },
      { status: 500 },
    );
  }
}
