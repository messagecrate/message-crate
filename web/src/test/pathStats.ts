import type { ImportPathStat } from "../lib/pathChecks";

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
/** A path the app is not allowed to read, such as one macOS protects until the app has Full Disk Access. */
export const DENIED_STAT: ImportPathStat = {
  exists: false,
  isFile: false,
  isDirectory: false,
  unreadable: { permissionDenied: true, reason: "Operation not permitted (os error 1)" },
};
