import type { ImportPathStat } from "../lib/pathChecks";
import type { PathStat, PathUnreadable } from "../lib/tauri";

/**
 * What the desktop app's `path_stat` answers, for a file of 1000 bytes
 * unless `overrides` says otherwise.
 */
export function desktopPathStat(overrides: Partial<PathStat> = {}): PathStat {
  return {
    exists: true,
    isFile: true,
    isDirectory: false,
    sizeBytes: 1000,
    modifiedUnixMs: 1_700_000_000_000,
    unreadable: null,
    ...overrides,
  };
}

/** What the Import form's path check finds for each kind of path. */
export const DIRECTORY_STAT: ImportPathStat = {
  exists: true,
  isFile: false,
  isDirectory: true,
  unreadable: null,
};
export const FILE_STAT: ImportPathStat = {
  exists: true,
  isFile: true,
  isDirectory: false,
  unreadable: null,
};
export const MISSING_STAT: ImportPathStat = {
  exists: false,
  isFile: false,
  isDirectory: false,
  unreadable: null,
};
/** A socket, a device file, or a pipe: it exists but is neither. */
export const NEITHER_STAT: ImportPathStat = {
  exists: true,
  isFile: false,
  isDirectory: false,
  unreadable: null,
};
/** The refusal macOS gives for a path it protects until the app has Full Disk Access. */
export const PERMISSION_DENIED: PathUnreadable = {
  kind: "permission_denied",
  reason: "Operation not permitted (os error 1)",
};
/** A path the app is not allowed to read, such as one macOS protects until the app has Full Disk Access. */
export const DENIED_STAT: ImportPathStat = {
  exists: false,
  isFile: false,
  isDirectory: false,
  unreadable: PERMISSION_DENIED,
};
