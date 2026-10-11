/**
 * The Import form's message when the chosen iPhone backup is encrypted and
 * Encryption password is empty, for Apple Messages and WhatsApp alike. The
 * Apple Messages Reader and the WhatsApp exporter refuse the same backup
 * with the same words.
 */
export const ENCRYPTED_BACKUP_PASSWORD_REQUIRED =
  "The backup is encrypted. Fill in the Encryption password.";

/**
 * The Encryption password field's error for an iPhone backup: required when
 * the backup is known to be encrypted, and nothing when it is not or when
 * whether it is encrypted is unknown.
 */
export function encryptedBackupPasswordError(
  backupEncrypted: boolean | null,
  backupPassword: string,
): string | null {
  return backupEncrypted === true && backupPassword.trim() === ""
    ? ENCRYPTED_BACKUP_PASSWORD_REQUIRED
    : null;
}
