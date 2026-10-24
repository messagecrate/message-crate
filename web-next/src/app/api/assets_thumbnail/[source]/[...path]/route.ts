import { assetRoute } from "@/lib/vault/assets";

export const runtime = "nodejs";

/**
 * An attachment's Thumbnail, proxied from `GET /v1/assets/{sha256}/thumbnail`:
 * a small JPEG of an image or of a video's first frame. Without one it
 * answers the original.
 */
export const GET = assetRoute("thumbnail");
