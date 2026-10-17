import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { listVaultImports } from "@/lib/vault/account";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

/** Past import runs, from `GET /v1/accounts/{id}/imports`. */
export async function GET() {
  try {
    return await withAccountHandler(async () => {
      const imports = await listVaultImports();
      return NextResponse.json({ imports });
    });
  } catch (err) {
    if (err instanceof Error && err.message === "Not signed in") {
      return unauthorizedResponse();
    }
    return NextResponse.json(
      { error: err instanceof Error ? err.message : "Couldn’t load imports." },
      { status: 500 },
    );
  }
}
