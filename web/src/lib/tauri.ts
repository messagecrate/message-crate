import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { type DesktopJobName, holdDesktopJob } from "./desktopJob";
import type { LogLinesPage } from "./serverApi";
import type { components } from "./serverApi.types";
import type {
  AttachmentMediaMode,
  ExtractConfig,
  ExtractErrorEvent,
  ImportFileDoneEvent,
  ImportFileWrittenEvent,
  ImportIssueEvent,
  ImportProgressEvent,
} from "./types";

/** Start extracting a phone backup on the desktop backend. */
export async function invokeExtract(config: ExtractConfig): Promise<void> {
  return invoke("extract", {
    args: {
      source: config.source,
      path: config.path,
      outputDir: config.output_dir,
      backupPassword: config.backup_password ?? null,
      attachmentMedia: config.attachment_media ?? null,
      mediaMaxResolution: config.media_max_resolution ?? null,
      mediaMaxFps: config.media_max_fps ?? null,
      mediaMinSize: config.media_min_size ?? null,
      obfuscate: config.obfuscate ?? null,
      timezone: config.timezone ?? null,
      phoneCountry: config.phone_country ?? null,
      ownerPhones: config.owner_phones ?? null,
      ownerEmails: config.owner_emails ?? null,
      attachmentRoot: config.attachment_root ?? null,
      appleContacts: config.apple_contacts ?? null,
      whatsappKey: config.whatsapp_key ?? null,
      whatsappWa: config.whatsapp_wa ?? null,
      whatsappMedia: config.whatsapp_media ?? null,
      whatsappDb: config.whatsapp_db ?? null,
      whatsappBusiness: config.whatsapp_business ?? null,
      resume: config.resume ?? null,
      assetMaxBytes: config.asset_max_bytes,
    },
  });
}

/** Ask the desktop backend to stop the job that is currently running. */
export async function invokeCancel(): Promise<void> {
  return invoke("cancel");
}

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
export interface StagingRoot {
  root: string;
  defaultRoot: string;
}

/** The Staging Directory the desktop process keeps. */
export async function invokeStagingRoot(): Promise<StagingRoot> {
  return invoke("staging_root");
}

/**
 * Store the Staging Directory. An empty string goes back to the default.
 * Directories made under the earlier setting keep working.
 */
export async function invokeSetStagingRoot(root: string): Promise<StagingRoot> {
  return invoke("set_staging_root", { root });
}

/** Who ran an Import Run, on which Message Crate: the first line of the run's log. */
export interface RunLogAccount {
  importRunId: number;
  accountId: number;
  /** The server's address, for a person to read. */
  server: string;
  /**
   * The Message Crate's id, as `GET /v1/server` answers it: what tells two
   * Message Crates at one address apart.
   */
  messageCrateId: string;
}

/**
 * Start a new Import Run's log with the line that names the account running
 * it and the server, so the Logs panel shows the log to that account and the
 * owner only.
 */
export async function invokeStartImportRunLog(
  runDir: string,
  account: RunLogAccount,
): Promise<void> {
  return invoke("start_import_run_log", { args: { runDir, account } });
}

/** Who is asking for the Import Run logs on this computer. */
export interface RunLogReader {
  /** The id of the Message Crate the window is signed in to. */
  messageCrateId: string;
  accountId: number;
  /** The owner reads every run log; an account only those of its own runs. */
  owner: boolean;
}

/** One Import Run log on this computer. */
export interface RunLogEntry {
  /** The file's name in the Logs Directory, which a download keeps. */
  name: string;
  /** Who ran the run, or null when the log does not say. */
  account: RunLogAccount | null;
  /** Whether the run imported into the Message Crate the reader is signed in to. */
  thisMessageCrate: boolean;
  /**
   * Whether its first lines have a time and a level. A log written before
   * its lines carried them has none the viewer reads, though it downloads.
   */
  hasLines: boolean;
  bytes: number;
  /** When its last line was written, in UTC (RFC 3339). */
  modifiedAt: string;
}

/** The Import Run logs on this computer that `reader` may read, newest first. */
export async function invokeListImportRunLogs(reader: RunLogReader): Promise<RunLogEntry[]> {
  return invoke("list_import_run_logs", { reader });
}

/**
 * The levels a run log writes. The desktop app refuses `debug` and `trace`,
 * which only the server's log has.
 */
export type RunLogLevel = "error" | "warn" | "info";

/**
 * A page of one Import Run log's lines, newest first: those at `level` and
 * more severe (every line when absent), holding `text`, older than the line
 * whose id is `after`. The page has the shape the server's log answers.
 */
export async function invokeReadImportRunLogLines(
  reader: RunLogReader,
  name: string,
  query: {
    level?: RunLogLevel;
    text?: string;
    after?: number;
    limit: number;
  },
): Promise<LogLinesPage> {
  return invoke("read_import_run_log_lines", { args: { reader, name, query } });
}

/** One Import Run log whole, byte for byte as it is on disk, for a download. */
export async function invokeReadImportRunLog(
  reader: RunLogReader,
  name: string,
): Promise<ArrayBuffer> {
  return invoke("read_import_run_log", { args: { reader, name } });
}

/**
 * The log of the Import Run whose directory is `runDir`, in the Logs
 * Directory, where it stays after the run's directory is deleted.
 */
export async function invokeImportRunLog(runDir: string): Promise<string> {
  return invoke("import_run_log", { runDir });
}

/**
 * Make a new run directory under the Staging Directory and return its path.
 * `label` is the Import source.
 */
export async function invokeCreateRunDir(label: string): Promise<string> {
  return invoke("create_run_dir", { label });
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
 * Run the Media stage over a staged directory, after the Staging Review
 * approves it. Reports through the `extract:*` events like every other long
 * job, so `awaitTauriJob` drives it exactly as it drives extract and the Upload.
 */
export async function invokeTranscodeStaging(config: RunDirConfig): Promise<void> {
  return invoke("transcode_staging", {
    args: { runDir: config.run_dir },
  });
}

/**
 * Delete a run directory — the decline path's terminal action: closing an
 * review without approving deletes the directory outright.
 */
export async function invokeDeleteRunDir(config: { run_dir: string }): Promise<void> {
  return invoke("delete_run_dir", {
    args: { runDir: config.run_dir },
  });
}

/**
 * Read the Import Run record kept in a run directory
 * (`read_import_run_record`): what a paused run's earlier parts recorded.
 * Null when the directory holds none. The caller checks its shape.
 */
export async function invokeReadImportRunRecord(config: {
  run_dir: string;
}): Promise<unknown | null> {
  return invoke("read_import_run_record", {
    args: { runDir: config.run_dir },
  });
}

/** Write the Import Run record into its run directory (`save_import_run_record`). */
export async function invokeSaveImportRunRecord(config: {
  run_dir: string;
  record: unknown;
}): Promise<void> {
  return invoke("save_import_run_record", {
    args: { runDir: config.run_dir, record: config.record },
  });
}

export interface UploadConfig {
  base_url: string;
  token: string;
  input_dir: string;
  mode: string;
  skip_attachments: boolean;
  trust_export: boolean;
  import_id?: number;
}

export interface UploadFinishedReport {
  ok: boolean;
  /** The cancel flag paused the Upload: the run resumes from it, so it is not a failure. */
  cancelled: boolean;
  /**
   * The server refused the session the Upload sent (it expired or was ended),
   * which paused the Upload. The window ends the session too.
   */
  session_refused: boolean;
  messages_attempted: number;
  messages_inserted: number;
  messages_deduped: number;
  messages_failed: number;
  assets_uploaded: number;
  assets_bytes: number;
  conversations_ok: number;
  conversations_total: number;
  conversations_failed: number;
  conversations_skipped: number;
  /** Conversations a pause left unsent; the next Upload sends them. */
  conversations_cancelled: number;
  results: Array<{
    file: string;
    status: string;
    error?: string;
    messages: number;
    attachments: number;
  }>;
}

/**
 * What `transcode_staging`'s job did, from its `extract:finished` payload.
 * `TranscodeReport` (`message-staging`) has no serde derive: `transcode_staging`
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

export interface TauriJobResult {
  summary: string;
  report?: UploadFinishedReport;
  extraction?: {
    files_parsed: number;
    messages_parsed: number;
  };
  transcode?: TranscodeFinishedReport;
}

/** Upload extracted conversations to a server. */
export async function invokeUpload(config: UploadConfig): Promise<void> {
  return invoke("upload", {
    args: {
      baseUrl: config.base_url,
      token: config.token,
      inputDir: config.input_dir,
      mode: config.mode,
      skipAttachments: config.skip_attachments,
      trustExport: config.trust_export,
      importId: config.import_id ?? null,
    },
  });
}

/**
 * The list an export's search is for: the messages it matches on the
 * Messages list, or every message of the conversations it shows on the
 * Conversations list. The server's own type, so the two cannot drift.
 */
export type ExportQueryList = components["schemas"]["ExportQueryList"];

export interface ExportConfig {
  base_url: string;
  token: string;
  out_dir: string;
  /** Blank exports everything the account holds. */
  query: string;
  /** The list `query` is for. Unused when `query` is blank. */
  list: ExportQueryList;
  skip_attachments: boolean;
}

/** Download conversations from a server into a directory. */
export async function invokeExport(config: ExportConfig): Promise<void> {
  return invoke("export", {
    args: {
      baseUrl: config.base_url,
      token: config.token,
      outDir: config.out_dir,
      query: config.query,
      list: config.list,
      skipAttachments: config.skip_attachments,
    },
  });
}

/**
 * File formats `message-reexport` can write, in the order the Export screen
 * offers them. The ids are the strings the `format` command parses
 * (`src-tauri/src/commands/format.rs`); anything else is rejected there.
 */
export const EXPORT_FORMATS = [
  { id: "jsonl", label: "JSON Lines (.jsonl)" },
  { id: "json", label: "JSON (.json)" },
  { id: "csv", label: "CSV (.csv)" },
  { id: "eml", label: "EML (one file per message)" },
  { id: "mbox", label: "MBOX (.mbox)" },
  { id: "xml", label: "Android XML (smses.xml)" },
  { id: "sms-backup-plus", label: "EML (SMS Backup+)" },
] as const;

/** Id of a format the Export screen can write. */
export type ExportFormat = (typeof EXPORT_FORMATS)[number]["id"];

/**
 * The directory an Export or Convert gets of its own in the Export Directory,
 * and where an Export keeps its in-between files inside it.
 */
export interface ExportDir {
  /** Where the result lands unless another destination is chosen. */
  dir: string;
  /** Where an Export writes its JSON Lines before converting them. */
  exported: string;
  /** Where an Export's conversion writes when the result lands in `dir`. */
  converting: string;
}

/** The Export Directory, for Settings. */
export async function invokeExportDirectory(): Promise<string> {
  return invoke("export_directory");
}

/**
 * Make the directory of a new Export or Convert in the Export Directory.
 * `chosen` is the destination the person chose, or empty; one that holds the
 * Export Directory is refused before anything is made.
 */
export async function invokeCreateExportDir(
  kind: "export" | "convert",
  format: ExportFormat,
  chosen: string,
): Promise<ExportDir> {
  return invoke("create_export_dir", { kind, format, chosen: chosen || null });
}

/**
 * Finish an Export's or Convert's directory after it succeeded: its
 * in-between files are deleted and only the result is left. Returns the
 * directory, or null when the result went elsewhere and the directory was
 * deleted.
 */
export async function invokeFinishExportDir(dir: string): Promise<string | null> {
  return invoke("finish_export_dir", { dir });
}

/** Delete an Export's or Convert's directory after it failed or was cancelled. */
export async function invokeDiscardExportDir(dir: string): Promise<void> {
  return invoke("discard_export_dir", { dir });
}

/**
 * Rewrite an export directory into another format.
 *
 * `input_dir` and `output_dir` must differ: `message-reexport` canonicalizes
 * both and refuses to write into its own input.
 */
export async function invokeFormat(config: {
  input_dir: string;
  output_dir: string;
  output_format: ExportFormat;
  /** The screen that started the run, which the desktop names it by when it
   * refuses another one while this runs. */
  started_from: "export" | "convert";
  /** When the Export Run started, in epoch milliseconds: SMS Backup+ mail
   * records it as its backup time. Left out by Settings → Convert, which is a
   * run of its own. */
  run_started_ms?: number;
}): Promise<void> {
  return invoke("format", {
    inputDir: config.input_dir,
    outputDir: config.output_dir,
    outputFormat: config.output_format,
    startedFrom: config.started_from,
    runStartedMs: config.run_started_ms ?? null,
  });
}

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
/** A program the desktop app keeps in its Tools Directory, as `tools_status` names it. */
export type ToolName = "ffmpeg" | "ffprobe" | "wtsexporter";

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
export type RetryResult = "started" | "alreadyRunning" | "noToolsDirectory" | "couldNotStart";

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

/** The programs Convert and Compress run. */
export type MediaToolName = "ffmpeg" | "ffprobe";

/**
 * Which of ffmpeg and ffprobe, which Convert and Compress run, cannot be used.
 * One still downloading can: the Media stage waits for it.
 */
export function ffmpegMissing(status: ToolsStatus): MediaToolName[] {
  return (["ffmpeg", "ffprobe"] as const).filter((name) => !toolUsable(status[name]));
}

/** Whether an import can run a program: it is found, or downloading, which the import waits for. */
export function toolUsable(status: ToolStatus): boolean {
  return status.state === "found" || status.state === "downloading";
}

export interface HomeDirInfo {
  path: string;
  os: string;
}

/** User home directory and operating system name from the desktop backend. */
export async function invokeHomeDir(): Promise<HomeDirInfo> {
  return invoke("home_dir");
}

export interface PathStat {
  exists: boolean;
  isFile: boolean;
  isDirectory: boolean;
  sizeBytes: number;
  modifiedUnixMs: number | null;
}

/**
 * Show the Save dialog with `fileName` filled in, and have the desktop app
 * download `url`, a Media Link to an attachment's original, to the file the
 * person chose. The app writes the server's answer to the disk as it arrives,
 * so a video of hundreds of megabytes is never held in the window or sent to
 * the app whole (#1739).
 *
 * Resolves false when the person closed the dialog without choosing a place,
 * and true once the file is written.
 */
export async function invokeSaveDownload(url: string, fileName: string): Promise<boolean> {
  return invoke<boolean>("save_download", { url, fileName });
}

/** Whether a path exists and whether it is a file or directory. */
export async function invokePathStat(path: string): Promise<PathStat> {
  return invoke("path_stat", { path });
}

/** Whether an iOS backup directory is encrypted, or null when unknown. */
export async function invokeIosBackupEncrypted(path: string): Promise<boolean | null> {
  return invoke("ios_backup_encrypted", { path });
}

/**
 * Addresses an iMessage backup's device sent from. The desktop opens the
 * source the same way the extractor will, so a source it cannot read fails
 * here with the message the extractor would give.
 */
export async function invokeImessageBackupIdentities(args: {
  path: string;
  ios: boolean;
  backupPassword: string;
}): Promise<string[]> {
  return invoke("imessage_backup_identities", {
    path: args.path,
    ios: args.ios,
    backupPassword: args.backupPassword.trim() === "" ? null : args.backupPassword,
  });
}

/**
 * Listen for job events from the desktop backend (log lines, progress, errors).
 * Returns one function that removes every listener.
 */
export function onExtractEvents(callbacks: {
  onLog: (line: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
  onFileDone?: (event: ImportFileDoneEvent) => void;
  onFileWritten?: (event: ImportFileWrittenEvent) => void;
  onFinished: (summary: string) => void;
  onError: (err: ExtractErrorEvent) => void;
}): Promise<UnlistenFn> {
  return Promise.all([
    listen<string>("extract:log", (e) => callbacks.onLog(e.payload)),
    listen<ImportProgressEvent>("extract:progress", (e) => callbacks.onProgress?.(e.payload)),
    listen<ImportIssueEvent>("extract:issue", (e) => callbacks.onIssue?.(e.payload)),
    listen<ImportFileDoneEvent>("extract:file-done", (e) => callbacks.onFileDone?.(e.payload)),
    listen<ImportFileWrittenEvent>("extract:file-written", (e) =>
      callbacks.onFileWritten?.(e.payload),
    ),
    listen<string>("extract:finished", (e) => callbacks.onFinished(e.payload)),
    listen<ExtractErrorEvent>("extract:error", (e) => callbacks.onError(e.payload)),
  ]).then((unlisteners) => {
    return () => {
      for (const u of unlisteners) {
        u();
      }
    };
  });
}

/**
 * Run a desktop job and wait until it finishes.
 * Extract and upload return as soon as the background thread starts, so callers
 * must use this instead of awaiting the invoke call alone.
 *
 * `job` names the screen's job while it runs (`desktopJob.ts`), so the other
 * screens keep their Start buttons off until it ends.
 */
export async function awaitTauriJob(
  job: DesktopJobName,
  invokeFn: () => Promise<void>,
  onLog?: (line: string) => void,
  onProgress?: (event: ImportProgressEvent) => void,
  onIssue?: (event: ImportIssueEvent) => void,
  onFileDone?: (event: ImportFileDoneEvent) => void,
  onFileWritten?: (event: ImportFileWrittenEvent) => void,
): Promise<TauriJobResult> {
  let unlisten: UnlistenFn | undefined;
  const release = holdDesktopJob(job);
  try {
    return await new Promise<TauriJobResult>((resolve, reject) => {
      void (async () => {
        try {
          unlisten = await onExtractEvents({
            onLog: (line) => onLog?.(line),
            onProgress,
            onIssue,
            onFileDone,
            onFileWritten,
            onFinished: (summary) => resolve(parseTauriJobResult(summary)),
            onError: (err) => reject(new Error(err.user_message ?? err.detail)),
          });
          await invokeFn();
        } catch (e: unknown) {
          reject(e instanceof Error ? e : new Error(String(e)));
        }
      })();
    });
  } finally {
    unlisten?.();
    release();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isUploadFinishedReport(value: unknown): value is UploadFinishedReport {
  if (!isRecord(value)) return false;
  return (
    typeof value.ok === "boolean" &&
    typeof value.cancelled === "boolean" &&
    typeof value.session_refused === "boolean" &&
    typeof value.messages_attempted === "number" &&
    typeof value.messages_inserted === "number" &&
    typeof value.messages_deduped === "number" &&
    typeof value.messages_failed === "number" &&
    typeof value.assets_uploaded === "number" &&
    typeof value.assets_bytes === "number" &&
    typeof value.conversations_ok === "number" &&
    typeof value.conversations_total === "number" &&
    typeof value.conversations_failed === "number" &&
    typeof value.conversations_skipped === "number" &&
    typeof value.conversations_cancelled === "number"
  );
}

function isTranscodeFinishedReport(value: unknown): value is TranscodeFinishedReport {
  if (!isRecord(value)) return false;
  return (
    typeof value.converted === "number" &&
    typeof value.skipped === "number" &&
    typeof value.too_large === "number" &&
    typeof value.failed === "number" &&
    typeof value.missing === "number" &&
    typeof value.repointed === "number" &&
    typeof value.bytes_before === "number" &&
    typeof value.bytes_after === "number"
  );
}

/** Turn a finished-job summary string into a structured result when it is JSON. */
export function parseTauriJobResult(summary: string): TauriJobResult {
  try {
    const parsed: unknown = JSON.parse(summary);
    if (!isRecord(parsed)) return { summary };

    if (
      typeof parsed.summary === "string" &&
      typeof parsed.files_parsed === "number" &&
      typeof parsed.messages_parsed === "number"
    ) {
      return {
        summary: parsed.summary,
        extraction: {
          files_parsed: parsed.files_parsed,
          messages_parsed: parsed.messages_parsed,
        },
      };
    }

    const summaryText = typeof parsed.summary === "string" ? parsed.summary : summary;

    if (isUploadFinishedReport(parsed)) {
      return {
        summary: summaryText,
        report: parsed,
      };
    }

    if (isTranscodeFinishedReport(parsed)) {
      // Picked field by field, not spread: `parsed` also carries the raw
      // `summary` string this same object holds the report fields
      // alongside, which isn't part of `TranscodeFinishedReport`.
      return {
        summary: summaryText,
        transcode: {
          converted: parsed.converted,
          skipped: parsed.skipped,
          too_large: parsed.too_large,
          failed: parsed.failed,
          missing: parsed.missing,
          repointed: parsed.repointed,
          bytes_before: parsed.bytes_before,
          bytes_after: parsed.bytes_after,
        },
      };
    }
  } catch {
    // Format and export jobs send a plain sentence, not JSON.
  }
  return { summary };
}
