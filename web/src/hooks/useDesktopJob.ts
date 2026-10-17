import { type DesktopJobName, desktopJobHolds, firstHeld } from "../lib/desktopJob";
import { useExternalStore } from "./useExternalStore";

/** The desktop job that is running, or null, kept up to date. */
export function useDesktopJob(): DesktopJobName | null {
  return useExternalStore(desktopJobHolds, firstHeld);
}
