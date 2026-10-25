import { useEffect } from "react";

/** macOS takes Cmd where every other system takes Ctrl. */
function isMac(): boolean {
  return /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
}

/**
 * True when the key goes to a field that has its own undo, such as the search
 * box, or to an open dialog or menu, such as the "Move 3 contacts to Trash?"
 * confirmation, which must not see the list change under it.
 */
function keepKeysFrom(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return true;
  // A modal (ModalShell marks its overlay) or a popover menu, but not the
  // docked contact pane, which is a dialog beside the list rather than over it.
  return target.closest("[data-mc-modal], [data-mc-overlay], [role='menu']") !== null;
}

/**
 * Ctrl+Z undoes; Ctrl+Shift+Z and Ctrl+Y redo; Cmd in place of Ctrl on
 * macOS (#3234). The keys work while the screen that calls this is mounted,
 * and not while focus is in a text field, so the search box keeps the
 * browser's own undo, nor in an open dialog or menu.
 */
export function useUndoShortcuts(onUndo: () => void, onRedo: () => void): void {
  useEffect(() => {
    const mac = isMac();
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (!mod || event.altKey || event.defaultPrevented) return;
      if (keepKeysFrom(event.target)) return;
      const key = event.key.toLowerCase();
      if (key === "z" && !event.shiftKey) {
        event.preventDefault();
        onUndo();
      } else if ((key === "z" && event.shiftKey) || (key === "y" && !event.shiftKey)) {
        event.preventDefault();
        onRedo();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onUndo, onRedo]);
}
