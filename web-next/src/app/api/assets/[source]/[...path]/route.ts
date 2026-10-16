import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import { vaultFetch } from "@/lib/vault/client";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

type Params = { params: Promise<{ source: string; path: string[] }> };

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

/**
 * Attachment bytes, proxied from `GET /v1/assets/{sha256}`. The server
 * addresses assets by content hash, so the last path segment is the sha256
 * (`toAttachment` in `lib/vault/messages.ts` puts it in `assetsPath`). The
 * `[source]` segment stays in the URL web-next builds; this route ignores it.
 */
export async function GET(_req: Request, { params }: Params) {
  const { path: parts } = await params;
  const sha256 = parts[parts.length - 1] ?? "";
  if (!/^[0-9a-f]{64}$/i.test(sha256)) {
    return NextResponse.json(
      { error: "asset path must end in a sha256" },
      { status: 400 },
    );
  }
  try {
    return await withAccountHandler(async () => {
      const upstream = await vaultFetch(`/v1/assets/${sha256}`);
      if (upstream.status === 401) return unauthorizedResponse();
      if (!upstream.ok) {
        return NextResponse.json(
          { error: upstream.status === 404 ? "not found" : "asset failed" },
          { status: upstream.status },
        );
      }
      const headers = new Headers();
      const type = upstream.headers.get("content-type");
      if (type) headers.set("Content-Type", type);
      const length = upstream.headers.get("content-length");
      if (length) headers.set("Content-Length", length);
      headers.set("Cache-Control", "private, max-age=31536000, immutable");
      return new NextResponse(upstream.body, { headers });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message = err instanceof Error ? err.message : "serve failed";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
