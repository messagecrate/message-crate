import { useCallback, useEffect, useState } from "react";
import { errorText } from "../lib/apiErrorMessage";
import { type LocalServerNotice, onLocalServerNotice } from "../lib/desktop/localServer";
import { startLocalServer } from "../lib/localServer";
import { isTauri } from "../lib/tauriCheck";

/** A notice the bar shows: one the desktop app sent, other than `answering`. */
export type ShownLocalServerNotice = Exclude<LocalServerNotice, { state: "answering" }>;

/**
 * What the desktop app last said about its own Message Crate while it starts
 * it again after it stopped (#2695), for the notice bar. Null in the browser,
 * before anything was said, and once a Message Crate answers again.
 *
 * The app tells the window; nothing here asks it. `retry` asks the app to
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
    let unlisten: (() => void) | undefined;
    onLocalServerNotice((next) => {
      if (!stopped) setNotice(next.state === "answering" ? null : next);
    }).then(
      (stop) => {
        if (stopped) stop();
        else unlisten = stop;
      },
      () => {
        // No window to listen on: there is nothing to show either.
      },
    );
    return () => {
      stopped = true;
      unlisten?.();
    };
  }, []);

  const retry = useCallback(() => {
    setNotice({ state: "restarting" });
    startLocalServer().then(
      (status) => {
        if (status.status === "ready") setNotice(null);
        else if (status.status === "failed")
          setNotice({ state: "failed", message: status.message, details: status.details });
      },
      (error: unknown) => {
        setNotice({
          state: "failed",
          message: "Message Crate could not be started.",
          details: errorText(error),
        });
      },
    );
  }, []);

  return { notice, retry };
}
