import { noThumbnailNote } from "../lib/attachmentMedia";
import type { MessageAttachment } from "../lib/types";
import { type AssetObjectUrl, useAssetObjectUrl } from "./useAssetObjectUrl";
import { useNearScreen } from "./useNearScreen";

/**
 * Where an attachment's Thumbnail stands, and `note`: why the server could
 * not make it (`noThumbnailNote`), while no Thumbnail is drawn because the
 * row names none or the one it names failed to load. A row can name a
 * Thumbnail whose file the media pass found damaged, so its reason waits
 * for the load to fail rather than standing beside a picture.
 */
export type Thumbnail = AssetObjectUrl & { note: string | null };

/**
 * An attachment's Thumbnail, fetched only once the element given to the
 * returned ref comes near the screen (`docs/architecture/media.md`, rule 5).
 * An attachment with no Thumbnail yet fetches nothing, and its caller shows a
 * placeholder.
 */
export function useThumbnail<T extends Element>(
  attachment: MessageAttachment,
): [(node: T | null) => void, Thumbnail] {
  const [ref, near] = useNearScreen<T>();
  const hasThumbnail = Boolean(attachment.thumbnail_mime_type && !attachment.missing_reason);
  const thumbnail = useAssetObjectUrl(near && hasThumbnail ? attachment.sha256 : null, "thumbnail");
  const note = !hasThumbnail || thumbnail.error ? noThumbnailNote(attachment) : null;
  return [ref, { ...thumbnail, note }];
}
