import type { ImportPathStat } from "../lib/pathChecks";

/** What the Import form's path check finds for each kind of path. */
export const DIRECTORY_STAT: ImportPathStat = { exists: true, isFile: false, isDirectory: true };
export const FILE_STAT: ImportPathStat = { exists: true, isFile: true, isDirectory: false };
export const MISSING_STAT: ImportPathStat = { exists: false, isFile: false, isDirectory: false };
/** A socket, a device file, or a pipe: it exists but is neither. */
export const NEITHER_STAT: ImportPathStat = { exists: true, isFile: false, isDirectory: false };
