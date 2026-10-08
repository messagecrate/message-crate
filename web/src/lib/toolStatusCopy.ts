import { formatBytes } from "./attachmentProgressCopy";
import type { ToolName } from "./tauri";

/**
 * A download's progress: "12 MB of 29 MB (41%)", or "12 MB so far" with no
 * total. The Import form, Settings and the run's progress line all say it
 * this way, so one download reads the same in each.
 */
export function downloadProgress(received: number, total: number | null): string {
  if (total == null || total <= 0) return `${formatBytes(received)} so far`;
  const percent = Math.min(100, Math.floor((received / total) * 100));
  return `${formatBytes(received)} of ${formatBytes(total)} (${percent}%)`;
}

/**
 * The progress line of a run waiting for `name`'s download (#1053):
 * "Waiting for the wtsexporter download: 12 MB of 30 MB (40%)", or no
 * numbers before the first byte arrives.
 */
export function waitingLine(name: ToolName, received: number, total: number | null): string {
  const lead = `Waiting for the ${name} download`;
  if (received <= 0 && (total == null || total <= 0)) return lead;
  return `${lead}: ${downloadProgress(received, total)}`;
}
