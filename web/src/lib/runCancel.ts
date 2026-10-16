import { invokeCancel } from "./desktop/job";
import { CanceledError } from "./desktopJobError";

/**
 * The Cancel of one run of desktop jobs: an Import Run, or an Export's export step
 * and conversion.
 *
 * The desktop side stops only the job that is running, and each job command
 * starts its job with a cancel flag of its own
 * (`src-tauri/src/commands/jobs.rs`). A Cancel pressed while the run is
 * between jobs, or while a job command is starting, finds no job to stop and
 * would be lost. So the run remembers its Cancel here: `guard` refuses to
 * start a job once the run is canceled, and sends the Cancel again after a
 * job command returns, when the run was canceled while that command was
 * starting.
 */
export type RunCancel = {
  /** Record the Cancel for this run and stop the job that is running. */
  cancel: () => Promise<void>;
  /**
   * Wrap the invoke of a job command. The wrapped call throws a
   * `CanceledError` without invoking when the run is canceled, as a job
   * the desktop app canceled rejects.
   */
  guard: (invokeFn: () => Promise<void>) => () => Promise<void>;
};

/** A run's Cancel, stopping the running job with `stopJob` (the desktop side's Cancel). */
export function createRunCancel(stopJob: () => Promise<void> = invokeCancel): RunCancel {
  let cancelled = false;
  return {
    cancel: async () => {
      cancelled = true;
      await stopJob();
    },
    guard: (invokeFn) => async () => {
      if (cancelled) throw new CanceledError();
      await invokeFn();
      if (cancelled) await stopJob();
    },
  };
}
