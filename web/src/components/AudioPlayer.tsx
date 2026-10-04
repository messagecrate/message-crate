import { useMutation } from "@tanstack/react-query";
import { attachmentName, fullMimeType, fullVersion } from "../lib/attachmentMedia";
import { createMediaLink } from "../lib/serverApi";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import { PlayIcon } from "./icons";
import PlainButton from "./PlainButton";

/**
 * A voice note or other sound in the conversation, played in place. Nothing
 * loads until play is pressed; then a Media Link is made and an `<audio>`
 * streams the original or the Preview, as the type decides
 * (`docs/architecture/media.md`, rules 1, 2 and 5).
 */
export default function AudioPlayer({ attachment }: { attachment: MessageAttachment }) {
  const sha256 = attachment.sha256 ?? "";
  const link = useMutation({ mutationFn: () => createMediaLink(sha256) });
  const name = attachmentName(attachment);
  const version = fullVersion(attachment);

  return (
    <div className="mt-1.5 flex w-[280px] max-w-full flex-col gap-1 rounded-md bg-elevated px-2 py-1.5 text-[0.813rem]">
      <div className="flex min-h-8 items-center gap-2">
        {version === "none" || link.data ? null : (
          <PlainButton
            onPress={() => link.mutate()}
            isDisabled={link.isPending}
            aria-label={`Play ${name}`}
            className={`flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full border-none bg-accent text-sent-text disabled:cursor-wait ${focusRing}`}
          >
            <PlayIcon size={14} />
          </PlainButton>
        )}
        <span className="min-w-0 truncate text-text">{name}</span>
      </div>
      {link.data ? (
        <audio controls autoPlay preload="metadata" aria-label={name} className="h-9 w-full">
          <source
            src={version === "preview" ? link.data.preview_url : link.data.url}
            type={fullMimeType(attachment)}
          />
          <track kind="captions" />
        </audio>
      ) : null}
      <div className="flex items-center justify-between gap-2 text-[0.75rem] text-muted">
        <span>
          {version === "none"
            ? "No copy a browser can play yet"
            : link.isError
              ? "Could not play the recording"
              : null}
        </span>
        <DownloadAttachmentButton attachment={attachment} look="text" />
      </div>
    </div>
  );
}
