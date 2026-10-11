import type { PathStat as DesktopPathStat } from "./tauri";

/** What the Import form knows about a path the person typed or picked. */
export type PathStat = Pick<DesktopPathStat, "exists" | "isFile" | "isDirectory">;

/** Whether a path field takes a file or a directory. */
export type PathKind = "file" | "directory";

export const PATH_MISSING = "This path does not exist.";

/**
 * Check an optional path field of the Import form. An empty path is fine,
 * and so is one not checked yet (`stat` is null). A missing path gets
 * `PATH_MISSING`; a path that is not the `expected` kind gets the field's
 * own `kindError`.
 */
export function checkOptionalPath<K extends string>(
  path: string,
  stat: PathStat | null,
  errors: Partial<Record<K, string>>,
  key: K,
  kindError: string,
  expected: PathKind,
): void {
  if (path.trim() === "" || stat === null) {
    return;
  }
  if (!stat.exists) {
    errors[key] = PATH_MISSING;
    return;
  }
  if (expected === "directory" ? stat.isFile : stat.isDirectory) {
    errors[key] = kindError;
  }
}
