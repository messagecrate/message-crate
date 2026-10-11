import type { PathStat as DesktopPathStat } from "./tauri";

/** What the Import form knows about a path the person typed or picked. */
export type ImportPathStat = Pick<DesktopPathStat, "exists" | "isFile" | "isDirectory">;

/** Whether a path field takes a file or a directory. */
export type PathKind = "file" | "directory";

/** The kind of path a field takes, and the field's own message for a path of any other kind. */
export type PathRequirement = { expected: PathKind; kindError: string };

export const PATH_MISSING = "This path does not exist.";

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
  if (!stat.exists) {
    errors[key] = PATH_MISSING;
    return;
  }
  if (expected === "directory" ? !stat.isDirectory : !stat.isFile) {
    errors[key] = kindError;
  }
}
