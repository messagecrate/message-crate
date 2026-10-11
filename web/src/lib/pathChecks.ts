import { type PathStat as DesktopPathStat, invokePathStat, type PathUnreadable } from "./tauri";

/**
 * Why the Import form knows nothing about a path: the operating system
 * refused to describe it (`PathUnreadable`), or the desktop app's check
 * itself failed (`check_failed`), which says nothing about the path at all.
 */
export type ImportPathUnknown = PathUnreadable | { kind: "check_failed"; reason: string };

/**
 * What the Import form knows about a path the person typed or picked. When
 * `unreadable` is set, `exists`, `isFile` and `isDirectory` are all false,
 * because nothing is known.
 */
export type ImportPathStat = Pick<DesktopPathStat, "exists" | "isFile" | "isDirectory"> & {
  unreadable: ImportPathUnknown | null;
};

/** Whether a path field takes a file or a directory. */
export type PathKind = "file" | "directory";

/** The kind of path a field takes, and the field's own message for a path of any other kind. */
export type PathRequirement = { expected: PathKind; kindError: string };

export const PATH_MISSING = "This path does not exist.";

/**
 * What the form says about a path it knows nothing about. Such a path may
 * well be there, so it is never `PATH_MISSING`. A permission refusal says
 * what lets Message Crate read it; a failed check says the path was never
 * looked at, so it does not send the person to change permissions.
 */
function unknownPathMessage({ kind, reason }: ImportPathUnknown): string {
  const said = reason.trim().replace(/\.$/, "");
  switch (kind) {
    case "permission_denied":
      return `Message Crate isn't allowed to read this path. The system says: ${said}. On a Mac, give Message Crate Full Disk Access in System Settings, under Privacy & Security.`;
    case "other":
      return `Message Crate could not read this path. The system says: ${said}.`;
    case "check_failed":
      return `Message Crate could not check this path. The check failed with: ${said}.`;
  }
}

/**
 * Check a path the person typed or picked. Null for an empty path. A check
 * the desktop app could not run says nothing about the path, so it comes
 * back as `check_failed`, never as a path that is missing.
 */
export async function probeImportPath(path: string): Promise<ImportPathStat | null> {
  const trimmed = path.trim();
  if (trimmed === "") return null;
  try {
    return await invokePathStat(trimmed);
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
