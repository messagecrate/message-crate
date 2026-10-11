import { completionTextFor } from "../../components/import/ImportSummaryPanel";
import { getAccountId, getBaseUrl, getToken } from "../../lib/api";
import { useAuth } from "../../lib/authContext";
import {
  type ActiveImportRun,
  discardImportRun,
  getActiveImportRun,
  setImportStage,
} from "../../lib/importRun";
import type { ImportJobFormValues } from "../../lib/importSources/types";
import { completeImport, createImport, getServerState } from "../../lib/serverApi";
import {
  awaitDesktopJob,
  invokeCancel,
  invokeCreateRunDir,
  invokeDeleteRunDir,
  invokeExtract,
  invokeImessageBackupIdentities,
  invokePathStat,
  invokeReadImportRunRecord,
  invokeSaveImportRunRecord,
  invokeStartImportRunLog,
  invokeSummarizeStaging,
  invokeToolsStatus,
  invokeTranscodeStaging,
  invokeUpload,
  onDesktopJobEvents,
} from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { useFetchAccountProfile } from "../../lib/useAccountProfile";
import { type ImportRunState, importRunStore, useImportRunState } from "./importRunStore";
import type { ImportRunDeps } from "./run/context";
import { createImportRunService, type ImportRunService, type RunAccount } from "./run/service";
import type { ResumeUpload, ResumeWrite } from "./run/staging";

export type { ImportPhase, ImportStep } from "./importProgressState";

/**
 * What the app's Import Run works through: the desktop app's commands, the
 * server's routes and the session logged in now, and the one store the
 * screen and the sidebar read.
 *
 * Every entry, `desktop` and `server` alike, calls its import when the run
 * calls it, rather than holding the import itself, so building this reads no
 * export. A test that loads this file with a `vi.mock` of an app module
 * without `importOriginal` gives only the exports it uses, and Vitest throws
 * on reading an export its mock lacks, so holding the imports would fail
 * such a test at load.
 */
const appDeps: ImportRunDeps = {
  desktop: {
    isTauri: () => isTauri(),
    awaitDesktopJob: (...args) => awaitDesktopJob(...args),
    invokeCancel: () => invokeCancel(),
    onDesktopJobEvents: (callbacks) => onDesktopJobEvents(callbacks),
    invokeCreateRunDir: (...args) => invokeCreateRunDir(...args),
    invokeDeleteRunDir: (...args) => invokeDeleteRunDir(...args),
    invokeExtract: (...args) => invokeExtract(...args),
    invokeImessageBackupIdentities: (...args) => invokeImessageBackupIdentities(...args),
    invokePathStat: (...args) => invokePathStat(...args),
    invokeReadImportRunRecord: (...args) => invokeReadImportRunRecord(...args),
    invokeSaveImportRunRecord: (...args) => invokeSaveImportRunRecord(...args),
    invokeStartImportRunLog: (...args) => invokeStartImportRunLog(...args),
    invokeSummarizeStaging: (...args) => invokeSummarizeStaging(...args),
    invokeToolsStatus: () => invokeToolsStatus(),
    invokeTranscodeStaging: (...args) => invokeTranscodeStaging(...args),
    invokeUpload: (...args) => invokeUpload(...args),
  },
  server: {
    getServerState: (...args) => getServerState(...args),
    createImport: (...args) => createImport(...args),
    completeImport: (...args) => completeImport(...args),
    setImportStage: (...args) => setImportStage(...args),
    discardImportRun: (...args) => discardImportRun(...args),
    getActiveImportRun: (...args) => getActiveImportRun(...args),
    token: () => getToken(),
    accountId: () => getAccountId(),
    baseUrl: () => getBaseUrl(),
  },
  store: importRunStore,
  now: () => performance.now(),
};

/**
 * The app's one Import Run. It is built once, here, because the run does not
 * stop when the person leaves the Import screen (`importRunStore.ts`).
 */
export const importRunService: ImportRunService = createImportRunService(appDeps);

/**
 * The Import screen's view of `service`'s run for the account logged in: the
 * run as it stands, and the actions on it, each taken for that account. The
 * run itself is the service's: this hook keeps no state of its own.
 */
export function useImportJob(service: ImportRunService = importRunService) {
  const fetchProfile = useFetchAccountProfile();
  const auth = useAuth();
  const account: RunAccount = {
    token: auth.token,
    accountId: auth.accountId ?? null,
    fetchProfile: () => fetchProfile(),
  };
  // The logged-in account's run, or the fresh form when another account's
  // run is in the store (#1085).
  const state: ImportRunState = useImportRunState(service.store);

  return {
    phase: state.phase,
    steps: state.steps,
    running: state.running,
    form: state.form,
    summaryView: state.summaryView,
    runDir: state.runDir,
    importRunId: state.importRunId,
    stagingSummary: state.stagingSummary,
    mediaSummary: state.mediaSummary,
    mediaFailedCount: state.mediaFailedCount,
    mediaToolsMissing: state.mediaToolsMissing,
    mediaPartiallyRan: state.mediaPartiallyRan,
    resumeError: state.resumeError,
    reviewError: state.reviewError,
    computingSummary: state.computingSummary,
    completionText:
      state.phase === "done" ? completionTextFor(state.summaryView?.status) : undefined,
    sourceIdentities: state.sourceIdentities,
    runDirDeleteFailure: state.runDirDeleteFailure,
    discardRun: service.discardRun,
    dismissRunDirDeleteFailure: service.dismissRunDirDeleteFailure,
    startImport: (form: ImportJobFormValues, resume?: ResumeUpload, resumeWrite?: ResumeWrite) =>
      service.startImport(account, form, resume, resumeWrite),
    continueAfterIdentityStop: () => service.continueAfterIdentityStop(account),
    cancelIdentityStop: () => service.cancelIdentityStop(account),
    approve: () => service.approve(account),
    cancelRun: () => service.cancelRun(account),
    resumeAtReview: (importRun: ActiveImportRun, resumedForm: ImportJobFormValues) =>
      service.resumeAtReview(account, importRun, resumedForm),
    cancel: () => service.cancel(account),
    returnToForm: () => service.returnToForm(account),
  };
}
