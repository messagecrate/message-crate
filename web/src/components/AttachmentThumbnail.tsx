import { useThumbnail } from "../hooks/useThumbnail";
import { attachmentName } from "../lib/attachmentMedia";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import PlainButton from "./PlainButton";
import { ThumbnailPicture, ThumbnailTile } from "./ThumbnailTile";

/**
 * A photo in the conversation: its Thumbnail, fetched once its message comes
 * near the screen, never the original (`docs/architecture/media.md`, rule 5).
 * Pressing it opens the viewer.
 */
export default function AttachmentThumbnail({
  attachment,
  onClick,
}: {
  attachment: MessageAttachment;
  onClick: () => void;
}) {
  const [ref, thumbnail] = useThumbnail<HTMLDivElement>(attachment);
  const name = attachmentName(attachment);

  return (
    <ThumbnailTile tileRef={ref}>
      <PlainButton
        onPress={onClick}
        aria-label={`Open ${name}`}
        className={`block h-full cursor-pointer overflow-hidden rounded-md border border-border bg-transparent p-0 ${focusRing}`}
      >
        <ThumbnailPicture name={name} url={thumbnail.url} />
      </PlainButton>
      <DownloadAttachmentButton attachment={attachment} look="overlay" />
    </ThumbnailTile>
  );
}
