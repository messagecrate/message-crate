import { assetRoute } from "@/lib/vault/assets";

export const runtime = "nodejs";

/**
 * An attachment's Preview, proxied from `GET /v1/assets/{sha256}/preview`:
 * the JPEG, MP4 or MP3 the server made for a browser to show. Without one it
 * answers the original.
 */
export const GET = assetRoute("preview");
