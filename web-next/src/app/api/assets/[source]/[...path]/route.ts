import { assetRoute } from "@/lib/vault/assets";

export const runtime = "nodejs";

/** An attachment's original bytes, proxied from `GET /v1/assets/{sha256}`. */
export const GET = assetRoute("original");
