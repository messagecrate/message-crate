import { useStreamedMedia } from "../hooks/useStreamedMedia";
import { attachmentName } from "../lib/attachmentMedia";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import { PlayIcon } from "./icons";
import PlainButton from "./PlainButton";

/**
 * A voice note or other sound in the conversation, played in place. Nothing
 * loads until play is pressed; then it streams (`useStreamedMedia`).
 */
export default function AudioPlayer({ attachment }: { attachment: MessageAttachment }) {
  const media = useStreamedMedia(attachment);
  const name = attachmentName(attachment);

  return (
    <div className="mt-1.5 flex w-[280px] max-w-full flex-col gap-1 rounded-md bg-elevated px-2 py-1.5 text-[0.813rem]">
      <div className="flex min-h-8 items-center gap-2">
        {media.playable && !media.src ? (
          <PlainButton
            onPress={media.play}
            isDisabled={media.pending}
            aria-label={`Play ${name}`}
            className={`flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full border-none bg-accent text-sent-text disabled:cursor-wait ${focusRing}`}
          >
            <PlayIcon size={14} />
          </PlainButton>
        ) : null}
        <span className="min-w-0 truncate text-text">{name}</span>
      </div>
      {media.src ? (
        <audio
          controls
          autoPlay
          preload="metadata"
          src={media.src}
          onError={media.onMediaError}
          aria-label={name}
          className="h-9 w-full"
        >
          <track kind="captions" />
        </audio>
      ) : null}
      <div className="flex items-center justify-between gap-2 text-[0.75rem] text-muted">
        <span>{media.note}</span>
        <DownloadAttachmentButton attachment={attachment} look="text" />
      </div>
    </div>
  );
}
