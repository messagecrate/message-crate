import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

interface HomeDirInfo {
  path: string;
  os: string;
}

/** User home directory and operating system name from the desktop backend. */
export async function invokeHomeDir(): Promise<HomeDirInfo> {
  return invoke("home_dir");
}

export interface PathStat {
  exists: boolean;
  isFile: boolean;
  isDirectory: boolean;
  sizeBytes: number;
  modifiedUnixMs: number | null;
  /** Why the operating system would not say what is at the path; null when it answered. */
  unreadable: PathUnreadable | null;
}

/** The operating system's refusal to say what is at a path. */
export interface PathUnreadable {
  /**
   * `permission_denied` when the app is not allowed to read the path, such as
   * one macOS protects until the app has Full Disk Access; `other` for any
   * other error from the operating system.
   */
  kind: "permission_denied" | "other";
  /** The operating system's own words for the refusal. */
  reason: string;
}

/**
 * Show the Save dialog with `fileName` filled in, and have the desktop app
 * download `url`, a Media Link to an attachment's original, to the file the
 * person chose. The app writes the server's answer to the disk as it arrives,
 * so a video of hundreds of megabytes is never held in the window or sent to
 * the app whole (#1739).
 *
 * Resolves false when the person closed the dialog without choosing a place,
 * and true once the file is written.
 */
export async function invokeSaveDownload(url: string, fileName: string): Promise<boolean> {
  return invoke<boolean>("save_download", { url, fileName });
}

/** Whether a path exists and whether it is a file or directory. */
export async function invokePathStat(path: string): Promise<PathStat> {
  return invoke("path_stat", { path });
}

/** Whether an iOS backup directory is encrypted, or null when unknown. */
export async function invokeIosBackupEncrypted(path: string): Promise<boolean | null> {
  return invoke("ios_backup_encrypted", { path });
}

/**
 * Addresses an iMessage backup's device sent from. The desktop opens the
 * source the same way the extractor will, so a source it cannot read fails
 * here with the message the extractor would give.
 */
export async function invokeImessageBackupIdentities(args: {
  path: string;
  ios: boolean;
  backupPassword: string;
}): Promise<string[]> {
  return invoke("imessage_backup_identities", {
    path: args.path,
    ios: args.ios,
    backupPassword: args.backupPassword.trim() === "" ? null : args.backupPassword,
  });
}

/**
 * Open a file or directory with the operating system's default handler. The
 * desktop process opens only a run directory it made, or a path inside one.
 */
export async function invokeOpenPath(path: string): Promise<void> {
  await invoke("open_path", { path });
}

/**
 * Show the Save dialog with `fileName` filled in, and write `bytes` where the
 * person chose. The bytes go as the request's raw body and the name as a
 * header, so a file of megabytes is not written out as JSON. A header carries
 * ASCII only, so the name goes as a JSON string with every other character
 * escaped.
 *
 * Resolves false when the person closed the dialog without choosing a place.
 */
export async function invokeSaveFile(bytes: Uint8Array, fileName: string): Promise<boolean> {
  return invoke<boolean>("save_file", bytes, {
    headers: { "file-name": asciiJson(fileName) },
  });
}

/** `value` as a JSON string, with every character outside ASCII written as `\uXXXX`. */
function asciiJson(value: string): string {
  return JSON.stringify(value).replace(
    /[\u007f-\uffff]/g,
    (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
}

/**
 * Show the operating system's dialog to choose one file, or one directory
 * when `directory` is set. Resolves null when the person closed the dialog
 * without a choice.
 */
export async function pickPath(options: {
  directory?: boolean;
  filters?: { name: string; extensions: string[] }[];
}): Promise<string | null> {
  const result = options.directory
    ? await openDialog({ directory: true, multiple: false })
    : await openDialog({ multiple: false, filters: options.filters });
  return typeof result === "string" ? result : null;
}
