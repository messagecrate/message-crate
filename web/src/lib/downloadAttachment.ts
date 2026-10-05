import { attachmentName } from "./attachmentMedia";
import { saveDownload, saveFile } from "./saveFile";
import { createMediaLink, fetchAsset } from "./serverApi";
import { isTauri } from "./tauri-check";
import type { MessageAttachment } from "./types";

/**
 * Save an attachment's original under the name the export gave it. A
 * download is always the original, never the Preview: the original is the
 * record of what was sent (`docs/architecture/media.md`, rule 2).
 *
 * In the desktop app the window makes a Media Link and the app downloads the
 * original to the file the person chose, a buffer at a time, so a large video
 * is never held in memory whole. A browser fetches the original and hands it
 * to its downloads.
 *
 * Returns false when the person closed the desktop app's Save dialog.
 */
export async function downloadAttachment(attachment: MessageAttachment): Promise<boolean> {
  if (!attachment.sha256) throw new Error("This attachment has no stored file.");
  const name = attachmentName(attachment);
  if (isTauri()) {
    const link = await createMediaLink(attachment.sha256);
    return saveDownload(name, link.url);
  }
  const original = await fetchAsset(attachment.sha256, { version: "original" });
  return saveFile(name, original);
}
