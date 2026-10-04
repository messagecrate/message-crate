import type { MessageAttachment } from "../lib/types";
import { type AssetObjectUrl, useAssetObjectUrl } from "./useAssetObjectUrl";
import { useNearScreen } from "./useNearScreen";

/**
 * An attachment's Thumbnail, fetched only once the element given to the
 * returned ref comes near the screen (`docs/architecture/media.md`, rule 5).
 * An attachment with no Thumbnail yet fetches nothing, and its caller shows a
 * placeholder.
 */
export function useThumbnail<T extends Element>(
  attachment: MessageAttachment,
): [(node: T | null) => void, AssetObjectUrl] {
  const [ref, near] = useNearScreen<T>();
  const hasThumbnail = Boolean(attachment.thumbnail_mime_type && !attachment.missing_reason);
  const thumbnail = useAssetObjectUrl(near && hasThumbnail ? attachment.sha256 : null, "thumbnail");
  return [ref, thumbnail];
}
