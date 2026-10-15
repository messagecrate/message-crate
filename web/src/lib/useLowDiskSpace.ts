import { useQuery } from "@tanstack/react-query";
import { keys } from "./queryKeys";
import { invokeLowDiskSpace } from "./tauri";
import { isTauri } from "./tauriCheck";

/**
 * What the Import form says about the disks that hold the Staging Directory
 * and the Scratch Directory: one sentence for each with less free than the
 * desktop process's threshold (#2010). It is read when the form is shown,
 * again for each `source` picked, and again after Settings changes the
 * Staging Directory, which invalidates it. A reading that fails says
 * nothing: the warning never stops an import, so a disk that cannot be read
 * is left to the run's own check.
 * It belongs to this computer, not to an account, so this is a plain
 * `useQuery`; the browser has no desktop process to ask.
 */
export function useLowDiskSpace(source: string): string[] {
  const query = useQuery({
    queryKey: keys.desktopLowDiskSpace.of(source),
    queryFn: invokeLowDiskSpace,
    enabled: isTauri(),
    retry: false,
    staleTime: 0,
  });
  return query.data ?? [];
}
