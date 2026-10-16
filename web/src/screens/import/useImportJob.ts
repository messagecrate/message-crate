import { completionTextFor } from "../../components/import/ImportSummaryPanel";
import { getAccountId, getBaseUrl, getToken } from "../../lib/api";
import { useAuth } from "../../lib/authContext";
import { invokeImessageBackupIdentities } from "../../lib/desktop/backup";
import { invokeExtract } from "../../lib/desktop/extract";
import { invokePathStat } from "../../lib/desktop/fs";
import { awaitDesktopJob, invokeCancel, onDesktopJobEvents } from "../../lib/desktop/job";
import {
  invokeCreateRunDir,
  invokeDeleteRunDir,
  invokeReadImportRunRecord,
  invokeSaveImportRunRecord,
} from "../../lib/desktop/runDir";
import { invokeStartImportRunLog } from "../../lib/desktop/runLog";
import { invokeSummarizeStaging, invokeTranscodeStaging } from "../../lib/desktop/staging";
import { invokeToolsStatus } from "../../lib/desktop/tools";
import { invokeUpload } from "../../lib/desktop/upload";
import {
  type ActiveImportRun,
  discardImportRun,
  getActiveImportRun,
  setImportStage,
} from "../../lib/importRun";
import type { ImportJobFormValues } from "../../lib/importSources/types";
import { completeImport, createImport, getServerState } from "../../lib/serverApi";
import { isTauri } from "../../lib/tauriCheck";
import { useFetchAccountProfile } from "../../lib/useAccountProfile";
import { type ImportRunState, importRunStore, useImportRunState } from "./importRunStore";
import type { ImportRunDeps } from "./run/context";
import { createImportRunService, type ImportRunService, type RunAccount } from "./run/service";
import type { Resume } from "./run/staging";

/**
 * What the app's Import Run works through: the desktop app's commands, the
 * server's routes and the session logged in now, and the one store the
 * screen and the sidebar read.
 */
const appDeps: ImportRunDeps = {
  desktop: {
    isTauri,
    awaitDesktopJob,
    invokeCancel,
    onDesktopJobEvents,
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
  },
  server: {
    getServerState,
    createImport,
    completeImport,
    setImportStage,
    discardImportRun,
    getActiveImportRun,
    token: getToken,
    accountId: getAccountId,
    baseUrl: getBaseUrl,
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
    startError: state.startError,
    reviewError: state.reviewError,
    computingSummary: state.computingSummary,
    completionText:
      state.phase === "done" ? completionTextFor(state.summaryView?.status) : undefined,
    sourceIdentities: state.sourceIdentities,
    runDirDeleteFailure: state.runDirDeleteFailure,
    discardRun: service.discardRun,
    dismissRunDirDeleteFailure: service.dismissRunDirDeleteFailure,
    startImport: (form: ImportJobFormValues, resume?: Resume) =>
      service.startImport(account, form, resume),
    continueAfterIdentityStop: () => service.continueAfterIdentityStop(account),
    cancelIdentityStop: () => service.cancelIdentityStop(account),
    formEdited: () => service.formEdited(account),
    approve: () => service.approve(account),
    cancelRun: () => service.cancelRun(account),
    resumeAtReview: (importRun: ActiveImportRun, resumedForm: ImportJobFormValues) =>
      service.resumeAtReview(account, importRun, resumedForm),
    cancel: () => service.cancel(account),
    returnToForm: () => service.returnToForm(account),
  };
}
