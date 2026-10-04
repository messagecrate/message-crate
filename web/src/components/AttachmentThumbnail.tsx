import { useThumbnail } from "../hooks/useThumbnail";
import { attachmentName } from "../lib/attachmentMedia";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import PlainButton from "./PlainButton";

/**
 * Every picture in the conversation is this tall, the Thumbnail and the
 * placeholder before it alike, so a Thumbnail that loads above the messages
 * on screen does not push them down.
 */
export const TILE_HEIGHT = "h-[200px]";

/** What a tile shows before its Thumbnail, or instead of one: the file's name. */
export function TilePlaceholder({ name }: { name: string }) {
  return (
    <span className="flex h-full w-[200px] items-center justify-center break-all bg-elevated px-3 text-center text-[0.75rem] text-muted">
      {name}
    </span>
  );
}

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
    <div ref={ref} className={`relative mt-1.5 w-fit max-w-[280px] ${TILE_HEIGHT}`}>
      <PlainButton
        onPress={onClick}
        aria-label={`Open ${name}`}
        className={`block h-full cursor-pointer overflow-hidden rounded-md border border-border bg-transparent p-0 ${focusRing}`}
      >
        {thumbnail.url ? (
          <img
            src={thumbnail.url}
            alt={name}
            className="block h-full w-auto max-w-[278px] object-cover"
          />
        ) : (
          <TilePlaceholder name={name} />
        )}
      </PlainButton>
      <DownloadAttachmentButton attachment={attachment} look="overlay" />
    </div>
  );
}
