/** What the Import form knows about a path the person typed or picked. */
export type PathStat = {
  exists: boolean;
  isFile: boolean;
  isDirectory: boolean;
};

export const PATH_MISSING = "This path does not exist.";

/**
 * Check an optional path field of the Import form. An empty path is fine,
 * and so is one not checked yet (`stat` is null). A missing path gets
 * `PATH_MISSING`; a file where a directory is needed, or a directory where a
 * file is needed, gets the field's own `kindError`.
 */
export function checkOptionalPath<K extends string>(
  path: string,
  stat: PathStat | null,
  errors: Partial<Record<K, string>>,
  key: K,
  kindError: string,
  expectDirectory: boolean,
): void {
  if (path.trim() === "" || stat === null) {
    return;
  }
  if (!stat.exists) {
    errors[key] = PATH_MISSING;
    return;
  }
  if (expectDirectory ? stat.isFile : stat.isDirectory) {
    errors[key] = kindError;
  }
}
