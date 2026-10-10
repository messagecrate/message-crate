import { formatBytes } from "./formatBytes";
import type { ToolName, ToolStatus } from "./tauri";

/**
 * A download's progress: "12 MB of 29 MB (41%)", or "12 MB so far" with no
 * total. The Import form, Settings and the run's progress line all say it
 * this way, so one download reads the same in each.
 */
function downloadProgress(received: number, total: number | null): string {
  if (total == null || total <= 0) return `${formatBytes(received)} so far`;
  const percent = Math.min(100, Math.floor((received / total) * 100));
  return `${formatBytes(received)} of ${formatBytes(total)} (${percent}%)`;
}

/**
 * The line for a program that is not found, the same in Settings and on the
 * Import form, one case per `state` the desktop process sends. It is split
 * around the program's name so Settings can set the name as code:
 * `${before}${name}${after}` is the sentence.
 */
export function toolStatusLine(status: Exclude<ToolStatus, { state: "found" }>): {
  before: string;
  after: string;
} {
  switch (status.state) {
    case "missing":
      return { before: "", after: " not found." };
    case "unavailable":
      return {
        before: "",
        after: " not found, and the app has no download of it for this computer.",
      };
    case "unusable":
      return { before: "", after: ` not used. ${status.reason}` };
    case "downloading":
      return {
        before: "Downloading ",
        after: `: ${downloadProgress(status.received, status.total)}.`,
      };
    case "downloadFailed":
      return { before: "", after: ` download failed. ${status.reason}` };
  }
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
 * src-tauri/src/tool_downloads.rs). Split around the section's heading, so
 * Settings can set the heading as a link to `section.url`:
 * `${before}${section.heading}${after}` is the sentence.
 */
export function troubleshootingSentence(name: ToolName): {
  before: string;
  section: { heading: string; url: string };
  after: string;
} {
  return {
    before: 'See "',
    section: troubleshootingSection(name),
    after: '" in Troubleshooting at messagecrate.app.',
  };
}
