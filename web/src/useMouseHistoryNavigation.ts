import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { isTauri } from "./lib/tauriCheck";

/** The thumb buttons, as the DOM numbers them. */
const BACK_BUTTON = 3;
const FORWARD_BUTTON = 4;

/**
 * Move through history with the back and forward buttons on the side of a mouse.
 *
 * A browser already does this on its own, and handling the buttons here as well
 * would move two entries per click, so the hook only binds inside the desktop
 * app — which has no browser chrome and otherwise ignores them entirely.
 */
export function useMouseHistoryNavigation(): void {
  const navigate = useNavigate();

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    function onMouseDown(event: MouseEvent) {
      if (event.button !== BACK_BUTTON && event.button !== FORWARD_BUTTON) {
        return;
      }
      // Cancelling keeps a webview that maps these buttons itself from stepping
      // through history a second time on top of the navigate below.
      event.preventDefault();
      navigate(event.button === BACK_BUTTON ? -1 : 1);
    }

    // The click that follows the press is cancelled as well, so nothing further
    // down the page mistakes it for a plain auxiliary click.
    function onAuxClick(event: MouseEvent) {
      if (event.button === BACK_BUTTON || event.button === FORWARD_BUTTON) {
        event.preventDefault();
      }
    }

    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("auxclick", onAuxClick, true);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("auxclick", onAuxClick, true);
    };
  }, [navigate]);
}
