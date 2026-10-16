import { invoke } from "@tauri-apps/api/core";
import type { MediaToolName } from "../types";

/**
 * Where one program the desktop app runs is. Tagged by `state`, one tag per
 * state, so a state can be added beside the others without changing them.
 */
export type ToolStatus =
  | { state: "found"; path: string }
  | { state: "missing" }
  /** Missing, and the app has no download of it for this computer: only a copy put in the Tools Directory by hand is used. */
  | { state: "unavailable" }
  | { state: "unusable"; reason: string }
  /** Downloading into the Tools Directory: bytes so far, and the total when the server said. */
  | { state: "downloading"; received: number; total: number | null }
  /** The download failed and the program is not found. Tried again at the next start, or with Try again. */
  | { state: "downloadFailed"; reason: string };

/**
 * Where ffmpeg, ffprobe and wtsexporter are. ffmpeg and ffprobe are looked
 * for on PATH, then in the Tools Directory, and are taken from one place;
 * wtsexporter only from the Tools Directory.
 */
export interface ToolsStatus {
  toolsDir: string | null;
  /**
   * A check of the Tools Directory runs in this process now, the start-up
   * check or Try again. A program it has not looked at yet shows as missing
   * until it does. Another app's check on the same Tools Directory is not
   * counted: Try again reports it as `alreadyRunning`.
   */
  checking: boolean;
  ffmpeg: ToolStatus;
  ffprobe: ToolStatus;
  wtsexporter: ToolStatus;
}

/** Ask the desktop process where ffmpeg, ffprobe and wtsexporter are. */
export async function invokeToolsStatus(): Promise<ToolsStatus> {
  return invoke("tools_status");
}

/**
 * What Try again did. `started`: a check started in this process.
 * `alreadyRunning`: another check holds the lock on the Tools Directory (the
 * start-up check, an earlier Try again, or another app's check), and nothing
 * started. `noToolsDirectory`: the app has none (`toolsDir` is null).
 * `couldNotStart`: the check's thread could not be started, which each
 * program it would have downloaded shows as a failed download with that
 * reason.
 */
type RetryResult = "started" | "alreadyRunning" | "noToolsDirectory" | "couldNotStart";

/**
 * Check the Tools Directory again and download what is missing, as Try again
 * on the Import form asks. Returns at once, with what it did.
 */
export async function invokeRetryToolDownloads(): Promise<RetryResult> {
  return invoke("retry_tool_downloads");
}

/** Whether any of ffmpeg, ffprobe and wtsexporter is downloading now. */
export function toolsDownloading(status: ToolsStatus): boolean {
  return [status.ffmpeg, status.ffprobe, status.wtsexporter].some(
    (tool) => tool.state === "downloading",
  );
}

/**
 * Which of ffmpeg and ffprobe, which Convert media and Convert & Compress
 * media run, cannot be used. One still downloading can: the Media Stage waits
 * for it.
 */
export function ffmpegMissing(status: ToolsStatus): MediaToolName[] {
  return (["ffmpeg", "ffprobe"] as const).filter((name) => !toolUsable(status[name]));
}

/** Whether an import can run a program: it is found, or downloading, which the import waits for. */
export function toolUsable(status: ToolStatus): boolean {
  return status.state === "found" || status.state === "downloading";
}
