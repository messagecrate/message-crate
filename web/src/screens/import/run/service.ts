import type { AccountProfile } from "../../../lib/account";
import { profileAddresses } from "../../../lib/account";
import { needsIdentityStop, parseSourceIdentities } from "../../../lib/backupIdentity";
import type { ActiveImportRun } from "../../../lib/importRun";
import { importSourceFor } from "../../../lib/importSources";
import type { ImportJobFormValues } from "../../../lib/importSources/types";
import { createRunCancel } from "../../../lib/runCancel";
import { CLEARED_RUN, initialImportRunState } from "../importRunStore";
import { mediaJobVerb } from "../reviewForecast";
import type { ImportRunDeps, RunContext } from "./context";
import { summarizeStagingWithProgress } from "./desktopJob";
import { discardRun } from "./finish";
import { mediaToolsMissingFor, runMediaStage } from "./media";
import { dismissRunDirDeleteFailure } from "./runDirectory";
import { beginRun, freshScratch, loadCarriedRecord } from "./scratch";
import {
  initialSteps,
  movePhase,
  resumeSteps,
  returnToForm,
  returnToFormWithError,
  waitAtReview,
} from "./screen";
import { accountLeft, moveStageAtReview } from "./serverCalls";
import { type ResumeUpload, type ResumeWrite, runImport } from "./staging";
import { adoptRecordedMode, parseStoredStagingSummary, withRecordedMode } from "./stagingSummary";
import { runUpload } from "./upload";

/** The account an action on the run is taken for: the one logged in when it was asked. */
export type RunAccount = {
  token: string | null;
  accountId: number | null;
  /** Read the account's profile, for the identity check before a new iMessage run. */
  fetchProfile: () => Promise<AccountProfile | null>;
};

/**
 * One Import Run: its stages, its reviews, and the state it keeps between
 * them, built over `deps`. The run's state lives in this closure, not in its
 * modules, so two services never share a run; the app builds one
 * (`importRunService` in `useImportJob.ts`), so its run outlives the Import
 * screen.
 *
 * Every action reads the run from the store at the moment it is called, never
 * from a render's closure, so a screen that unmounted while a stage ran and
 * mounted again finds the run where it left it.
 */
export function createImportRunService(deps: ImportRunDeps) {
  const ctx: RunContext = {
    ...deps,
    scratch: freshScratch(deps.desktop.invokeCancel),
    recordWrites: Promise.resolve(),
    queuedRecordWrite: null,
    work: null,
  };
  const { store } = ctx;

  /** Put the store and the scratch back to a fresh form, started by no account. */
  function reset(): void {
    ctx.scratch = freshScratch(ctx.desktop.invokeCancel);
    store.reset(initialImportRunState(initialSteps()));
  }
  reset();

  /** Run `action` as the Import screen's work of the moment (`ctx.work`). */
  function asWork(action: () => Promise<void>): Promise<void> {
    const tracked = action().finally(() => {
      if (ctx.work === tracked) ctx.work = null;
    });
    ctx.work = tracked;
    return tracked;
  }

  /** True when the run in the store is `account`'s to act on. */
  function ownsRun(account: RunAccount): boolean {
    return store.get().accountId === account.accountId;
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
      const running = ctx.work;
      if (running == null) {
        reset();
        store.set({ accountId });
        return;
      }
      await ctx.scratch.runCancel.cancel();
      await running.catch(() => {});
    }
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
    account: RunAccount,
    form: ImportJobFormValues,
    resume?: ResumeUpload,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
    if (!ctx.desktop.isTauri()) return;
    await takeRunFor(account.accountId);
    // A second call while one is already probing or running is a no-op.
    if (ctx.scratch.startImport) return;
    ctx.scratch.startImport = true;
    const started = ctx.scratch;
    await asWork(async () => {
      try {
        await startRun(account, form, resume, resumeWrite);
      } finally {
        started.startImport = false;
      }
    });
  }

  /** `startImport`, once the store is this account's and no other start is under way. */
  async function startRun(
    account: RunAccount,
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
        identities = await ctx.desktop
          .invokeImessageBackupIdentities({
            path: form.backupPath,
            ios: identityRead.ios,
            backupPassword: form.backupPassword,
          })
          .catch(() => []);
        store.set({ sourceIdentities: identities });
        const profile = await account.fetchProfile();
        // The account logged out during the probe: the profile just read
        // is another account's, and no run exists yet to resume.
        if (accountLeft(ctx)) return;
        if (needsIdentityStop(identities, profile && profileAddresses(profile))) {
          ctx.scratch.pendingIdentityForm = form;
          movePhase(ctx, "identity_stop");
          return;
        }
      } finally {
        store.set({ running: false });
      }
    } else {
      store.set({ sourceIdentities: resumeWrite ? (resumeWrite.identities ?? null) : null });
    }
    await runImport(ctx, account.token, form, identities, resume, resumeWrite);
  }

  /** Continue past the identity stop with the parked form. */
  async function continueAfterIdentityStop(account: RunAccount): Promise<void> {
    if (!ownsRun(account)) return;
    const form = ctx.scratch.pendingIdentityForm;
    if (!form) return;
    ctx.scratch.pendingIdentityForm = null;
    await asWork(() => runImport(ctx, account.token, form, store.get().sourceIdentities));
  }

  /** Leave the identity stop; nothing was created, so only the phase moves. */
  function cancelIdentityStop(account: RunAccount): void {
    if (!ownsRun(account)) return;
    ctx.scratch.pendingIdentityForm = null;
    returnToForm(ctx);
  }

  /** Approve the waiting review: Media after the Staging Review when there is one, Upload otherwise. */
  async function approve(account: RunAccount): Promise<void> {
    if (!ctx.desktop.isTauri()) return;
    if (!ownsRun(account)) return;
    if (ctx.scratch.reviewAction) return;
    const form = ctx.scratch.form;
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

    ctx.scratch.reviewAction = true;
    ctx.scratch.runCancel = createRunCancel(ctx.desktop.invokeCancel);
    const approving = ctx.scratch;
    await asWork(async () => {
      try {
        // The review's own stage did not reach the server, so it is written
        // first: a later visit must find the run at this review.
        if (reviewError != null) {
          const recorded =
            phase === "media_review"
              ? await moveStageAtReview(ctx, runId, "media_review", stagingSummary ?? undefined)
              : await moveStageAtReview(ctx, runId, "staging_review");
          if (recorded !== "recorded") return;
        }
        if (phase === "staging_review" && mediaJobVerb(form.attachmentMedia) !== null) {
          await runMediaStage(ctx, form, runId, outputDir, approvedSummary);
        } else {
          await runUpload(ctx, account.token, runId, outputDir, approvedSummary);
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
    account: RunAccount,
    importRun: ActiveImportRun,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (!ctx.desktop.isTauri()) return;
    await takeRunFor(account.accountId);
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

    beginRun(ctx, known, importRun.stage === "media" ? "media" : "staging");
    await loadCarriedRecord(ctx, outputDir);
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
      movePhase(ctx, "resume", {
        steps: resumeSteps(known.attachmentMedia, mediaDone),
        computingSummary: true,
        running: true,
      });
      try {
        const actual = await summarizeStagingWithProgress(ctx, { run_dir: outputDir });
        const recorded = adoptRecordedMode(ctx, known, actual);
        store.set({ steps: resumeSteps(recorded.attachmentMedia, mediaDone) });
        if (review === "staging_review") {
          const missing = await mediaToolsMissingFor(ctx, recorded.attachmentMedia);
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
        waitAtReview(ctx, review);
      } catch (e: unknown) {
        returnToFormWithError(ctx, e);
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
    if ((await mediaToolsMissingFor(ctx, known.attachmentMedia)).length > 0) {
      await landOn("staging_review", true);
      return;
    }
    store.set({ steps: resumeSteps(known.attachmentMedia, false) });
    await runMediaStage(ctx, known, runId, outputDir, approved);
  }

  /** Cancel the run at a Review: end it (`discardRun`) and go back to the form. */
  async function cancelRunAtReview(): Promise<void> {
    if (ctx.scratch.reviewAction) return;
    ctx.scratch.reviewAction = true;
    try {
      const { importRunId: runId, runDir: outputDir } = store.get();
      await discardRun(ctx, runId, outputDir);
    } finally {
      ctx.scratch.reviewAction = false;
    }
    returnToForm(ctx);
  }

  return {
    /** Where the screen reads the run. */
    store,
    reset,
    startImport,
    continueAfterIdentityStop,
    cancelIdentityStop,
    approve,
    resumeAtReview,
    // Another account's run is that account's to cancel, stop or leave.
    /** Cancel the run at a Review. */
    cancelRun: async (account: RunAccount): Promise<void> => {
      if (ownsRun(account)) await asWork(cancelRunAtReview);
    },
    /** Stop the stage that is running. The run stays where it got to. */
    cancel: async (account: RunAccount): Promise<void> => {
      if (ownsRun(account)) await ctx.scratch.runCancel.cancel();
    },
    returnToForm: (account: RunAccount): void => {
      if (ownsRun(account)) returnToForm(ctx);
    },
    discardRun: (runId: number | null, runDir: string | null): Promise<void> =>
      discardRun(ctx, runId, runDir),
    dismissRunDirDeleteFailure: (): void => dismissRunDirDeleteFailure(ctx),
  };
}

export type ImportRunService = ReturnType<typeof createImportRunService>;
