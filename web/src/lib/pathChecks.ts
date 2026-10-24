import {
  type PathStat as DesktopPathStat,
  invokeHomeDir,
  invokePathStat,
  type PathUnreadable,
} from "./desktop/fs";

/** An operating system the desktop app ships for, as the app names it. */
export type DesktopOs = "macos" | "windows" | "linux";

/**
 * Why the Import form knows nothing about a path: the operating system would
 * not let the app read it (`permission_denied`, with the operating system,
 * which decides how to give the app access), refused for another reason
 * (`other`), or the desktop app's check itself failed (`check_failed`), which
 * says nothing about the path at all.
 */
export type ImportPathUnknown =
  | {
      kind: "permission_denied";
      reason: string;
      /** The operating system that refused; null when the desktop app could not say. */
      os: DesktopOs | null;
    }
  | { kind: "other" | "check_failed"; reason: string };

/**
 * What the Import form knows about a path the person typed or picked. When
 * `unreadable` is set, `exists`, `isFile` and `isDirectory` are all false,
 * because nothing is known.
 */
export type ImportPathStat = Pick<DesktopPathStat, "exists" | "isFile" | "isDirectory"> & {
  unreadable: ImportPathUnknown | null;
};

/**
 * What the backup path of a source whose only checked path is its backup
 * points at: the Android SMS sources, iMazing and OpenExtract. Null until it
 * is checked, and always in the browser.
 */
export type BackupPathStats = { backup: ImportPathStat | null };

export const NO_BACKUP_PATH_STATS: BackupPathStats = { backup: null };

/** Whether a path field takes a file or a directory. */
export type PathKind = "file" | "directory";

/** The kind of path a field takes, and the field's own message for a path of any other kind. */
export type PathRequirement = { expected: PathKind; kindError: string };

export const PATH_MISSING = "This path does not exist.";

/**
 * What gives Message Crate access to a path the operating system `os` would
 * not let it read, as a sentence to follow the refusal, or "" when there is
 * nothing to say. macOS protects some directories until the app has Full
 * Disk Access; on Windows and Linux the app reads as the person's own user,
 * so it is that user's permission that counts.
 */
function accessSentence(os: DesktopOs | null): string {
  switch (os) {
    case "macos":
      return " Give Message Crate access to it, such as Full Disk Access in System Settings.";
    case "windows":
      return " Check that your Windows user may read it, on the Security tab of its Properties.";
    case "linux":
      return " Check that your Linux user may read it.";
    case null:
      return "";
  }
}

/**
 * The operating system this desktop app runs on, from its answer to
 * `home_dir`. Null on any other system, and when `home_dir` fails, which it
 * does only when the system reports no home directory for the person's user.
 * A message built with a null system says nothing about access.
 */
export async function desktopOs(): Promise<DesktopOs | null> {
  try {
    return toDesktopOs((await invokeHomeDir()).os);
  } catch {
    return null;
  }
}

/** An operating system name from the desktop app, or null for one it does not ship for. */
export function toDesktopOs(os: string): DesktopOs | null {
  return os === "macos" || os === "windows" || os === "linux" ? os : null;
}

/**
 * Add the operating system to a refusal from the desktop app's `path_stat`
 * when it is a permission refusal, so its message can say how to give
 * Message Crate access there. `os` is asked only for a permission refusal;
 * a caller describing several refusals passes one that asks the app once.
 */
export async function describeUnreadable(
  unreadable: PathUnreadable,
  os: () => Promise<DesktopOs | null> = desktopOs,
): Promise<ImportPathUnknown> {
  if (unreadable.kind !== "permission_denied") {
    return { kind: unreadable.kind, reason: unreadable.reason };
  }
  return { kind: "permission_denied", reason: unreadable.reason, os: await os() };
}

/**
 * What Message Crate says about a path it knows nothing about, naming the
 * path as `subject`: "this path" on the Import form, "it" after the path
 * itself. Such a path may well be there, so it is never `PATH_MISSING`. A
 * permission refusal says what lets Message Crate read it on the operating
 * system that refused; a failed check says the path was never looked at, so
 * it does not send the person to change permissions.
 */
export function unknownPathMessage(
  unknown: ImportPathUnknown,
  subject: "this path" | "it" = "this path",
): string {
  const said = unknown.reason.trim().replace(/\.$/, "");
  switch (unknown.kind) {
    case "permission_denied":
      return `Message Crate isn't allowed to read ${subject}. The system says: ${said}.${accessSentence(unknown.os)}`;
    case "other":
      return `Message Crate could not read ${subject}. The system says: ${said}.`;
    case "check_failed":
      return `Message Crate could not check ${subject}. The check failed with: ${said}.`;
  }
}

/**
 * Check a path on this computer: one the person typed or picked on the
 * Import form, or an Import Run's own directory. Null for an empty path. A
 * check the desktop app could not run says nothing about the path, so it
 * comes back as `check_failed`, never as a path that is missing.
 */
export async function probePath(path: string): Promise<ImportPathStat | null> {
  const trimmed = path.trim();
  if (trimmed === "") return null;
  let stat: DesktopPathStat;
  try {
    stat = await invokePathStat(trimmed);
  } catch (err) {
    return {
      exists: false,
      isFile: false,
      isDirectory: false,
      unreadable: {
        kind: "check_failed",
        reason: err instanceof Error ? err.message : String(err),
      },
    };
  }
  return {
    exists: stat.exists,
    isFile: stat.isFile,
    isDirectory: stat.isDirectory,
    unreadable: stat.unreadable && (await describeUnreadable(stat.unreadable)),
  };
}

/**
 * Check an optional path field of the Import form. An empty path is fine,
 * and so is one not checked yet (`stat` is null). Any other path is checked
 * as `checkRequiredPath` checks it.
 */
export function checkOptionalPath<K extends string>(
  path: string,
  stat: ImportPathStat | null,
  errors: Partial<Record<K, string>>,
  key: K,
  requirement: PathRequirement,
): void {
  if (path.trim() === "" || stat === null) {
    return;
  }
  checkRequiredPath(stat, errors, key, requirement);
}

/**
 * Check a path field of the Import form against what the check found. A
 * path the operating system would not describe says why; a
 * missing path gets `PATH_MISSING`; a path that exists but is not the
 * `expected` kind gets the field's own `kindError`. That covers a file where
 * a directory is needed, a directory where a file is, and a path that is
 * neither, such as a socket or a device file.
 */
export function checkRequiredPath<K extends string>(
  stat: ImportPathStat,
  errors: Partial<Record<K, string>>,
  key: K,
  { expected, kindError }: PathRequirement,
): void {
  if (stat.unreadable) {
    errors[key] = unknownPathMessage(stat.unreadable);
    return;
  }
  if (!stat.exists) {
    errors[key] = PATH_MISSING;
    return;
  }
  if (expected === "directory" ? !stat.isDirectory : !stat.isFile) {
    errors[key] = kindError;
  }
}
