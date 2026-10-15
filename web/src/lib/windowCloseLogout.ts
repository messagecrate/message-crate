/**
 * Desktop only: closing the window logs out, then closes it.
 */
import { useEffect, useRef } from "react";
import { destroyWindow, onWindowCloseRequested } from "./desktop/window";
import { isTauri } from "./tauriCheck";

/**
 * Hold the desktop window open on close until `revokeSession` has ended the
 * session, then close it. Does nothing outside the desktop app.
 *
 * There is no pause first: closing the window ends a running Upload with the
 * app, and the run resumes from what the Upload's journal recorded as sent.
 */
export function useLogoutOnWindowClose(revokeSession: () => Promise<void>): void {
  const closingRef = useRef(false);
  useEffect(() => {
    if (!isTauri()) return;

    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      try {
        unlisten = await onWindowCloseRequested(async (event) => {
          event.preventDefault();
          if (closingRef.current) return;
          closingRef.current = true;
          try {
            await revokeSession();
            await destroyWindow();
          } catch {
            // Destroy failed or window already gone — allow another close attempt.
            closingRef.current = false;
          }
        });
        if (cancelled) {
          unlisten();
          unlisten = undefined;
        }
      } catch {
        // Missing window permissions or not a real Tauri window — leave close alone.
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [revokeSession]);
}
