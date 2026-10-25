import {
  attachmentMedia,
  isImageMime,
  type MediaVariant,
  noPreviewNote,
  noThumbnailNote,
  threadDrawing,
} from "@/lib/attachmentMedia";
import type { AttachmentRow } from "@/lib/types";

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
      {thumbnailUrl ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={thumbnailUrl}
          alt={label}
          loading="lazy"
          decoding="async"
          className="max-h-64 max-w-full rounded-lg"
        />
      ) : null}
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

/**
 * A picture with no Thumbnail yet, or one the server could not make: its name
 * in the Thumbnail's place, with why under it when the server says, opening
 * `url` (the original, or the Preview when that is the version that opens)
 * on click, as `web/`'s `AttachmentThumbnail` does. The thread never fetches
 * those bytes itself (`docs/architecture/rules/media.md`, MEDIA-5).
 */
function ThumbnailPlaceholder({
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
        const { url, thumbnailUrl } = media;
        if (!url) return <MissingMediaPlaceholder key={a.id} attachment={a} />;
        switch (threadDrawing({ ...media, url })) {
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
                  src={thumbnailUrl ?? undefined}
                  alt={a.originalName ?? "attachment"}
                  loading="lazy"
                  decoding="async"
                  className="max-h-64 max-w-full rounded-lg"
                />
              </a>
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
