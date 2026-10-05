import { useCallback, useState } from "react";
import type { DesktopJobName } from "../lib/desktopJob";
import { awaitTauriJob, invokeCancel, type TauriJobResult } from "../lib/tauri";
import type { ImportIssueEvent, ImportProgressEvent } from "../lib/types";

export type TauriJobRunCallbacks = {
  onLog?: (line: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
};

/**
 * Shared run/cancel state for long-running desktop jobs, used by Export and
 * Settings → Convert. `run` waits for the job's result and holds the desktop
 * job under `job` while it runs.
 *
 * `Request` is what the screen asked the job to do, such as a directory and a
 * format. Each job is started with one, and `finished` hands back the request
 * of the job that finished, so a success message names what the job wrote
 * rather than what the form holds now.
 */
export function useTauriJob<Request>(options: { job: DesktopJobName }): {
  running: boolean;
  /**
   * The request the last job was started with, once that job reports that it
   * finished successfully. `null` while a job runs, after a failure, and
   * before the first job.
   */
  finished: Request | null;
  /**
   * Wait for one desktop job to finish. Throws if the job reports an error;
   * the caller handles that.
   */
  run: (
    invokeFn: () => Promise<void>,
    request: Request,
    callbacks?: TauriJobRunCallbacks,
  ) => Promise<TauriJobResult>;
  cancel: () => Promise<void>;
} {
  const job = options.job;
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState<Request | null>(null);

  const run = useCallback(
    async (
      invokeFn: () => Promise<void>,
      request: Request,
      callbacks?: TauriJobRunCallbacks,
    ): Promise<TauriJobResult> => {
      setRunning(true);
      setFinished(null);
      try {
        const result = await awaitTauriJob(
          job,
          invokeFn,
          callbacks?.onLog,
          callbacks?.onProgress,
          callbacks?.onIssue,
        );
        // The updater form, so a request that is itself a function is stored as is.
        setFinished(() => request);
        return result;
      } catch (err: unknown) {
        setFinished(null);
        throw err;
      } finally {
        setRunning(false);
      }
    },
    [job],
  );

  const cancel = useCallback(async () => {
    await invokeCancel();
  }, []);

  return { running, finished, run, cancel };
}
