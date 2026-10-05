/**
 * True when two directory paths name the same directory as typed.
 *
 * `message-reexport` canonicalizes both paths and refuses to write into its
 * own input (`crates/libs/reexport/src/lib.rs`), so the screen catches the
 * plain case before a job starts: same text, ignoring surrounding whitespace
 * and trailing slashes. Symlinks and case-insensitive file systems are left
 * to the Rust side, which reports them as a job error.
 */
export function sameDirectory(a: string, b: string): boolean {
  const left = normalizeDirectory(a);
  const right = normalizeDirectory(b);
  return left !== "" && left === right;
}

function normalizeDirectory(path: string): string {
  // Keep a lone root ("/" or "C:\") intact; strip trailing separators otherwise.
  return path.trim().replace(/(?<=.)[/\\]+$/, "");
}
