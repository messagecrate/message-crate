import type { AttachmentChoices } from "./types";

/**
 * The checks on Max FPS and Minimum Video File Size, the two typed fields of
 * Convert & Compress media, made as the person types so a bad value never
 * starts an Import Run.
 *
 * The desktop app checks the same rules again before Staging, with the same
 * messages (`compress_options_from_form`, `MAX_FPS_RANGE` and
 * `MIN_VIDEO_SIZE_MB_RANGE` in `crates/libs/media/src/lib.rs`), so a change
 * here is made there too.
 */

/** The whole numbers Max FPS accepts, in frames per second. */
export const MAX_FPS_RANGE = { min: 1, max: 240 } as const;

/** The whole numbers Minimum Video File Size accepts, in megabytes of 1024 × 1024 bytes: up to 1 TB. */
export const MIN_VIDEO_SIZE_MB_RANGE = { min: 1, max: 1_048_576 } as const;

/** The compress fields' problems, keyed by the form's field names. */
type CompressFieldErrors = Partial<Record<"maxFps" | "minSizeMb", string>>;

export const MAX_FPS_INVALID = "Max FPS must be 1–240.";
export const MIN_VIDEO_SIZE_INVALID = "Minimum Video File Size must be 1–1,048,576.";

/**
 * True for an empty field, which means no limit, and for a whole number in
 * `range`. Only digits count, so `+5`, `-5`, `29.97` and `20aaa` are refused.
 */
function emptyOrWholeNumberIn(typed: string, range: { min: number; max: number }): boolean {
  const trimmed = typed.trim();
  if (trimmed === "") return true;
  if (!/^\d+$/.test(trimmed)) return false;
  const n = Number(trimmed);
  return n >= range.min && n <= range.max;
}

/** The fields' problems, keyed as the form shows them; empty unless Convert & Compress media is picked. */
export function compressFieldErrors(attachments: AttachmentChoices): CompressFieldErrors {
  if (attachments.attachmentMedia !== "compress") return {};
  const errors: CompressFieldErrors = {};
  if (!emptyOrWholeNumberIn(attachments.maxFps, MAX_FPS_RANGE)) errors.maxFps = MAX_FPS_INVALID;
  if (!emptyOrWholeNumberIn(attachments.minSizeMb, MIN_VIDEO_SIZE_MB_RANGE)) {
    errors.minSizeMb = MIN_VIDEO_SIZE_INVALID;
  }
  return errors;
}

/**
 * A source's readiness with the compress fields checked too, for a source
 * that shows the Attachments field: Import stays off while either is wrong.
 */
export function withCompressFieldChecks<E extends Partial<Record<string, string>>>(
  readiness: { enabled: boolean; errors: E },
  attachments: AttachmentChoices,
): { enabled: boolean; errors: E & CompressFieldErrors } {
  const compress = compressFieldErrors(attachments);
  return {
    enabled: readiness.enabled && Object.keys(compress).length === 0,
    errors: { ...readiness.errors, ...compress },
  };
}
