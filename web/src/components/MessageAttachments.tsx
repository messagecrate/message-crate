import { attachmentKind, attachmentName } from "../lib/attachmentMedia";
import { missingAttachmentChipLabel } from "../lib/missingAttachmentLabel";
import type { Message, MessageAttachment } from "../lib/types";
import AttachmentThumbnail from "./AttachmentThumbnail";
import AudioPlayer from "./AudioPlayer";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import VideoPlayer from "./VideoPlayer";

/** One attachment in a bubble: a picture, a player, or a file to download. */
function AttachmentItem({
  attachment,
  onClick,
}: {
  attachment: MessageAttachment;
  onClick: () => void;
}) {
  if (attachment.missing_reason) {
    return (
      <div className="mt-1.5 flex items-center gap-2 rounded bg-elevated px-2 py-2 text-[0.813rem] text-muted">
        <span>📎</span>
        <span>{missingAttachmentChipLabel(attachment)}</span>
      </div>
    );
  }
  const kind = attachment.sha256 ? attachmentKind(attachment) : "file";
  if (kind === "image") return <AttachmentThumbnail attachment={attachment} onClick={onClick} />;
  if (kind === "video") return <VideoPlayer attachment={attachment} />;
  if (kind === "audio") return <AudioPlayer attachment={attachment} />;
  return (
    <div className="mt-1.5 flex max-w-[320px] items-center gap-2 rounded bg-elevated px-2 py-2 text-[0.813rem]">
      <span>📎</span>
      <span className="min-w-0 flex-1 truncate text-text">{attachmentName(attachment)}</span>
      {attachment.sha256 ? <DownloadAttachmentButton attachment={attachment} look="text" /> : null}
    </div>
  );
}

/** Shared attachment strip for every service bubble. */
export default function MessageAttachments({
  message,
  onAttachmentClick,
}: {
  message: Message;
  onAttachmentClick?: (attachment: MessageAttachment) => void;
}) {
  if (!message.attachments.length) return null;

  return (
    <div>
      {message.attachments.map((att, i) => (
        <AttachmentItem
          key={att.sha256 ?? att.path ?? i}
          attachment={att}
          onClick={() => onAttachmentClick?.(att)}
        />
      ))}
    </div>
  );
}
