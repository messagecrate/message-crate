import { formatBytes } from "./formatBytes";
import type { AttachmentMediaMode } from "./types";

/**
 * Staging's attachments line for the run's attachment mode. Staging copies
 * the originals under Convert and Compress too; the Media stage converts
 * them afterwards, so only Skip reads differently here.
 */
export function formatAttachmentProgress(input: {
  mode: AttachmentMediaMode;
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
}): string {
  const verb = input.mode === "skip" ? "Skipped" : "Copied";
  const counts = `${input.done.toLocaleString()}/${input.total.toLocaleString()}`;
  return `${verb} attachments: ${counts} (${formatBytes(input.bytesDone)} / ${formatBytes(input.bytesTotal)})`;
}
