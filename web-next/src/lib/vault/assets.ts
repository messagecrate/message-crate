/**
 * Attachment bytes, proxied from the server's asset routes: the original at
 * `GET /v1/assets/{sha256}`, the Preview at `/v1/assets/{sha256}/preview`
 * and the Thumbnail at `/v1/assets/{sha256}/thumbnail`.
 *
 * The browser cannot send the session token itself (it lives in an httpOnly
 * cookie), so the route handlers under `app/api/assets*` call
 * {@link assetRoute}, which reads the asset with the token and streams the
 * answer back.
 */
import { NextResponse } from "next/server";

import { unauthorizedResponse, withAccountHandler } from "@/lib/accountContext";

import { vaultFetch } from "./client";

/** Which version of an attachment to read. */
export type AssetVersion = "original" | "preview" | "thumbnail";

/** The `/v1` path of one version of an attachment. */
export function assetPath(sha256: string, version: AssetVersion): string {
  const base = `/v1/assets/${sha256}`;
  return version === "original" ? base : `${base}/${version}`;
}

/**
 * The sha256 a web-next asset URL ends in, or null. The URL is
 * `/api/assets…/{source}/{sha256}`: `toAttachment` in `lib/vault/messages.ts`
 * puts the sha256 in the path fields, and the `[source]` segment is ignored.
 */
export function sha256FromPath(parts: string[]): string | null {
  const last = parts[parts.length - 1] ?? "";
  return /^[0-9a-f]{64}$/i.test(last) ? last : null;
}

/**
 * Read one version of an attachment and answer it to the browser.
 *
 * A Preview or Thumbnail the server does not hold answers `404 Not Found`,
 * as when it has not made one yet. The original is answered in its place, so
 * the attachment still shows. That answer is not cached, so the Preview or
 * Thumbnail shows once the server has made it.
 */
export async function proxyAsset(
  sha256: string,
  version: AssetVersion,
  fetchPath: (path: string) => Promise<Response> = (path) => vaultFetch(path),
): Promise<Response> {
  let upstream = await fetchPath(assetPath(sha256, version));
  let fellBack = false;
  if (upstream.status === 404 && version !== "original") {
    // Read the small problem body to its end rather than cancel it: inside a
    // Next.js route handler, `body.cancel()` on this answer never settled and
    // the request hung, though plain Node.js settles it at once.
    await upstream.arrayBuffer();
    upstream = await fetchPath(assetPath(sha256, "original"));
    fellBack = true;
  }
  if (upstream.status === 401) return unauthorizedResponse();
  if (!upstream.ok) {
    await upstream.arrayBuffer();
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
  headers.set(
    "Cache-Control",
    fellBack ? "private, no-store" : "private, max-age=31536000, immutable",
  );
  return new NextResponse(upstream.body, { headers });
}

type Params = { params: Promise<{ source: string; path: string[] }> };

/** The `GET` handler of an `app/api/assets*` route for one version. */
export function assetRoute(version: AssetVersion) {
  return async function GET(_req: Request, { params }: Params) {
    const { path: parts } = await params;
    const sha256 = sha256FromPath(parts);
    if (!sha256) {
      return NextResponse.json(
        { error: "asset path must end in a sha256" },
        { status: 400 },
      );
    }
    try {
      return await withAccountHandler(() => proxyAsset(sha256, version));
    } catch (err) {
      if (err instanceof Error && err.message === "Not signed in") {
        return unauthorizedResponse();
      }
      const message = err instanceof Error ? err.message : "serve failed";
      return NextResponse.json({ error: message }, { status: 500 });
    }
  };
}
