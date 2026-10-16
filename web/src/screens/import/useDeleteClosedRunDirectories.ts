import { useEffect } from "react";
import { useAuth } from "../../hooks/useAuth";
import { useRouteCache } from "../../hooks/useRouteQuery";
import { useServerInfo } from "../../hooks/useServerInfo";
import { invokeDeleteClosedRunDir, invokeListAccountRunDirs } from "../../lib/desktop/runDir";
import { getImport } from "../../lib/serverApi";
import { isTauri } from "../../lib/tauriCheck";
import { deleteClosedRunDirectories } from "./closedRunDirectories";

/**
 * When Import opens in the desktop app, delete the directories of the
 * signed-in account's closed Import Runs on the Message Crate it is signed in
 * to. It runs again for another account or another Message Crate.
 */
export function useDeleteClosedRunDirectories(): void {
  const { accountId } = useAuth();
  const messageCrateId = useServerInfo().data?.id;
  const cache = useRouteCache();
  useEffect(() => {
    if (!isTauri() || accountId === null || !messageCrateId) return;
    let going = true;
    const signedIn = { accountId, messageCrateId };
    void deleteClosedRunDirectories(
      {
        list: () => invokeListAccountRunDirs(accountId, messageCrateId),
        // The run's entry is the one its Import Run view reads, so a refused
        // session ends here as it does on any screen.
        status: async (id) =>
          (await cache.fetch(["imports", id], (signal) => getImport(id, { signal }))).status,
        deleteClosed: (run, status) => invokeDeleteClosedRunDir(run, signedIn, status),
      },
      () => going,
    );
    return () => {
      going = false;
    };
  }, [accountId, messageCrateId, cache]);
}
