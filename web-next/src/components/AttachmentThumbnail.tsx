"use client";

import { useEffect, useRef, useState } from "react";

import { noThumbnailNote } from "@/lib/attachmentMedia";
import type { AttachmentRow } from "@/lib/types";

/**
 * A picture with no Thumbnail yet, one the server could not make, or one whose
 * load failed: its name in the Thumbnail's place, with why under it when the
 * server says, opening `url` (the original, or the Preview when that is the
 * version that opens) on click, as `web/`'s `AttachmentThumbnail` does. The
 * thread never fetches those bytes itself (`docs/architecture/rules/media.md`,
 * MEDIA-5).
 */
export function ThumbnailPlaceholder({
  attachment,
  url,
}: {
  attachment: AttachmentRow;
  url: string;
}) {
  const label = attachment.originalName?.trim() || "Attachment";
  const note = noThumbnailNote(attachment);
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      aria-label={`Open ${label}`}
      className="flex h-[200px] w-[200px] flex-col rounded-lg border border-border bg-hover px-3 text-center text-[12px] opacity-90 hover:opacity-100"
    >
      <span className="flex min-h-0 flex-1 items-center justify-center overflow-hidden break-all">
        {label}
      </span>
      {note ? (
        <span title={note} className="mb-2 line-clamp-2 shrink-0 break-words text-[11px] opacity-80">
          {note}
        </span>
      ) : null}
    </a>
  );
}

/**
 * Whether loading `src` failed, and the ref and error handler that tell. A row
 * can name a Thumbnail whose file the media pass found damaged, so the thread
 * learns of it only when the load fails, as in `web/`'s `useThumbnail`. The
 * failure belongs to the `src` that failed: a new `src` is tried again.
 */
function useImageFailed(src: string) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const ref = useRef<HTMLImageElement>(null);

  // A load that failed before hydration fired its error event before React
  // listened for it; the element says so as a complete image with no width.
  useEffect(() => {
    const el = ref.current;
    if (el?.complete && el.naturalWidth === 0) setFailedSrc(src);
  }, [src]);

  return {
    failed: failedSrc === src,
    ref,
    onError: () => setFailedSrc(src),
  };
}

function ThumbnailImg({
  src,
  alt,
  ref,
  onError,
}: {
  src: string;
  alt: string;
  ref: React.Ref<HTMLImageElement>;
  onError: () => void;
}) {
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      ref={ref}
      src={src}
      alt={alt}
      loading="lazy"
      decoding="async"
      onError={onError}
      className="max-h-64 max-w-full rounded-lg"
    />
  );
}

/**
 * The Thumbnail a row names, opening `url` on click. When it fails to load it
 * gives way to {@link ThumbnailPlaceholder}, which shows why the server could
 * not make it when the server says.
 */
export function AttachmentThumbnail({
  attachment,
  url,
  thumbnailUrl,
}: {
  attachment: AttachmentRow;
  url: string;
  thumbnailUrl: string;
}) {
  const { failed, ref, onError } = useImageFailed(thumbnailUrl);
  if (failed) return <ThumbnailPlaceholder attachment={attachment} url={url} />;
  return (
    <a href={url} className="block w-fit" target="_blank" rel="noreferrer">
      <ThumbnailImg
        src={thumbnailUrl}
        alt={attachment.originalName ?? "attachment"}
        ref={ref}
        onError={onError}
      />
    </a>
  );
}

/**
 * The Thumbnail above a no-Preview notice. When it fails to load it is left
 * out, and the notice under it still says why the attachment does not open.
 */
export function NoticeThumbnail({ src, alt }: { src: string; alt: string }) {
  const { failed, ref, onError } = useImageFailed(src);
  if (failed) return null;
  return <ThumbnailImg src={src} alt={alt} ref={ref} onError={onError} />;
}
