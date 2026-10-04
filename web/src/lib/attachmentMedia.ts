import type { MessageAttachment } from "./types";

/**
 * What an attachment is, for how a conversation shows it: a picture, a video
 * or a sound to play, or a file to download.
 */
export type AttachmentKind = "image" | "video" | "audio" | "file";

/**
 * Which version opens an attachment in full (`docs/architecture/media.md`,
 * rule 2): the original, the Preview, or neither, when the attachment is of
 * a type browsers often cannot show and the server has not made its Preview
 * yet.
 */
export type FullVersion = "original" | "preview" | "none";

/**
 * The types every browser shows as they are, the list the server's
 * `media::browser_shows` holds. An MP4 is here by its type alone: the server
 * also reads its video codec and makes a Preview of a HEVC one, and an
 * attachment with a Preview opens the Preview.
 */
const SHOWN_AS_IS = new Set([
  "image/jpeg",
  "image/png",
  "image/gif",
  "image/webp",
  "audio/mpeg",
  "video/mp4",
]);

/** Other spellings of a type, read as its usual one. */
const ALIASES: Record<string, string> = {
  "image/jpg": "image/jpeg",
  "image/pjpeg": "image/jpeg",
  "audio/mp3": "audio/mpeg",
  "audio/x-mp3": "audio/mpeg",
  "audio/x-mpeg": "audio/mpeg",
};

/** Types by a file name's extension, for an attachment the import named no type for. */
const BY_EXTENSION: Record<string, string> = {
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  png: "image/png",
  gif: "image/gif",
  webp: "image/webp",
  heic: "image/heic",
  heif: "image/heif",
  tif: "image/tiff",
  tiff: "image/tiff",
  bmp: "image/bmp",
  mp4: "video/mp4",
  m4v: "video/x-m4v",
  mov: "video/quicktime",
  "3gp": "video/3gpp",
  webm: "video/webm",
  mp3: "audio/mpeg",
  m4a: "audio/mp4",
  aac: "audio/aac",
  amr: "audio/amr",
  caf: "audio/x-caf",
  wav: "audio/wav",
  ogg: "audio/ogg",
  opus: "audio/ogg",
};

function extensionType(name: string | null | undefined): string | null {
  const match = /\.([A-Za-z0-9]+)$/.exec(name?.trim() ?? "");
  return match ? (BY_EXTENSION[match[1].toLowerCase()] ?? null) : null;
}

/**
 * The attachment's own type, lowercased and without parameters, read in the
 * order the server's `media::media_type_of` reads it for a stored original,
 * which has no extension: the type the import declared, then the extension
 * of the name the export gave the file, then of its path in the export.
 */
export function mediaType(attachment: MessageAttachment): string | null {
  const declared = attachment.mime_type?.split(";")[0].trim().toLowerCase();
  if (declared) return ALIASES[declared] ?? declared;
  return extensionType(attachment.original_name) ?? extensionType(attachment.path);
}

/** Whether the conversation shows the attachment as a picture, a player, or a file. */
export function attachmentKind(attachment: MessageAttachment): AttachmentKind {
  const type = mediaType(attachment) ?? attachment.preview_mime_type ?? "";
  if (type.startsWith("image/")) return "image";
  if (type.startsWith("video/")) return "video";
  if (type.startsWith("audio/")) return "audio";
  return "file";
}

/**
 * Which version opens the attachment, decided by its type before any byte is
 * fetched, never by a load that failed: the Preview when the server made one,
 * the original when every browser shows its type, and none when neither.
 */
export function fullVersion(attachment: MessageAttachment): FullVersion {
  if (attachment.preview_mime_type) return "preview";
  const type = mediaType(attachment);
  return type && SHOWN_AS_IS.has(type) ? "original" : "none";
}

/** The name a person knows the attachment by: the export's name, else its path's last part. */
export function attachmentName(attachment: MessageAttachment): string {
  if (attachment.original_name?.trim()) return attachment.original_name.trim();
  const base = attachment.path?.trim().split(/[/\\]/).pop();
  return base || "attachment";
}
