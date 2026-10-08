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

const TROUBLESHOOTING = "https://messagecrate.app/docs/user/features/owner/troubleshooting/";

/** The user guide's troubleshooting section for `name`: its heading and its address. */
export function troubleshootingSection(name: ToolName): { heading: string; url: string } {
  return name === "wtsexporter"
    ? {
        heading: "Import can't find wtsexporter",
        url: `${TROUBLESHOOTING}#import-cant-find-wtsexporter`,
      }
    : {
        heading: "ffmpeg or ffprobe not found",
        url: `${TROUBLESHOOTING}#ffmpeg-or-ffprobe-not-found`,
      };
}

/**
 * The sentence that sends a person to `name`'s troubleshooting section, word
 * for word as a run's failed-download error ends (`troubleshooting` in
 * src-tauri/src/tool_downloads.rs).
 */
export function troubleshootingSentence(name: ToolName): string {
  return `See "${troubleshootingSection(name).heading}" in Troubleshooting at messagecrate.app.`;
}
