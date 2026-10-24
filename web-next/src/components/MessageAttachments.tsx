import { attachmentMedia, type MediaVariant } from "@/lib/attachmentMedia";
import type { AttachmentRow } from "@/lib/types";

function isImageMime(mime: string | null): boolean {
  return Boolean(mime?.startsWith("image/"));
}

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

/** Formats a browser cannot show from the original bytes. */
const NEEDS_TRANSCODE = new Set([
  "image/heic",
  "image/heif",
  "video/quicktime",
  "video/x-msvideo",
  "audio/amr",
  "audio/x-caf",
]);

function needsTranscode(mime: string | null): boolean {
  return Boolean(mime && NEEDS_TRANSCODE.has(mime.toLowerCase()));
}

/** The server holds no Preview of the attachment yet; the raw file is offered instead. */
function NoTranscodeNotice({
  attachment,
  url,
}: {
  attachment: AttachmentRow;
  url: string;
}) {
  const label = attachment.originalName?.trim() || "Attachment";
  return (
    <div
      role="note"
      className="flex max-w-xs flex-col gap-1 rounded-lg border border-dashed border-border bg-hover px-3 py-2.5 text-[12px]"
    >
      <div className="text-[11px] font-medium tracking-wide uppercase opacity-70">
        No Preview
      </div>
      <div className="opacity-80">
        {attachment.mimeType ?? "This format"} is not playable here. Raw file:{" "}
        <a href={url} className="underline" target="_blank" rel="noreferrer">
          {label}
        </a>
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
        const { url, mimeType, thumbnailUrl } = attachmentMedia(
          source,
          a,
          mediaVariant(),
        );
        if (url && thumbnailUrl) {
          return (
            <a
              key={a.id}
              href={url}
              className="block w-fit"
              target="_blank"
              rel="noreferrer"
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={thumbnailUrl}
                alt={a.originalName ?? "attachment"}
                loading="lazy"
                decoding="async"
                className="max-h-64 max-w-full rounded-lg"
              />
            </a>
          );
        }
        if (url && needsTranscode(mimeType)) {
          return <NoTranscodeNotice key={a.id} attachment={a} url={url} />;
        }
        if (url && isImageMime(mimeType)) {
          return (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              key={a.id}
              src={url}
              alt={a.originalName ?? "attachment"}
              loading="lazy"
              decoding="async"
              className="max-h-64 max-w-full rounded-lg"
            />
          );
        }
        if (url) {
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
        return <MissingMediaPlaceholder key={a.id} attachment={a} />;
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
