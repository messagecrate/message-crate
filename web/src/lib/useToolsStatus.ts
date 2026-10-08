import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { keys } from "./queryKeys";
import { invokeRetryToolDownloads, invokeToolsStatus, toolsDownloading } from "./tauri";
import { isTauri } from "./tauri-check";

/** How often the window asks the desktop process again while a check or a download runs. */
const DOWNLOAD_POLL_MS = 1000;

/**
 * Where ffmpeg, ffprobe and wtsexporter are and how their downloads stand,
 * asked again each second while a check or a download runs: a program the
 * check has not looked at yet shows as missing until it does. Settings and the Import
 * screen read the same entry. It belongs to this computer, not to an account,
 * so this is a plain `useQuery`; the browser has no desktop process to ask.
 */
export function useToolsStatus() {
  return useQuery({
    queryKey: keys.desktopToolsStatus.all,
    queryFn: invokeToolsStatus,
    enabled: isTauri(),
    retry: false,
    refetchInterval: (query) =>
      query.state.data && (query.state.data.checking || toolsDownloading(query.state.data))
        ? DOWNLOAD_POLL_MS
        : false,
  });
}

/**
 * Try again: the desktop process checks the Tools Directory and downloads
 * what is missing, and the status is asked for at once. The programs it
 * downloads show as downloading by then, so the status keeps being asked for
 * until they arrive or fail.
 */
export function useRetryToolDownloads() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: invokeRetryToolDownloads,
    onSettled: () => client.invalidateQueries({ queryKey: keys.desktopToolsStatus.all }),
  });
}
