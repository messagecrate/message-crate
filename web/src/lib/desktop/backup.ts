import { invoke } from "@tauri-apps/api/core";

/** Whether an iOS backup directory is encrypted, or null when unknown. */
export async function invokeIosBackupEncrypted(path: string): Promise<boolean | null> {
  return invoke("ios_backup_encrypted", { path });
}

/**
 * Addresses an iMessage backup's device sent from. The desktop opens the
 * source the same way the extractor will, so a source it cannot read fails
 * here with the message the extractor would give.
 */
export async function invokeImessageBackupIdentities(args: {
  path: string;
  ios: boolean;
  backupPassword: string;
}): Promise<string[]> {
  return invoke("imessage_backup_identities", {
    path: args.path,
    ios: args.ios,
    backupPassword: args.backupPassword.trim() === "" ? null : args.backupPassword,
  });
}
