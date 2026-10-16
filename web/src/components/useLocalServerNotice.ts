import { useCallback, useEffect, useState } from "react";
import {
  invokeLocalServerStatus,
  type LocalServerNotice,
  onLocalServerNotice,
} from "../lib/desktop/localServer";
import { startFailure, startLocalServer } from "../lib/localServer";
import { isTauri } from "../lib/tauriCheck";

/** A notice the bar shows: one the desktop app sent, other than `answering`. */
export type ShownLocalServerNotice = Exclude<LocalServerNotice, { state: "answering" }>;

/** The failure notice for a failed state the app reports. */
function failedNotice(status: { message: string; details: string }): ShownLocalServerNotice {
  return { state: "failed", message: status.message, details: status.details };
}

/**
 * What the desktop app last said about its own Message Crate while it starts
 * it again after it stopped (#2695), for the notice bar. Null in the browser,
 * before anything was said, and once a Message Crate answers again.
 *
 * The app tells the window; nothing here polls. A window opened or reloaded
 * after a restart failed missed that notice, so the hook reads the state once
 * when it mounts and shows a failure it finds there. `retry` asks the app to
 * start its server again after a failure, the way the login card's Try again
 * does, and shows the restart until the app says how it went.
 */
export function useLocalServerNotice(): {
  notice: ShownLocalServerNotice | null;
  retry: () => void;
} {
  const [notice, setNotice] = useState<ShownLocalServerNotice | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    let stopped = false;
    // Whether the app said something since the hook mounted. Its word is
    // newer than the state read at mount, which then changes nothing.
    let told = false;
    let unlisten: (() => void) | undefined;
    onLocalServerNotice((next) => {
      if (stopped) return;
      told = true;
      setNotice(next.state === "answering" ? null : next);
    }).then(
      (stop) => {
        if (stopped) stop();
        else unlisten = stop;
      },
      () => {
        // No window to listen on: there is nothing to show either.
      },
    );
    invokeLocalServerStatus().then(
      (status) => {
        if (!stopped && !told && status.status === "failed") setNotice(failedNotice(status));
      },
      () => {
        // The state could not be read; the next notice says it.
      },
    );
    return () => {
      stopped = true;
      unlisten?.();
    };
  }, []);

  const retry = useCallback(() => {
    // The app says how the start went; until it does, it is under way.
    setNotice({ state: "restarting", cause: "exited" });
    startLocalServer().then(
      (status) => {
        if (status.status === "ready") setNotice(null);
        else if (status.status === "failed") setNotice(failedNotice(status));
      },
      (error: unknown) => setNotice(failedNotice(startFailure(error))),
    );
  }, []);

  return { notice, retry };
}
