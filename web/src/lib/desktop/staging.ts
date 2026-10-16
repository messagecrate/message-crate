import { invoke } from "@tauri-apps/api/core";
import type { AttachmentMediaMode } from "../types";

/**
 * The staged directory `summarize_staging` and `transcode_staging` act on.
 *
 * It carries no media settings: `extract` recorded the run's in the directory,
 * and both commands read them from there, so they work to the values the
 * Import Run was started with.
 *
 * It carries no Staging Directory either. The desktop process keeps the
 * setting and the directories it made under it, and acts on a directory it made
 * wherever the setting points now, so changing the setting never strands a
 * run that started under the earlier one.
 */
export interface RunDirConfig {
  run_dir: string;
}

/** The Staging Directory, and the directory used when Settings name none. */
interface StagingDirectorySetting {
  root: string;
  defaultRoot: string;
}

/** The Staging Directory the desktop process keeps, and its default. */
export async function invokeStagingDirectory(): Promise<StagingDirectorySetting> {
  return invoke("staging_root");
}

/**
 * Store the Staging Directory, and return it with its default. An empty
 * string goes back to the default. Directories made under the earlier
 * setting keep working.
 */
export async function invokeSetStagingDirectory(root: string): Promise<StagingDirectorySetting> {
  return invoke("set_staging_root", { root });
}

/**
 * The disk space, in bytes, that `accountId`'s Import Runs on the Message
 * Crate `messageCrateId` take in the Staging Directory on this computer,
 * measured now.
 */
export async function invokeAccountStagingBytes(
  accountId: number,
  messageCrateId: string,
): Promise<number> {
  return invoke("account_staging_bytes", { args: { accountId, messageCrateId } });
}

/**
 * The disk space, in bytes, every Import Run on this computer takes in the
 * Staging Directory, for every account, measured now.
 */
export async function invokeAllStagingBytes(): Promise<number> {
  return invoke("all_staging_bytes");
}

/**
 * One sentence for each disk that holds the Staging Directory or the Scratch
 * Directory with less free than `LOW_DISK_SPACE_BYTES` in `staging`,
 * naming the disk, the directory and the space free. Empty when every disk
 * has more, or cannot say how much it has.
 */
export async function invokeLowDiskSpace(): Promise<string[]> {
  return invoke("low_disk_space");
}

/** How a staged attachment is expected to land against the size limit. */
export type SizeVerdict =
  | "fits_as_is"
  | "likely_fits"
  | "may_grow"
  | "probably_too_big"
  | "cannot_process";

/** One attachment the user should see before approving. */
export interface AttachmentForecast {
  path: string;
  name: string;
  sizeBytes: number;
  estimateBytes: number;
  verdict: SizeVerdict;
}

/** How many messages one of the owner's identities sent and received. */
export interface OwnerIdentityCount {
  identity: string;
  sent: number;
  received: number;
}

/** What a staged directory holds, recomputed for the first review. */
export interface StagingSummary {
  conversations: number;
  messages: number;
  contactIdentifiers: string[];
  /** Messages under the owner identity each was sent from or received at. */
  ownerIdentities: OwnerIdentityCount[];
  attachments: number;
  attachmentBytes: number;
  forecasts: AttachmentForecast[];
  /** Largest single attachment the upload accepts; what the verdicts were measured against. */
  assetMaxBytes: number;
  /** The attachment mode Staging recorded in the directory, under the form's name for it. */
  mediaMode: AttachmentMediaMode;
}

/** Recompute what a staged directory holds, for the first review. */
export async function invokeSummarizeStaging(config: RunDirConfig): Promise<StagingSummary> {
  return invoke("summarize_staging", {
    args: { runDir: config.run_dir },
  });
}

/**
 * Run the Media Stage over a staged directory, after the Staging Review
 * approves it. Reports through the `desktop-job:*` events like every other long
 * job, so `awaitDesktopJob` drives it exactly as it drives extract and the Upload.
 */
export async function invokeTranscodeStaging(config: RunDirConfig): Promise<void> {
  return invoke("transcode_staging", {
    args: { runDir: config.run_dir },
  });
}

/**
 * What `transcode_staging`'s job did, from its `desktop-job:finished` payload.
 * `TranscodeReport` (`staging`) has no serde derive: `transcode_staging`
 * (`src-tauri/src/commands/staging.rs`) hand-builds the payload with these
 * fields flat at the top level, alongside `summary` — not nested under a
 * `report` key — so this mirrors the wire shape exactly, snake_case included.
 */
export interface TranscodeFinishedReport {
  converted: number;
  skipped: number;
  too_large: number;
  failed: number;
  missing: number;
  repointed: number;
  bytes_before: number;
  bytes_after: number;
}
