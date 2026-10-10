import { mediaExtractFields } from "./sbrExtractFields";
import type { AttachmentChoices, ExtractConfig } from "./types";

/** iMessage extract method keys sent to the desktop `extract` command. */
export type ImessageExtractSource = "imessage-ios" | "imessage-macos" | "imessage-jailbreak";

/**
 * Build extract payload fields for an iMessage method.
 *
 * Media options apply to all three methods. Backup password and obfuscate
 * apply only to iPhone backups. Attachment root and Apple Contacts apply
 * only to Mac Messages and jailbreak, and only when the path is non-empty
 * after trim so Mac auto-scan still runs.
 */
export function imessageExtractFields(
  args: AttachmentChoices & {
    source: ImessageExtractSource;
    backupPassword: string;
    obfuscate: boolean;
    attachmentRoot: string;
    appleContacts: string;
  },
): Pick<
  ExtractConfig,
  | "attachment_media"
  | "media_max_resolution"
  | "media_max_fps"
  | "media_min_size"
  | "obfuscate"
  | "backup_password"
  | "attachment_root"
  | "apple_contacts"
> {
  const fields: ReturnType<typeof imessageExtractFields> = {
    ...mediaExtractFields(args),
  };

  if (args.source === "imessage-ios") {
    const password = args.backupPassword.trim();
    if (password) {
      fields.backup_password = password;
    }
    fields.obfuscate = args.obfuscate;
    return fields;
  }

  const attachmentRoot = args.attachmentRoot.trim();
  if (attachmentRoot) {
    fields.attachment_root = attachmentRoot;
  }
  const appleContacts = args.appleContacts.trim();
  if (appleContacts) {
    fields.apple_contacts = appleContacts;
  }
  return fields;
}
