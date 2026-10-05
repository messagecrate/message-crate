import {
  type ExportDir,
  type ExportFormat,
  invokeCreateExportDir,
  invokeDiscardExportDir,
  invokeFinishExportDir,
} from "./tauri";

/**
 * Run `write` with a directory of its own in the Export Directory, for an
 * Export or a Convert. `chosen` is the destination the person chose, or
 * empty; `write` decides what goes where.
 *
 * Once `write` has written the result, the desktop finishes the directory,
 * which leaves only the result in it, or deletes it when the result went to
 * `chosen`. A `write` that fails or is cancelled deletes the directory, and
 * with it any copy of the messages; a directory that cannot be deleted is
 * named through `onLog` rather than failing a second time. A finish that
 * fails after the result is written keeps the directory, and its error says
 * where the result is.
 */
export async function writeInExportDir(
  kind: "export" | "convert",
  format: ExportFormat,
  chosen: string,
  write: (dir: ExportDir) => Promise<void>,
  onLog: (line: string) => void,
): Promise<void> {
  const made = await invokeCreateExportDir(kind, format, chosen);
  let written = false;
  try {
    await write(made);
    written = true;
    await invokeFinishExportDir(made.dir);
  } finally {
    if (!written) {
      try {
        await invokeDiscardExportDir(made.dir);
      } catch (cleanupError: unknown) {
        onLog(
          `Could not delete ${made.dir}: ${
            cleanupError instanceof Error ? cleanupError.message : String(cleanupError)
          }`,
        );
      }
    }
  }
}
