import { useMutation } from "@tanstack/react-query";
import { attachmentName } from "../lib/attachmentMedia";
import { downloadAttachment } from "../lib/downloadAttachment";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import { DownloadIcon } from "./icons";
import PlainButton from "./PlainButton";

/** How the round button is drawn: over a picture in the conversation, or in the viewer's dark top bar. */
const ROUND = {
  overlay: { className: "absolute right-1.5 top-1.5 h-7 w-7 bg-media-control", icon: 14 },
  viewer: { className: "h-10 w-10 bg-lightbox-control", icon: 18 },
} as const;

/**
 * Save an attachment's original: the control every attachment carries in the
 * conversation and in the viewer. `overlay` draws it as a round button over a
 * picture; otherwise it is a line of text beside a file's name.
 */
export default function DownloadAttachmentButton({
  attachment,
  look,
}: {
  attachment: MessageAttachment;
  look: "overlay" | "text" | "viewer";
}) {
  const download = useMutation({ mutationFn: () => downloadAttachment(attachment) });
  const name = attachmentName(attachment);
  const label = download.isPending
    ? `Downloading ${name}`
    : download.isError
      ? `Download of ${name} failed, try again`
      : `Download ${name}`;

  if (look === "text") {
    return (
      <PlainButton
        onPress={() => download.mutate()}
        isDisabled={download.isPending}
        aria-label={label}
        className={`shrink-0 cursor-pointer rounded border-none bg-transparent p-0 text-[0.75rem] text-accent hover:underline disabled:cursor-not-allowed disabled:opacity-50 ${focusRing}`}
      >
        {download.isPending ? "Downloading…" : download.isError ? "Download failed" : "Download"}
      </PlainButton>
    );
  }

  const round = ROUND[look];
  return (
    <PlainButton
      onPress={() => download.mutate()}
      isDisabled={download.isPending}
      aria-label={label}
      title={download.isError ? "Download failed" : "Download"}
      className={`${round.className} flex cursor-pointer items-center justify-center rounded-full border-none text-lightbox-text disabled:cursor-not-allowed disabled:opacity-50 ${focusRing}`}
    >
      <DownloadIcon size={round.icon} />
    </PlainButton>
  );
}
