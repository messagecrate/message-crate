import { invoke } from "@tauri-apps/api/core";

/** Why the app's own Message Crate is not running. */
type LocalServerFailure = "port_taken" | "start_failed";

/**
 * What the desktop app reports about its own Message Crate, the server it
 * ships and runs at its own address while the app is open. The rules are in
 * `src-tauri/src/local_server.rs`.
 */
export type LocalServerStatus =
  | { status: "idle" }
  | { status: "starting"; first_time: boolean }
  | { status: "ready"; started_by_app: boolean }
  | { status: "failed"; reason: LocalServerFailure; message: string; details: string };

/** Start the app's own Message Crate, or ask the one it found whether it still answers. */
export async function invokeStartLocalServer(openToNetwork: boolean): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("start_local_server", { openToNetwork });
}

/**
 * Give the network setting to the desktop app. It starts nothing: the app's
 * own server is restarted to match, once no import or other desktop job
 * runs, and a Message Crate the app only found is left as it is.
 */
export async function invokeSetOpenToNetwork(openToNetwork: boolean): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("set_open_to_network", { openToNetwork });
}

/** The state of the app's own Message Crate, read without starting it. */
export async function invokeLocalServerStatus(): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("local_server_status");
}

/** Open the Message Crate Directory, `~/message-crate`, which holds the app's own database. */
export async function invokeOpenMessageCrateDirectory(): Promise<void> {
  await invoke("open_message_crate_directory");
}
