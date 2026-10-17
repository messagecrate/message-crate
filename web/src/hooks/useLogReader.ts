import type { LogReader } from "../lib/desktop/runLog";
import { canReadDesktopLogs } from "../lib/desktopFeatures";
import { isTauri } from "../lib/tauriCheck";
import { useAuth } from "./useAuth";
import { useIsOwner } from "./useIsOwner";
import { useServerInfo } from "./useServerInfo";

/**
 * Who reads the logs in this computer's Logs Directory: the signed-in
 * account on the Message Crate it is signed in to, and whether it is the
 * owner, who reads every one, the desktop app's own included. Null in a
 * browser, where none is, and before the profile says whether the account
 * is the owner and the server says its id.
 */
export function useLogReader(): LogReader | null {
  const { accountId } = useAuth();
  const { isOwner, loading } = useIsOwner();
  const server = useServerInfo();
  const messageCrateId = server.data?.id;
  if (!canReadDesktopLogs(isTauri()) || accountId === null || loading || !messageCrateId) {
    return null;
  }
  return { messageCrateId, accountId, owner: isOwner };
}
