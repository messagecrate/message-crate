/**
 * Which of an attachment's versions to read (`docs/architecture/media.md`,
 * rule 3): the original as it was imported, the Preview a browser can show,
 * or the small Thumbnail a conversation shows.
 */
export type AssetVersion = "original" | "preview" | "thumbnail";

/**
 * Path of one version of an attachment, named by the content hash of the
 * original. The hash alone names the attachment: the account stores one file
 * per hash, whatever the source it was imported from.
 */
export function buildAssetPath(sha256: string, version: AssetVersion = "original"): string {
  const sha = sha256.trim();
  if (!sha) throw new Error("sha256 is required");
  const base = `/v1/assets/${encodeURIComponent(sha)}`;
  return version === "original" ? base : `${base}/${version}`;
}
