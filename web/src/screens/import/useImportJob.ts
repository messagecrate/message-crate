import { completionTextFor } from "../../components/import/ImportSummaryPanel";
import { profileAddresses } from "../../lib/account";
import { useAuth } from "../../lib/authContext";
import { needsIdentityStop, parseSourceIdentities } from "../../lib/backupIdentity";
import type { ActiveImportRun } from "../../lib/importRun";
import { importSourceFor } from "../../lib/importSources";
import type { ImportJobFormValues } from "../../lib/importSources/types";
import { createRunCancel } from "../../lib/runCancel";
import { invokeImessageBackupIdentities } from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { useFetchAccountProfile } from "../../lib/useAccountProfile";
import {
  CLEARED_RUN,
  type ImportRunState,
  importRunStore as store,
  useImportRunState,
} from "./importRunStore";
import { mediaJobVerb } from "./reviewForecast";
import { summarizeStagingWithProgress } from "./run/desktopJob";
import { discardRun } from "./run/finish";
import { mediaToolsMissingFor, runMediaStage } from "./run/media";
import { dismissRunDirDeleteFailure } from "./run/runDirectory";
import { beginRun, loadCarriedRecord, resetImportRun, runScratch } from "./run/scratch";
import { resumeSteps, returnToForm, returnToFormWithError, waitAtReview } from "./run/screen";
import { accountLeft, moveStageAtReview } from "./run/serverCalls";
import { type ResumeUpload, type ResumeWrite, runImport } from "./run/staging";
import {
  adoptRecordedMode,
  parseStoredStagingSummary,
  withRecordedMode,
} from "./run/stagingSummary";
import { runUpload } from "./run/upload";

export type { ImportPhase, ImportStep } from "./importProgressState";

/**
 * What the Import screen's actions are doing now, whichever account started
 * them: a stage, a probe, a Review being cancelled. Settles once they stop.
 * `takeRunFor` waits on it before another account's run replaces them.
 */
let work: Promise<void> | null = null;

/** Run `action` as the Import screen's work of the moment (`work`). */
function asWork(action: () => Promise<void>): Promise<void> {
  const tracked = action().finally(() => {
    if (work === tracked) work = null;
  });
  work = tracked;
  return tracked;
}

/**
 * Make the store `accountId`'s, before that account starts or resumes a run.
 *
 * A store already the account's is left as it is. A run another account
 * started is stopped and let settle first, so nothing it does afterwards
 * lands in the new run: its stage is cancelled, which leaves the run open on
 * the server for that account to resume from its run directory. Then the
 * store goes back to a fresh form, named with `accountId` (#1085).
 */
async function takeRunFor(accountId: number | null): Promise<void> {
  while (store.get().accountId !== accountId) {
    const running = work;
    if (running == null) {
      resetImportRun();
      store.set({ accountId });
      return;
    }
    await runScratch().runCancel.cancel();
    await running.catch(() => {});
  }
}

/** Cancel the run at a Review: end it (`discardRun`) and go back to the form. */
async function cancelRun(): Promise<void> {
  if (runScratch().reviewAction) return;
  runScratch().reviewAction = true;
  try {
    const { importRunId: runId, runDir: outputDir } = store.get();
    await discardRun(runId, outputDir);
  } finally {
    runScratch().reviewAction = false;
  }
  returnToForm();
}

/** Stop the stage that is running. The run stays where it got to. */
async function cancel(): Promise<void> {
  await runScratch().runCancel.cancel();
}

/**
 * Run an import through its stages and reviews, and keep the run's
 * state where the screen can read it (`importRunStore`).
 *
 * Every function here reads the run from the store at the moment it is
 * called, never from a render's closure, so a screen that unmounted while a
 * stage ran and mounted again finds the run where it left it.
 */
export function useImportJob() {
  const fetchAccountProfile = useFetchAccountProfile();
  const auth = useAuth();
  const token = auth.token;
  const accountId = auth.accountId ?? null;
  // The logged-in account's run, or the fresh form when another account's
  // run is in the store (#1085).
  const state: ImportRunState = useImportRunState();

  /** True when the run in the store is this account's to act on. */
  function ownsRun(): boolean {
    return store.get().accountId === accountId;
  }

  /**
   * Start an import. For a fresh iMessage start this first reads which
   * addresses the backup's device sent from and compares them to the
   * profile; when nothing matches, it parks the form and stops at
   * `identity_stop`, before any run exists, so Cancel has nothing to clean
   * up. The probe fails open: a source it cannot read will fail in the
   * extractor moments later with the proper error.
   */
  async function startImport(
    form: ImportJobFormValues,
    resume?: ResumeUpload,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
    if (!isTauri()) return;
    await takeRunFor(accountId);
    // A second call while one is already probing or running is a no-op.
    if (runScratch().startImport) return;
    runScratch().startImport = true;
    const started = runScratch();
    await asWork(async () => {
      try {
        await startRun(form, resume, resumeWrite);
      } finally {
        started.startImport = false;
      }
    });
  }

  /** `startImport`, once the store is this account's and no other start is under way. */
  async function startRun(
    form: ImportJobFormValues,
    resume?: ResumeUpload,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
    let identities: string[] | null = null;
    const identityRead =
      resume || resumeWrite ? null : importSourceFor(form.source).appleIdentityRead(form.source);
    if (identityRead) {
      // The probe reads the backup (and, for an encrypted one, decrypts
      // it) before any run exists, which can take seconds: mark the run
      // busy for that stretch so the Import button reflects it.
      store.set({ running: true });
      try {
        identities = await invokeImessageBackupIdentities({
          path: form.backupPath,
          ios: identityRead.ios,
          backupPassword: form.backupPassword,
        }).catch(() => []);
        store.set({ sourceIdentities: identities });
        const profile = await fetchAccountProfile();
        // The account logged out during the probe: the profile just read
        // is another account's, and no run exists yet to resume.
        if (accountLeft()) return;
        if (needsIdentityStop(identities, profile && profileAddresses(profile))) {
          runScratch().pendingIdentityForm = form;
          store.set({ phase: "identity_stop" });
          return;
        }
      } finally {
        store.set({ running: false });
      }
    } else {
      store.set({ sourceIdentities: resumeWrite ? (resumeWrite.identities ?? null) : null });
    }
    await runImport(token, form, identities, resume, resumeWrite);
  }

  /** Continue past the identity stop with the parked form. */
  async function continueAfterIdentityStop(): Promise<void> {
    if (!ownsRun()) return;
    const form = runScratch().pendingIdentityForm;
    if (!form) return;
    runScratch().pendingIdentityForm = null;
    await asWork(() => runImport(token, form, store.get().sourceIdentities));
  }

  /** Leave the identity stop; nothing was created, so only the phase moves. */
  function cancelIdentityStop(): void {
    if (!ownsRun()) return;
    runScratch().pendingIdentityForm = null;
    returnToForm();
  }

  /** Approve the waiting review: Media after the Staging Review when there is one, Upload otherwise. */
  async function approve(): Promise<void> {
    if (!isTauri()) return;
    if (!ownsRun()) return;
    if (runScratch().reviewAction) return;
    const form = runScratch().form;
    const {
      phase,
      importRunId: runId,
      runDir: outputDir,
      stagingSummary,
      mediaSummary,
      reviewError,
    } = store.get();
    // What the person is approving: the directory as Media left it at the
    // Media Review, as Staging left it at the Staging Review.
    const approvedSummary = phase === "media_review" ? mediaSummary : stagingSummary;
    if (!form || runId == null || outputDir == null || approvedSummary == null) return;

    runScratch().reviewAction = true;
    runScratch().runCancel = createRunCancel();
    const approving = runScratch();
    await asWork(async () => {
      try {
        // The review's own stage did not reach the server, so it is written
        // first: a later visit must find the run at this review.
        if (reviewError != null) {
          const recorded =
            phase === "media_review"
              ? await moveStageAtReview(runId, "media_review", stagingSummary ?? undefined)
              : await moveStageAtReview(runId, "staging_review");
          if (recorded !== "recorded") return;
        }
        if (phase === "staging_review" && mediaJobVerb(form.attachmentMedia) !== null) {
          await runMediaStage(form, runId, outputDir, approvedSummary);
        } else {
          await runUpload(token, runId, outputDir, approvedSummary);
        }
      } finally {
        approving.reviewAction = false;
      }
    });
  }

  /**
   * Resume a run the server reports waiting at a review (`staging_review`
   * / `media_review`) or mid Media (`media`).
   *
   * `approve` can't do this itself: it depends on what the store holds
   * (`stagingSummary`, the form, `runDir`, `importRunId`) that a
   * reload has none of, and it branches on the phase rather than the run's
   * own stored stage. This rebuilds that state from `importRun` instead, then
   * routes exactly the way the normal flow would have got here.
   *
   * `resumedForm` is the caller's already-validated `restoreFormFromSnapshot`
   * result: the caller needs that check anyway (to fall back to
   * `settings_unreadable`), so this trusts it rather than parsing
   * `importRun.form` a second time.
   *
   * The directory is the truth. Every landing recomputes the summary fresh
   * from the run directory; the run's stored `summary` is read only as the
   * approved plan, for the Staging row on the Media Review and the Media
   * stage's own bookkeeping.
   *
   * A recompute failing here is a transient read of the run directory, not
   * a run that failed: only an explicit cancel ends a waiting run, so this
   * must not complete it or write a stage. It returns to the form instead
   * (the resume check there re-runs and finds the same run, so the panel
   * reappears; that is the retry) and leaves the failure on `resumeError`.
   */
  async function resumeAtReview(
    importRun: ActiveImportRun,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (!isTauri()) return;
    await takeRunFor(accountId);
    await asWork(() => resumeRunAtReview(importRun, resumedForm));
  }

  /** `resumeAtReview`, once the store is this account's. */
  async function resumeRunAtReview(
    importRun: ActiveImportRun,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (
      importRun.stage !== "staging_review" &&
      importRun.stage !== "media_review" &&
      importRun.stage !== "media"
    ) {
      return;
    }
    if (!importRun.run_dir) return; // resumeDecisionFor guarantees this; defensive only.

    const runId = importRun.id;
    const outputDir = importRun.run_dir;
    const approved = parseStoredStagingSummary(importRun.summary);
    // Staging has finished for every stage resumed here, so the mode comes
    // from the directory: through the plan approved at the Staging Review until
    // the summary below is recomputed from the directory itself.
    const known = withRecordedMode(resumedForm, approved);

    beginRun(known, importRun.stage === "media" ? "media" : "staging");
    await loadCarriedRecord(outputDir);
    store.set({
      ...CLEARED_RUN,
      resumeError: null,
      form: known,
      runDir: outputDir,
      importRunId: runId,
      sourceIdentities: parseSourceIdentities(importRun.source_identities),
    });

    /** Recompute the summary from the directory, then land on the given review. */
    async function landOn(
      review: "staging_review" | "media_review",
      partiallyRan: boolean,
    ): Promise<void> {
      const mediaDone = review === "media_review";
      store.set({
        steps: resumeSteps(known.attachmentMedia, mediaDone),
        computingSummary: true,
        phase: "running",
        running: true,
      });
      try {
        const actual = await summarizeStagingWithProgress({ run_dir: outputDir });
        const recorded = adoptRecordedMode(known, actual);
        store.set({ steps: resumeSteps(recorded.attachmentMedia, mediaDone) });
        if (review === "staging_review") {
          const missing = await mediaToolsMissingFor(recorded.attachmentMedia);
          store.set({
            stagingSummary: actual,
            mediaToolsMissing: missing,
            mediaPartiallyRan: partiallyRan,
          });
        } else {
          // The Staging row shows the plan approved before Media, read
          // back from the run; the Media rows show the directory as it is now.
          // Media's own report is gone on a resume, so its failed count is
          // unknown rather than zero.
          store.set({
            stagingSummary: approved ?? null,
            mediaSummary: actual,
            mediaFailedCount: null,
          });
        }
        waitAtReview(review);
      } catch (e: unknown) {
        returnToFormWithError(e);
      }
    }

    if (importRun.stage === "staging_review") {
      await landOn("staging_review", false);
      return;
    }
    if (importRun.stage === "media_review") {
      await landOn("media_review", false);
      return;
    }

    // transcode: Media died mid-run. Re-running it is safe (the stage is
    // resumable), so long as the tools it needs are there: a resume with
    // ffmpeg missing falls back to the Staging Review's recomputed
    // summary instead of starting a job that can only fail, using the same
    // `mediaToolsMissing` check the normal flow shows there.
    if ((await mediaToolsMissingFor(known.attachmentMedia)).length > 0) {
      await landOn("staging_review", true);
      return;
    }
    store.set({ steps: resumeSteps(known.attachmentMedia, false) });
    await runMediaStage(known, runId, outputDir, approved);
  }

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
    discardRun,
    dismissRunDirDeleteFailure,
    startImport,
    continueAfterIdentityStop,
    cancelIdentityStop,
    approve,
    // Another account's run is that account's to cancel, stop or leave.
    cancelRun: async () => {
      if (ownsRun()) await asWork(cancelRun);
    },
    resumeAtReview,
    cancel: async () => {
      if (ownsRun()) await cancel();
    },
    returnToForm: () => {
      if (ownsRun()) returnToForm();
    },
  };
}
