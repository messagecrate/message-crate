import { useStreamedMedia } from "../hooks/useStreamedMedia";
import { useThumbnail } from "../hooks/useThumbnail";
import { attachmentName } from "../lib/attachmentMedia";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import { PlayIcon } from "./icons";
import PlainButton from "./PlainButton";
import { ThumbnailPicture, ThumbnailTile } from "./ThumbnailTile";

/**
 * A video in the conversation: its Thumbnail with a play button, and nothing
 * of the video itself until the button is pressed; then it streams
 * (`useStreamedMedia`). Most videos in a conversation are scrolled past and
 * never played.
 */
export default function VideoPlayer({ attachment }: { attachment: MessageAttachment }) {
  const [ref, thumbnail] = useThumbnail<HTMLDivElement>(attachment);
  const media = useStreamedMedia(attachment);
  const name = attachmentName(attachment);

  if (media.src) {
    return (
      <div className="mt-1.5 w-[400px] max-w-full">
        <video
          controls
          autoPlay
          preload="metadata"
          src={media.src}
          poster={thumbnail.url ?? undefined}
          onError={media.onMediaError}
          aria-label={name}
          className="block max-h-[400px] w-full rounded-md bg-lightbox-bg"
        >
          <track kind="captions" />
        </video>
        <div className="mt-0.5 flex items-center gap-2">
          <DownloadAttachmentButton attachment={attachment} look="text" />
        </div>
      </div>
    );
  }

  return (
    <ThumbnailTile tileRef={ref}>
      <div className="h-full overflow-hidden rounded-md border border-border">
        <ThumbnailPicture name={name} url={thumbnail.url} />
      </div>
      <div className="absolute inset-0 flex flex-col items-center justify-center gap-1.5">
        {media.playable ? (
          <PlainButton
            onPress={media.play}
            isDisabled={media.pending}
            aria-label={`Play ${name}`}
            className={`flex h-12 w-12 cursor-pointer items-center justify-center rounded-full border-none bg-media-control text-lightbox-text disabled:cursor-wait ${focusRing}`}
          >
            <PlayIcon size={20} />
          </PlainButton>
        ) : null}
        {media.note ? (
          <span className="mx-3 rounded bg-media-control px-2 py-1 text-center text-[0.75rem] text-lightbox-text">
            {media.note}
          </span>
        ) : null}
      </div>
      <DownloadAttachmentButton attachment={attachment} look="overlay" />
    </ThumbnailTile>
  );
}
