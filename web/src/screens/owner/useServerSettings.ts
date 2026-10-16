import { keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
import { getServerSettings } from "../../lib/serverApi";

/**
 * The server settings the owner reads, `GET /v1/server/settings`: what the
 * owner controls, and how the database is stored (its journal mode and the
 * error SQLite gave when the server asked for write-ahead logging). Server
 * Settings and the notice above every Owner Home section share the one entry.
 */
export function useServerSettings() {
  return useRouteQuery(keys.serverSettings.all, (signal) => getServerSettings({ signal }));
}
