import { invokeListLogs, type LogEntry, type LogReader } from "../lib/desktop/runLog";
import { useRouteQuery } from "./useRouteQuery";

/**
 * The logs in this computer's Logs Directory that `reader` may read, newest
 * first: its Import Run logs, and for the administrator every one and the desktop
 * app's own.
 */
export function useDesktopLogs(reader: LogReader | null): {
  logs: LogEntry[];
  loading: boolean;
  error: Error | null;
} {
  const listing = useRouteQuery(
    ["logs", "desktop", "list", reader?.messageCrateId, reader?.admin],
    () => (reader ? invokeListLogs(reader) : Promise.resolve([])),
    { enabled: reader !== null },
  );
  return {
    logs: listing.data ?? [],
    loading: reader !== null && listing.isPending,
    error: listing.error,
  };
}
