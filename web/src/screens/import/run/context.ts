/**
 * What an Import Run works through, and the state one run keeps between its
 * stages. `createImportRunService` (`service.ts`) builds one of these, and
 * every function in `run/` takes it as its first argument, so two services
 * never share a run.
 */

import type * as desktopBackup from "../../../lib/desktop/backup";
import type * as desktopExtract from "../../../lib/desktop/extract";
import type * as desktopFs from "../../../lib/desktop/fs";
import type * as desktopJob from "../../../lib/desktop/job";
import type * as desktopRunDir from "../../../lib/desktop/runDir";
import type * as desktopRunLog from "../../../lib/desktop/runLog";
import type * as desktopStaging from "../../../lib/desktop/staging";
import type * as desktopTools from "../../../lib/desktop/tools";
import type * as desktopUpload from "../../../lib/desktop/upload";
import type * as importRun from "../../../lib/importRun";
import type * as serverApi from "../../../lib/serverApi";
import type { ImportRunStore } from "../importRunStore";
import type { RecordWrite } from "./runRecordWrites";
import type { RunScratch } from "./scratch";

/**
 * An Import Run open on the server, as its stages work on it: the run, its run
 * directory, and the plan approved at the last Review it passed.
 *
 * On a resume, `approved` is parsed from the run's stored `summary`
 * (`parseStoredStagingSummary`), and is undefined when the run recorded
 * nothing usable. The stages tolerate that absence, so a resume is not
 * blocked on a plan that can no longer be read. `importOutcome` then cannot
 * diff a resumed Upload's expected omissions against it, which demotes an
 * honest `completed` outcome to `completed_with_issues`.
 */
export type OpenRun = {
  runId: number;
  runDir: string;
  approved?: desktopStaging.StagingSummary;
};

/** The desktop side of a run: the Tauri commands and events it uses. */
type RunDesktop = {
  /** True inside the desktop app. A run does nothing anywhere else. */
  isTauri: () => boolean;
  awaitDesktopJob: typeof desktopJob.awaitDesktopJob;
  /** Stop the desktop job that is running. */
  invokeCancel: typeof desktopJob.invokeCancel;
  onDesktopJobEvents: typeof desktopJob.onDesktopJobEvents;
  invokeCreateRunDir: typeof desktopRunDir.invokeCreateRunDir;
  invokeDeleteRunDir: typeof desktopRunDir.invokeDeleteRunDir;
  invokeExtract: typeof desktopExtract.invokeExtract;
  invokeImessageBackupIdentities: typeof desktopBackup.invokeImessageBackupIdentities;
  invokePathStat: typeof desktopFs.invokePathStat;
  invokeReadImportRunRecord: typeof desktopRunDir.invokeReadImportRunRecord;
  invokeSaveImportRunRecord: typeof desktopRunDir.invokeSaveImportRunRecord;
  invokeStartImportRunLog: typeof desktopRunLog.invokeStartImportRunLog;
  invokeSummarizeStaging: typeof desktopStaging.invokeSummarizeStaging;
  invokeToolsStatus: typeof desktopTools.invokeToolsStatus;
  invokeTranscodeStaging: typeof desktopStaging.invokeTranscodeStaging;
  invokeUpload: typeof desktopUpload.invokeUpload;
};

/** The server side of a run: its routes, and the session they go with. */
type RunServer = {
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
