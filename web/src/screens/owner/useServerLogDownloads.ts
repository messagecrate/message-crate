import type { LogDownload } from "../../components/logs/logSource";
import { useRouteQuery } from "../../hooks/useRouteQuery";
import { getServerLogFile, listServerLogFiles } from "../../lib/serverApi";

/**
 * The files of the server's log, each a download, newest first. The owner's
 * alone, like the lines.
 */
export function useServerLogDownloads(): { downloads: LogDownload[]; error: Error | null } {
  const files = useRouteQuery(["logs", "server", "files"], (signal) =>
    listServerLogFiles({ signal }),
  );
  return {
    downloads: (files.data ?? []).map((file) => ({
      name: file.name,
      read: () => getServerLogFile(file.id),
    })),
    error: files.error,
  };
}
