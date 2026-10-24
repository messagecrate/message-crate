import type { AttachmentRow } from "@/lib/types";

/**
 * Which bytes a thread shows: `converted` uses the Preview and Thumbnail the
 * server made, `original` only ever the original.
 */
export type MediaVariant = "converted" | "original";

/** The URLs a thread shows one attachment from. */
export type AttachmentMedia = {
  /** The version that opens the attachment: the original or its Preview. */
  url: string | null;
  /** The type of the bytes at `url`. */
  mimeType: string | null;
  /** The Thumbnail drawn in the thread, when the server made one. */
  thumbnailUrl: string | null;
};

/**
 * Pick the URLs of one attachment. The original opens when every browser
 * shows it as it is (`shownAsIs`), else the Preview when the server made one,
 * else the original. The Thumbnail is drawn in the thread whenever there is
 * one. With the `original` variant only the original is used.
 */
export function attachmentMedia(
  source: string,
  a: AttachmentRow,
  variant: MediaVariant,
): AttachmentMedia {
  const s = encodeURIComponent(source);
  const converted = variant === "converted";
  const thumbnailUrl =
    converted && a.thumbnailAssetsPath
      ? `/api/assets_thumbnail/${s}/${a.thumbnailAssetsPath}`
      : null;
  if (converted && !a.shownAsIs && a.previewAssetsPath) {
    return {
      url: `/api/assets_converted/${s}/${a.previewAssetsPath}`,
      mimeType: a.previewMimeType ?? a.mimeType,
      thumbnailUrl,
    };
  }
  if (a.assetsPath) {
    return {
      url: `/api/assets/${s}/${a.assetsPath}`,
      mimeType: a.mimeType,
      thumbnailUrl,
    };
  }
  return { url: null, mimeType: a.mimeType, thumbnailUrl: null };
}
