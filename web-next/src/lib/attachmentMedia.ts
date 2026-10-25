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
 * Pick the URLs of one attachment by the server's answer
 * (`docs/architecture/rules/media.md`, MEDIA-2). A picture, video or sound
 * opens as `fullVersion` in `web/src/lib/attachmentMedia.ts` decides: the
 * original when `shownAsIs`, else the Preview when the server made one, else
 * neither. Any other file opens as the original, as `web/`'s
 * `MessageAttachments` offers it as a download whatever its version. The
 * Thumbnail is drawn in the thread whenever there is one. The `original`
 * variant never reads a Preview or Thumbnail, so there a picture, video or
 * sound that is not shown as it is opens in no version.
 */
export function attachmentMedia(
  source: string,
  a: AttachmentRow,
  variant: MediaVariant,
): AttachmentMedia {
  const s = encodeURIComponent(source);
  const original = a.assetsPath ? `/api/assets/${s}/${a.assetsPath}` : null;
  if (a.shownAsIs || !isMedia(a)) {
    const thumbnailUrl =
      variant === "converted" && a.thumbnailAssetsPath
        ? `/api/assets_thumbnail/${s}/${a.thumbnailAssetsPath}`
        : null;
    return { full: "original", url: original, mimeType: a.mimeType, thumbnailUrl };
  }
  if (variant === "original") {
    return { full: "none", url: original, mimeType: a.mimeType, thumbnailUrl: null };
  }
  const thumbnailUrl = a.thumbnailAssetsPath
    ? `/api/assets_thumbnail/${s}/${a.thumbnailAssetsPath}`
    : null;
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

/**
 * What the thread says about an attachment that opens in no version because
 * the server tried to make its Preview and could not, in the words of
 * `noPreviewNote` in `web/src/lib/attachmentMedia.ts` (MEDIA-4). `null` while
 * no try has failed.
 */
export function noPreviewNote(a: AttachmentRow): string | null {
  const reason = a.previewNotMadeReason;
  return reason ? `No Preview could be made: ${reason}` : null;
}

/**
 * What the thread says about a picture with no Thumbnail because the server
 * tried to make one and could not, in the words of `noThumbnailNote` in
 * `web/src/lib/attachmentMedia.ts` (MEDIA-4). `null` while no try has failed.
 */
export function noThumbnailNote(a: AttachmentRow): string | null {
  const reason = a.thumbnailNotMadeReason;
  return reason ? `No Thumbnail could be made: ${reason}` : null;
}

/** Whether a type names a picture. */
export function isImageMime(mime: string | null): boolean {
  return Boolean(mime?.startsWith("image/"));
}

/**
 * What the thread draws for one attachment the server holds bytes of:
 *
 * - `notice`: it opens in no version (`full` is `none`), so the thread says
 *   why and offers the original as a download.
 * - `thumbnail`: its Thumbnail, which opens the full version on click.
 * - `placeholder`: a picture with no Thumbnail yet, or one the server could
 *   not make, and every picture with the `original` variant, which never
 *   reads a Thumbnail. The thread draws its name, and
 *   {@link noThumbnailNote} when there is one, and the version that opens
 *   opens on click, as `web/`'s `AttachmentThumbnail` does: a conversation
 *   never fetches an original while scrolling
 *   (`docs/architecture/rules/media.md`, MEDIA-5).
 * - `link`: any other file, as a link to the version that opens.
 */
export type ThreadDrawing = "notice" | "thumbnail" | "placeholder" | "link";

/**
 * Pick what the thread draws for an attachment, from {@link attachmentMedia}
 * with a `url`. One with no `url` has no bytes on the server, and the thread
 * says it is missing.
 */
export function threadDrawing(
  media: AttachmentMedia & { url: string },
): ThreadDrawing {
  if (media.full === "none") return "notice";
  if (media.thumbnailUrl) return "thumbnail";
  if (isImageMime(media.mimeType)) return "placeholder";
  return "link";
}
