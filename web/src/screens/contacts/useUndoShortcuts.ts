import { useEffect } from "react";

/** macOS takes Cmd where every other system takes Ctrl. */
function isMac(): boolean {
  return /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
}

/** True when the key goes to a field that has its own undo, such as the search box. */
function typingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/**
 * Ctrl+Z undoes; Ctrl+Shift+Z and Ctrl+Y redo; Cmd in place of Ctrl on
 * macOS (#3234). The keys work while the screen that calls this is mounted,
 * and not while focus is in a text field, so the search box keeps the
 * browser's own undo.
 */
export function useUndoShortcuts(onUndo: () => void, onRedo: () => void): void {
  useEffect(() => {
    const mac = isMac();
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (!mod || event.altKey || event.defaultPrevented) return;
      if (typingTarget(event.target)) return;
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
