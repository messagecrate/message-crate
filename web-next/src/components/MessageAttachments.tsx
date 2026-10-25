import {
  attachmentMedia,
  isHeld,
  isImageMime,
  type MediaVariant,
  noPreviewNote,
  threadDrawing,
} from "@/lib/attachmentMedia";
import type { AttachmentRow } from "@/lib/types";

import {
  AttachmentThumbnail,
  NoticeThumbnail,
  ThumbnailPlaceholder,
} from "./AttachmentThumbnail";

function isVideoMime(mime: string | null): boolean {
  return Boolean(mime?.startsWith("video/"));
}

function mediaVariant(): MediaVariant {
  const raw = (
    process.env.NEXT_PUBLIC_MEDIA_VARIANT ??
    process.env.MEDIA_VARIANT ??
    "converted"
  )
    .trim()
    .toLowerCase();
  if (raw === "original") return "original";
  return "converted";
}

function MissingMediaPlaceholder({
  attachment,
}: {
  attachment: AttachmentRow;
}) {
  const label = attachment.originalName?.trim() || "Attachment";
  const kind = isImageMime(attachment.mimeType)
    ? "image"
    : isVideoMime(attachment.mimeType)
      ? "video"
      : "file";
  const tall = kind === "image" || kind === "video";

  return (
    <div
      role="img"
      aria-label={`Missing ${kind}: ${label}`}
      title={`Missing ${kind}: ${label}`}
      className={`flex flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-border bg-hover px-3 text-center ${
        tall ? "min-h-[7.5rem] w-full max-w-xs py-5" : "min-h-[3.25rem] py-2.5"
      }`}
    >
      {kind === "image" ? (
        <BrokenImageIcon className="size-8 opacity-55" />
      ) : kind === "video" ? (
        <BrokenVideoIcon className="size-8 opacity-55" />
      ) : (
        <BrokenFileIcon className="size-6 opacity-55" />
      )}
      <div className="min-w-0">
        <div className="text-[11px] font-medium tracking-wide uppercase opacity-70">
          Missing {kind}
        </div>
        <div className="mt-0.5 truncate text-[12px] opacity-80">{label}</div>
      </div>
    </div>
  );
}

/**
 * The attachment opens in no version: the original is not one every browser
 * shows, and the server holds no Preview of it (`docs/architecture/rules/media.md`,
 * MEDIA-2). Says why, and offers the original as a download.
 */
function NoPreviewNotice({
  attachment,
  url,
  thumbnailUrl,
}: {
  attachment: AttachmentRow;
  url: string;
  thumbnailUrl: string | null;
}) {
  const label = attachment.originalName?.trim() || "Attachment";
  const note = noPreviewNote(attachment);
  return (
    <div className="flex max-w-xs flex-col gap-1.5">
      {thumbnailUrl ? <NoticeThumbnail src={thumbnailUrl} alt={label} /> : null}
      <div
        role="note"
        className="flex flex-col gap-1 rounded-lg border border-dashed border-border bg-hover px-3 py-2.5 text-[12px]"
      >
        <div className="opacity-80">
          {note ?? "This attachment has no copy a browser can show yet."}
        </div>
        <div className="opacity-80">
          Download it to open the original:{" "}
          <a href={url} className="underline" download={label}>
            {label}
          </a>
        </div>
      </div>
    </div>
  );
}

export function MessageAttachments({
  source,
  attachments,
  hasBody,
}: {
  source: string;
  attachments: AttachmentRow[];
  hasBody?: boolean;
}) {
  if (attachments.length === 0) return null;

  return (
    <div className={`${hasBody ? "mt-2" : ""} space-y-1.5`}>
      {attachments.map((a) => {
        const media = attachmentMedia(source, a, mediaVariant());
        if (!isHeld(media)) {
          return <MissingMediaPlaceholder key={a.id} attachment={a} />;
        }
        const { url, thumbnailUrl } = media;
        switch (threadDrawing(media)) {
          case "notice":
            return (
              <NoPreviewNotice
                key={a.id}
                attachment={a}
                url={url}
                thumbnailUrl={thumbnailUrl}
              />
            );
          case "thumbnail":
            return thumbnailUrl ? (
              <AttachmentThumbnail
                key={a.id}
                attachment={a}
                url={url}
                thumbnailUrl={thumbnailUrl}
              />
            ) : (
              <ThumbnailPlaceholder key={a.id} attachment={a} url={url} />
            );
          case "placeholder":
            return <ThumbnailPlaceholder key={a.id} attachment={a} url={url} />;
          case "link":
            return (
              <a
                key={a.id}
                href={url}
                className="block text-[12px] underline opacity-90"
                target="_blank"
                rel="noreferrer"
              >
                {a.originalName ?? url}
              </a>
            );
        }
      })}
    </div>
  );
}

function BrokenImageIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
      <circle cx="9" cy="10" r="1.75" />
      <path d="m6.5 17.5 4-4 2.5 2.5 2-2 2.5 3.5" />
      <path d="m4 4 16 16" />
    </svg>
  );
}

function BrokenVideoIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <rect x="3.5" y="5.5" width="13" height="13" rx="2" />
      <path d="m16.5 10 4-2.25v8.5L16.5 14" />
      <path d="m4 4 16 16" />
    </svg>
  );
}

function BrokenFileIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M14 3.5H8A1.5 1.5 0 0 0 6.5 5v14A1.5 1.5 0 0 0 8 20.5h8A1.5 1.5 0 0 0 17.5 19V8.5Z" />
      <path d="M14 3.5V8h4.5" />
      <path d="m5 5 14 14" />
    </svg>
  );
}
