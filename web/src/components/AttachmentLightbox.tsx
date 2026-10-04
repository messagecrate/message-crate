import { type ReactNode, useEffect } from "react";
import { Dialog, Modal, ModalOverlay } from "react-aria-components";
import { type AssetRequest, useAssetObjectUrls } from "../hooks/useAssetObjectUrl";
import { attachmentName, fullVersion } from "../lib/attachmentMedia";
import type { MessageAttachment } from "../lib/types";
import { focusRing } from "../lib/uiStyles";
import { Z_MODAL } from "../lib/zLayers";
import DownloadAttachmentButton from "./DownloadAttachmentButton";
import PlainButton from "./PlainButton";

/**
 * What the viewer holds: the photo on screen, in full and as its Thumbnail,
 * and its neighbours in full, so stepping to the next or previous photo finds
 * it loaded. A photo with no version a browser can show asks for none.
 */
function viewerRequests(items: MessageAttachment[], currentIndex: number): AssetRequest[] {
  const requests: AssetRequest[] = [];
  const full = (attachment: MessageAttachment | undefined) => {
    const version = attachment ? fullVersion(attachment) : "none";
    if (attachment?.sha256 && version !== "none") {
      requests.push({ sha256: attachment.sha256, version });
    }
  };
  const current = items[currentIndex];
  if (current?.sha256 && current.thumbnail_mime_type) {
    requests.push({ sha256: current.sha256, version: "thumbnail" });
  }
  full(current);
  if (items.length > 1) {
    full(items[(currentIndex + 1) % items.length]);
    full(items[(currentIndex - 1 + items.length) % items.length]);
  }
  return requests;
}

/**
 * The photo viewer. It opens the original or the Preview by the photo's type,
 * never by a load that failed (`docs/architecture/media.md`, rule 2), keeps
 * the Thumbnail on screen until the full version has loaded, and offers the
 * original as a download.
 */
export default function AttachmentLightbox({
  items,
  currentIndex,
  onClose,
  onPrev,
  onNext,
}: {
  items: MessageAttachment[];
  currentIndex: number;
  onClose: () => void;
  onPrev: () => void;
  onNext: () => void;
}) {
  const attachment = items[currentIndex];
  const lookup = useAssetObjectUrls(viewerRequests(items, currentIndex));

  // React Aria's Dialog type omits keyboard events and drops them at runtime,
  // so arrow-key navigation is handled with a window listener (as in ContactDrawer).
  useEffect(() => {
    if (!attachment) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") onPrev();
      else if (e.key === "ArrowRight") onNext();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [attachment, onPrev, onNext]);

  if (!attachment) return null;

  const name = attachmentName(attachment);
  const version = fullVersion(attachment);
  const full = version === "none" ? null : lookup(attachment.sha256, version);
  const thumbnail = attachment.thumbnail_mime_type ? lookup(attachment.sha256, "thumbnail") : null;
  const note = "text-[0.875rem] text-lightbox-text";

  let media: ReactNode;
  if (full?.url) {
    media = <img src={full.url} alt={name} className="max-h-[90vh] max-w-[90vw] object-contain" />;
  } else {
    const message =
      version === "none"
        ? "This photo has no copy a browser can show yet. Download it to open the original."
        : full?.error
          ? "Failed to load attachment"
          : "Loading…";
    media = (
      <div className="flex flex-col items-center gap-3">
        {thumbnail?.url ? (
          <img
            src={thumbnail.url}
            alt={name}
            aria-busy={version !== "none" && !full?.error}
            className="max-h-[80vh] max-w-[90vw] object-contain"
          />
        ) : null}
        <div className={`${note} max-w-[28rem] text-center`}>{message}</div>
        {version === "none" ? (
          <DownloadAttachmentButton attachment={attachment} look="text" />
        ) : null}
      </div>
    );
  }

  return (
    <ModalOverlay
      isOpen
      onOpenChange={() => onClose()}
      isDismissable
      className={`fixed inset-0 flex items-center justify-center bg-lightbox-bg ${Z_MODAL}`}
    >
      <Modal className="flex min-h-0 w-full items-center justify-center outline-none">
        <Dialog
          aria-label="Attachment viewer"
          className="flex items-center justify-center outline-none"
        >
          <div className="flex items-center justify-center outline-none">
            {items.length > 1 && (
              <PlainButton
                onPress={onPrev}
                aria-label="Previous attachment"
                className={`absolute left-4 top-1/2 flex h-12 w-12 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border-none bg-lightbox-control text-[2rem] text-lightbox-text ${focusRing}`}
              >
                ‹
              </PlainButton>
            )}

            {media}

            {items.length > 1 && (
              <PlainButton
                onPress={onNext}
                aria-label="Next attachment"
                className={`absolute right-4 top-1/2 flex h-12 w-12 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border-none bg-lightbox-control text-[2rem] text-lightbox-text ${focusRing}`}
              >
                ›
              </PlainButton>
            )}

            <div className="absolute right-4 top-4 flex items-center gap-4">
              <span className={note}>
                {currentIndex + 1} / {items.length}
              </span>
              <DownloadAttachmentButton attachment={attachment} look="viewer" />
              <PlainButton
                onPress={onClose}
                aria-label="Close attachment viewer"
                className={`flex h-10 w-10 cursor-pointer items-center justify-center rounded-full border-none bg-lightbox-control text-[1.5rem] text-lightbox-text ${focusRing}`}
              >
                ×
              </PlainButton>
            </div>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
