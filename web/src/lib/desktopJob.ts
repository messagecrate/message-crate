import { useSyncExternalStore } from "react";

/** The screens that run desktop jobs, by the name of what they run. */
export type DesktopJobName = "Import Run" | "Export" | "Convert";

/**
 * Which desktop job is running in this window, if any.
 *
 * The desktop runs one job at a time and refuses a second one
 * (`src-tauri/src/commands/jobs.rs`). The screens hold this while their job
 * runs, so Import, Export and Settings → Convert can keep their Start
 * buttons off and say which job is running, rather than start a job the
 * desktop refuses.
 *
 * Holds nest: an Import Run holds it from its first stage to its end and an
 * Export from its pull to the end of its format step, and `awaitTauriJob`
 * holds it again for each desktop job call inside them. A call ending
 * releases only its own hold, so the run's hold covers the gaps between its
 * jobs, when the desktop itself has nothing running to refuse a Convert with.
 */
let holds: { name: DesktopJobName }[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/**
 * Mark `name` as the desktop job that is running. Returns the function that
 * releases this hold; it does nothing the second time.
 */
export function holdDesktopJob(name: DesktopJobName): () => void {
  const hold = { name };
  holds = [...holds, hold];
  notify();
  return () => {
    if (!holds.includes(hold)) return;
    holds = holds.filter((held) => held !== hold);
    notify();
  };
}

/** The desktop job that is running, or null. */
export function currentDesktopJob(): DesktopJobName | null {
  // The first hold still held: the job that took the desktop first.
  return holds[0]?.name ?? null;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The desktop job that is running, or null, kept up to date. */
export function useDesktopJob(): DesktopJobName | null {
  return useSyncExternalStore(subscribe, currentDesktopJob, currentDesktopJob);
}

const RUNNING: Record<DesktopJobName, string> = {
  "Import Run": "An Import Run is running.",
  Export: "An Export is running.",
  Convert: "A Convert is running.",
};

/** Why `start` can't start: the job that is running, and when it can. */
export function desktopJobRunningText(running: DesktopJobName, start: string): string {
  return `${RUNNING[running]} ${start} can start once it ends.`;
}
