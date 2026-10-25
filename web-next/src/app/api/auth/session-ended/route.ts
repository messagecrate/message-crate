import type { NextRequest } from "next/server";

import { sessionEndedResponse } from "@/lib/sessionEnded";
import { vaultFetch } from "@/lib/vault/client";

export const runtime = "nodejs";

/**
 * A page's read was refused for the Session in the cookie: clear the session
 * cookies and go to `/login` (`lib/sessionEnded.ts`).
 */
export async function GET(request: NextRequest) {
  return sessionEndedResponse(request, (token) =>
    vaultFetch("/v1/session", { token }),
  );
}
