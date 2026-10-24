import type { AttachmentRow } from "@/lib/types";

/**
 * Which bytes a thread shows: `converted` uses the Preview and Thumbnail the
 * server made, `original` only ever the original.
 */
export type MediaVariant = "converted" | "original";

/**
 * Which version opens the attachment: the original, the Preview, or neither,
 * when the original is not one every browser shows and the server has no
 * Preview of it.
 */
export type FullVersion = "original" | "preview" | "none";

/** The URLs a thread shows one attachment from. */
export type AttachmentMedia = {
  full: FullVersion;
  /**
   * The bytes of the version that opens. With `full` at `none`, the original,
   * offered as a download only.
   */
  url: string | null;
  /** The type of the bytes at `url`. */
  mimeType: string | null;
  /** The Thumbnail drawn in the thread, when the server made one. */
  thumbnailUrl: string | null;
};

/** Whether the attachment is a picture, a video or a sound, rather than a file. */
function isMedia(a: AttachmentRow): boolean {
  const type = (a.mimeType ?? a.previewMimeType ?? "").toLowerCase();
  return /^(image|video|audio)\//.test(type);
}

/**
 * Pick the URLs of one attachment by the server's answer, as `fullVersion` in
 * `web/src/lib/attachmentMedia.ts` does (`docs/architecture/rules/media.md`,
 * MEDIA-2): the original when `shownAsIs`, else the Preview when the server
 * made one, else neither. A file that is not a picture, a video or a sound
 * always opens as the original. The Thumbnail is drawn in the thread whenever
 * there is one. With the `original` variant only the original is used.
 */
export function attachmentMedia(
  source: string,
  a: AttachmentRow,
  variant: MediaVariant,
): AttachmentMedia {
  const s = encodeURIComponent(source);
  const original = a.assetsPath ? `/api/assets/${s}/${a.assetsPath}` : null;
  if (variant === "original") {
    return {
      full: "original",
      url: original,
      mimeType: a.mimeType,
      thumbnailUrl: null,
    };
  }
  const thumbnailUrl = a.thumbnailAssetsPath
    ? `/api/assets_thumbnail/${s}/${a.thumbnailAssetsPath}`
    : null;
  if (a.shownAsIs || !isMedia(a)) {
    return { full: "original", url: original, mimeType: a.mimeType, thumbnailUrl };
  }
  if (a.previewAssetsPath) {
    return {
      full: "preview",
      url: `/api/assets_converted/${s}/${a.previewAssetsPath}`,
      mimeType: a.previewMimeType ?? a.mimeType,
      thumbnailUrl,
    };
  }
  return { full: "none", url: original, mimeType: a.mimeType, thumbnailUrl };
}
