/**
 * What an Import Run works through, and the state one run keeps between its
 * stages. `createImportRunService` (`service.ts`) builds one of these, and
 * every function in `run/` takes it as its first argument, so two services
 * never share a run.
 */

import type * as importRun from "../../../lib/importRun";
import type * as serverApi from "../../../lib/serverApi";
import type * as tauri from "../../../lib/tauri";
import type { ImportRunStore } from "../importRunStore";
import type { RecordWrite } from "./runRecordWrites";
import type { RunScratch } from "./scratch";

/** The desktop side of a run: the Tauri commands and events it uses. */
export type RunDesktop = {
  /** True inside the desktop app. A run does nothing anywhere else. */
  isTauri: () => boolean;
  awaitDesktopJob: typeof tauri.awaitDesktopJob;
  /** Stop the desktop job that is running. */
  invokeCancel: typeof tauri.invokeCancel;
  onDesktopJobEvents: typeof tauri.onDesktopJobEvents;
  invokeCreateRunDir: typeof tauri.invokeCreateRunDir;
  invokeDeleteRunDir: typeof tauri.invokeDeleteRunDir;
  invokeExtract: typeof tauri.invokeExtract;
  invokeImessageBackupIdentities: typeof tauri.invokeImessageBackupIdentities;
  invokePathStat: typeof tauri.invokePathStat;
  invokeReadImportRunRecord: typeof tauri.invokeReadImportRunRecord;
  invokeSaveImportRunRecord: typeof tauri.invokeSaveImportRunRecord;
  invokeStartImportRunLog: typeof tauri.invokeStartImportRunLog;
  invokeSummarizeStaging: typeof tauri.invokeSummarizeStaging;
  invokeToolsStatus: typeof tauri.invokeToolsStatus;
  invokeTranscodeStaging: typeof tauri.invokeTranscodeStaging;
  invokeUpload: typeof tauri.invokeUpload;
};

/** The server side of a run: its routes, and the session they go with. */
export type RunServer = {
  getServerState: typeof serverApi.getServerState;
  createImport: typeof serverApi.createImport;
  completeImport: typeof serverApi.completeImport;
  setImportStage: typeof importRun.setImportStage;
  discardImportRun: typeof importRun.discardImportRun;
  getActiveImportRun: typeof importRun.getActiveImportRun;
  /** The session token logged in now, which the run's server calls send. */
  token: () => string | null;
  /** The account logged in now. */
  accountId: () => number | null;
  /** The address of the Message Crate logged in to. */
  baseUrl: () => string;
};

/** What `createImportRunService` is built from. */
export type ImportRunDeps = {
  desktop: RunDesktop;
  server: RunServer;
  /** Where the screen reads the run. */
  store: ImportRunStore;
  /** Milliseconds from a fixed start, for the run's timings. */
  now: () => number;
};

/** One service's run: what it works through, and the state it keeps. */
export type RunContext = ImportRunDeps & {
  /** The scratch of the run in the store now. `reset` replaces it. */
  scratch: RunScratch;
  /** Every write of the run record so far, in order: settles when the last has. */
  recordWrites: Promise<void>;
  /** The write queued behind the one in progress, not yet started. */
  queuedRecordWrite: RecordWrite | null;
  /**
   * What the Import screen's actions are doing now, whichever account started
   * them: a stage, a probe, a Review being cancelled. Settles once they stop.
   */
  work: Promise<void> | null;
};
