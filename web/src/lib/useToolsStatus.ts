import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { keys } from "./queryKeys";
import { invokeRetryToolDownloads, invokeToolsStatus, toolsDownloading } from "./tauri";
import { isTauri } from "./tauri-check";

/** How often the window asks the desktop process again while a check or a download runs. */
const DOWNLOAD_POLL_MS = 1000;

/**
 * How often the window asks again after Try again found another app's check
 * running, which `checking` does not count, and for how long: a download
 * that arrives shows as found, and a failed one is never heard of here.
 */
const OTHER_CHECK_POLL_MS = 2000;
export const OTHER_CHECK_POLL_FOR_MS = 2 * 60 * 1000;

/**
 * Where ffmpeg, ffprobe and wtsexporter are and how their downloads stand,
 * asked again each second while a check or a download runs: a program the
 * check has not looked at yet shows as missing until it does. Until
 * `otherCheckUntil` (a time in milliseconds) it is also asked every 2 s, for
 * another app's check that `checking` does not count. Settings and the Import
 * screen read the same entry. It belongs to this computer, not to an account,
 * so this is a plain `useQuery`; the browser has no desktop process to ask.
 */
export function useToolsStatus({ otherCheckUntil = 0 }: { otherCheckUntil?: number } = {}) {
  return useQuery({
    queryKey: keys.desktopToolsStatus.all,
    queryFn: invokeToolsStatus,
    enabled: isTauri(),
    retry: false,
    refetchInterval: (query) => {
      const data = query.state.data;
      if (data && (data.checking || toolsDownloading(data))) return DOWNLOAD_POLL_MS;
      return Date.now() < otherCheckUntil ? OTHER_CHECK_POLL_MS : false;
    },
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
