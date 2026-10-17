import { createExternalStore, type ExternalStore } from "./externalStore";

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
 * Export from its export step to the end of its format step, and `awaitDesktopJob`
 * holds it again for each desktop job call inside them. A call ending
 * releases only its own hold, so the run's hold covers the gaps between its
 * jobs, when the desktop itself has nothing running to refuse a Convert with.
 */
const holds = createExternalStore<readonly { name: DesktopJobName }[]>([]);

/**
 * Mark `name` as the desktop job that is running. Returns the function that
 * releases this hold; it does nothing the second time.
 */
export function holdDesktopJob(name: DesktopJobName): () => void {
  const hold = { name };
  holds.set([...holds.get(), hold]);
  return () => {
    if (!holds.get().includes(hold)) return;
    holds.set(holds.get().filter((held) => held !== hold));
  };
}

/** The holds, for `useDesktopJob` to follow. Only `holdDesktopJob` changes them. */
export const desktopJobHolds: Pick<
  ExternalStore<readonly { name: DesktopJobName }[]>,
  "get" | "subscribe"
> = holds;

/** The first hold still held: the job that took the desktop first. */
export function firstHeld(held: readonly { name: DesktopJobName }[]): DesktopJobName | null {
  return held[0]?.name ?? null;
}

/** The desktop job that is running, or null. */
export function currentDesktopJob(): DesktopJobName | null {
  return firstHeld(holds.get());
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
