import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./tauri-check";

/**
 * Save `contents` as a file named `fileName`.
 *
 * In a browser the file goes to the browser's downloads, as any download
 * does. The desktop app's window has no downloads, so the app shows the Save
 * dialog and writes the file where the person chose. The window passes only
 * the name and the bytes: the path comes from the dialog, never the window.
 * The bytes go as the request's raw body and the name as a header, so a file
 * of megabytes is not written out as JSON. A header carries ASCII only, so
 * the name goes as a JSON string with every other character escaped.
 *
 * Returns false when the person closed the desktop app's dialog without
 * choosing a place, and true once the file is on its way.
 */
export async function saveFile(fileName: string, contents: Blob): Promise<boolean> {
  if (isTauri()) {
    const bytes = new Uint8Array(await contents.arrayBuffer());
    return await invoke<boolean>("save_file", bytes, {
      headers: { "file-name": asciiJson(fileName) },
    });
  }
  const url = URL.createObjectURL(contents);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  return true;
}

/**
 * In the desktop app, show the Save dialog with `fileName` filled in and have
 * the app download `url`, a Media Link to an attachment's original, to the
 * file the person chose. The app copies the server's answer to the disk as it
 * arrives, so a video of hundreds of megabytes is never held in the window or
 * sent to the app whole, as `saveFile` would have it.
 *
 * Returns false when the person closed the dialog without choosing a place,
 * and true once the file is written.
 */
export async function saveDownload(fileName: string, url: string): Promise<boolean> {
  return await invoke<boolean>("save_download", { url, fileName });
}

/** `value` as a JSON string, with every character outside ASCII written as `\uXXXX`. */
function asciiJson(value: string): string {
  return JSON.stringify(value).replace(
    /[\u007f-\uffff]/g,
    (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
}
