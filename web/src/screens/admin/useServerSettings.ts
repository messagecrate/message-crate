import { useRouteQuery } from "../../hooks/useRouteQuery";
import { keys } from "../../lib/queryKeys";
import { getServerSettings } from "../../lib/serverApi";

/**
 * The server settings the administrator reads, `GET /v1/server/settings`: what the
 * administrator controls, and how the database is stored (its journal mode and the
 * error SQLite gave when the server asked for write-ahead logging). Server
 * Settings and the notice above every Admin Home section share the one entry.
 */
export function useServerSettings() {
  return useRouteQuery(keys.serverSettings.all, (signal) => getServerSettings({ signal }));
}
