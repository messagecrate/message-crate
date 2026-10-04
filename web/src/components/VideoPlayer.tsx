import { useMutation } from "@tanstack/react-query";
import { useThumbnail } from "../hooks/useThumbnail";
import { attachmentName, fullMimeType, fullVersion } from "../lib/attachmentMedia";
import { createMediaLink } from "../lib/serverApi";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import { TILE_HEIGHT, TilePlaceholder } from "./AttachmentThumbnail";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import { PlayIcon } from "./icons";
import PlainButton from "./PlainButton";

/**
 * A video in the conversation: its Thumbnail with a play button, and nothing
 * of the video itself until the button is pressed. Pressing it makes a Media
 * Link and hands it to a `<video>`, which streams the original or the
 * Preview, as the type decides (`docs/architecture/media.md`, rules 1, 2
 * and 5). Most videos in a conversation are scrolled past and never played.
 */
export default function VideoPlayer({ attachment }: { attachment: MessageAttachment }) {
  const [ref, thumbnail] = useThumbnail<HTMLDivElement>(attachment);
  const sha256 = attachment.sha256 ?? "";
  const link = useMutation({ mutationFn: () => createMediaLink(sha256) });
  const name = attachmentName(attachment);
  const version = fullVersion(attachment);

  if (link.data) {
    return (
      <div className="mt-1.5 w-[400px] max-w-full">
        <video
          controls
          autoPlay
          preload="metadata"
          poster={thumbnail.url ?? undefined}
          aria-label={name}
          className="block max-h-[400px] w-full rounded-md bg-lightbox-bg"
        >
          <source
            src={version === "preview" ? link.data.preview_url : link.data.url}
            type={fullMimeType(attachment)}
          />
          <track kind="captions" />
        </video>
        <div className="mt-0.5 flex items-center gap-2">
          <DownloadAttachmentButton attachment={attachment} look="text" />
        </div>
      </div>
    );
  }

  return (
    <div ref={ref} className={`relative mt-1.5 w-fit max-w-[280px] ${TILE_HEIGHT}`}>
      <div className="h-full overflow-hidden rounded-md border border-border">
        {thumbnail.url ? (
          <img
            src={thumbnail.url}
            alt={name}
            className="block h-full w-auto max-w-[278px] object-cover"
          />
        ) : (
          <TilePlaceholder name={name} />
        )}
      </div>
      <div className="absolute inset-0 flex flex-col items-center justify-center gap-1.5">
        {version === "none" ? (
          <span className="mx-3 rounded bg-media-control px-2 py-1 text-center text-[0.75rem] text-lightbox-text">
            No copy a browser can play yet
          </span>
        ) : (
          <PlainButton
            onPress={() => link.mutate()}
            isDisabled={link.isPending}
            aria-label={`Play ${name}`}
            className={`flex h-12 w-12 cursor-pointer items-center justify-center rounded-full border-none bg-media-control text-lightbox-text disabled:cursor-wait ${focusRing}`}
          >
            <PlayIcon size={20} />
          </PlainButton>
        )}
        {link.isError ? (
          <span className="rounded bg-media-control px-2 py-1 text-[0.75rem] text-lightbox-text">
            Could not play the video
          </span>
        ) : null}
      </div>
      <DownloadAttachmentButton attachment={attachment} look="overlay" />
    </div>
  );
}
