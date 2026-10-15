import type { UnlistenFn } from "@tauri-apps/api/event";
import { type CloseRequestedEvent, getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Call `handler` when the person asks to close the desktop app's window. The
 * handler keeps the window open by calling the event's `preventDefault()`.
 * Resolves to a function that stops listening.
 */
export async function onWindowCloseRequested(
  handler: (event: CloseRequestedEvent) => void | Promise<void>,
): Promise<UnlistenFn> {
  return getCurrentWindow().onCloseRequested(handler);
}

/** Close the desktop app's window without asking `onWindowCloseRequested`'s handler. */
export async function destroyWindow(): Promise<void> {
  await getCurrentWindow().destroy();
}
