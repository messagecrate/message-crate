import { useEffect } from "react";
import { useContactHistory } from "../hooks/useContactHistory";
import { focusRingClass, popupShadowClass } from "../lib/uiStyles";
import { Z_POPOVER } from "../lib/zLayers";
import Button from "./Button";
import { XIcon } from "./icons";
import PlainButton from "./PlainButton";

/** How long the toast stays after an action, an undo or a redo. */
export const UNDO_TOAST_MS = 20_000;

/**
 * The toast at the bottom center after an action on the Contacts list, an
 * undo or a redo (#3234). It goes after `UNDO_TOAST_MS`, on ×, or when the
 * next one replaces it. Escape does not dismiss it, because Escape already
 * closes the menu or dialog the person may be in.
 */
export default function ContactHistoryToast() {
  const { toast, busy, undo, dismissToast } = useContactHistory();
  const toastId = toast?.id;

  useEffect(() => {
    if (toastId === undefined) return;
    const timer = setTimeout(dismissToast, UNDO_TOAST_MS);
    return () => clearTimeout(timer);
  }, [toastId, dismissToast]);

  if (!toast) return null;
  return (
    <div
      className={`pointer-events-none fixed bottom-4 left-1/2 w-max max-w-[min(calc(100vw-2rem),28rem)] -translate-x-1/2 ${Z_POPOVER}`}
    >
      <div
        role="status"
        className={`pointer-events-auto flex items-center gap-2 rounded-lg border border-border bg-popover py-1.5 pr-1.5 pl-3.5 text-[0.813rem] text-text ${popupShadowClass}`}
      >
        <span className="min-w-0 flex-1 truncate">{toast.text}</span>
        {toast.undoable ? (
          <Button size="chip" isDisabled={busy} onPress={() => void undo()}>
            Undo
          </Button>
        ) : null}
        <PlainButton
          aria-label="Dismiss"
          onPress={dismissToast}
          className={`flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted hover:bg-hover hover:text-text ${focusRingClass}`}
        >
          <XIcon size={14} />
        </PlainButton>
      </div>
    </div>
  );
}
