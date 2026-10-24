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

/** Request headers passed to the server, so a media element can stream and seek. */
const FORWARDED_REQUEST = ["range", "if-range"];

/** Answer headers passed back to the browser. */
const FORWARDED_ANSWER = [
  "content-type",
  "content-length",
  "content-range",
  "accept-ranges",
  "etag",
];

/** Reads one `/v1` path with the given request headers. */
export type FetchPath = (
  path: string,
  headers: Record<string, string>,
) => Promise<Response>;

/**
 * Read one version of an attachment and answer it to the browser with the
 * server's status: `206 Partial Content` for a byte range
 * (`docs/architecture/rules/media.md`, MEDIA-1), and `404 Not Found` for a
 * Preview or Thumbnail the server has not made. An attachment is read in
 * those versions only when its `preview_mime_type` or `thumbnail_mime_type`
 * says the server holds one (`toAttachment` in `lib/vault/messages.ts`), so
 * the browser never chooses a version by a failed load (MEDIA-2).
 */
export async function proxyAsset(
  sha256: string,
  version: AssetVersion,
  requestHeaders: Headers = new Headers(),
  fetchPath: FetchPath = (path, headers) => vaultFetch(path, { headers }),
): Promise<Response> {
  const forward: Record<string, string> = {};
  for (const name of FORWARDED_REQUEST) {
    const value = requestHeaders.get(name);
    if (value) forward[name] = value;
  }
  const upstream = await fetchPath(assetPath(sha256, version), forward);
  if (upstream.status === 401) return unauthorizedResponse();
  if (!upstream.ok) {
    // Read the small problem body to its end rather than cancel it: inside a
    // Next.js route handler, `body.cancel()` never settled and the request
    // hung, though plain Node.js settles it at once.
    await upstream.arrayBuffer();
    return NextResponse.json(
      { error: upstream.status === 404 ? "not found" : "asset failed" },
      { status: upstream.status },
    );
  }
  const headers = new Headers();
  for (const name of FORWARDED_ANSWER) {
    const value = upstream.headers.get(name);
    if (value) headers.set(name, value);
  }
  headers.set("Cache-Control", "private, max-age=31536000, immutable");
  return new NextResponse(upstream.body, { status: upstream.status, headers });
}

type Params = { params: Promise<{ source: string; path: string[] }> };

/** The `GET` handler of an `app/api/assets*` route for one version. */
export function assetRoute(version: AssetVersion) {
  return async function GET(req: Request, { params }: Params) {
    const { path: parts } = await params;
    const sha256 = sha256FromPath(parts);
    if (!sha256) {
      return NextResponse.json(
        { error: "asset path must end in a sha256" },
        { status: 400 },
      );
    }
    try {
      return await withAccountHandler(() =>
        proxyAsset(sha256, version, req.headers),
      );
    } catch (err) {
      if (err instanceof Error && err.message === "Not signed in") {
        return unauthorizedResponse();
      }
      const message = err instanceof Error ? err.message : "serve failed";
      return NextResponse.json({ error: message }, { status: 500 });
    }
  };
}
