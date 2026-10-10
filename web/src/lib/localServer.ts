import { DEFAULT_TAURI_SERVER_URL } from "./authGuards";
import { readPref, removePref, writePref } from "./storage";
import { invokeStartLocalServer, type LocalServerStatus } from "./tauri";

/**
 * The Message Crate the desktop app starts for itself. The app ships the
 * server and runs it at its own address while the app is open; the rules are
 * in `src-tauri/src/local_server.rs`. The commands that read and change it
 * are in `tauri.ts`.
 */

/**
 * Whether `url` is the app's own address, the one it starts a server for.
 * Any other address is a Message Crate the person chose, which the app never
 * starts.
 */
export function isOwnAddress(url: string): boolean {
  return url.trim().replace(/\/+$/, "") === DEFAULT_TAURI_SERVER_URL;
}

/**
 * Make sure the app's own Message Crate is running. Safe to call on every
 * launch: a Message Crate already answering is used as it is, and a start
 * under way is left alone. Calling it again after a failure tries again, and
 * a Message Crate the app found is asked again whether it still answers.
 */
export async function startLocalServer(): Promise<LocalServerStatus> {
  return invokeStartLocalServer(getOpenToNetwork());
}

const OPEN_TO_NETWORK_KEY = "mc-local-server-open-to-network";

/**
 * Whether the app's own Message Crate accepts connections from other devices
 * on the network. Off unless the person switched it on: the connection is
 * plain HTTP.
 */
export function getOpenToNetwork(): boolean {
  return readPref(OPEN_TO_NETWORK_KEY) === "1";
}

/**
 * Save the setting for the next launch. `invokeSetOpenToNetwork` in
 * `tauri.ts` gives it to a server the app runs now.
 */
export function setOpenToNetwork(on: boolean): void {
  if (on) writePref(OPEN_TO_NETWORK_KEY, "1");
  else removePref(OPEN_TO_NETWORK_KEY);
}
