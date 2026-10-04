import { attachmentName } from "./attachmentMedia";
import { saveFile } from "./saveFile";
import { fetchAsset } from "./serverApi";
import type { MessageAttachment } from "./types";

/**
 * Save an attachment's original under the name the export gave it. A
 * download is always the original, never the Preview: the original is the
 * record of what was sent (`docs/architecture/media.md`, rule 2).
 *
 * Returns false when the person closed the desktop app's Save dialog.
 */
export async function downloadAttachment(attachment: MessageAttachment): Promise<boolean> {
  if (!attachment.sha256) throw new Error("This attachment has no stored file.");
  const original = await fetchAsset(attachment.sha256, { version: "original" });
  return saveFile(attachmentName(attachment), original);
}
