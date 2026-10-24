/**
 * The problem readiness records when the chosen iPhone backup is encrypted
 * and Encryption password is empty, for Apple Messages and WhatsApp alike.
 * It keeps Import off and is not shown under the field: the asterisk and the
 * field's help text already say a password is needed (#3167). The Apple
 * Messages Reader and the WhatsApp exporter refuse the same backup with the
 * same words.
 */
export const ENCRYPTED_BACKUP_PASSWORD_REQUIRED =
  "The backup is encrypted. Fill in the Encryption password.";

/** The Encryption password field's help text, the one place the form says the backup is encrypted. */
export const ENCRYPTED_BACKUP_PASSWORD_HINT = "This backup is encrypted. Enter its password.";

/**
 * True when the Import form shows the Encryption password for an iPhone
 * backup: only when the backup is known to be encrypted. A backup that is
 * not, or whose state is not known yet, takes no password, so the field is
 * hidden and a password typed earlier is not sent with the import, where
 * the unencrypted backup would refuse it.
 */
export function encryptionPasswordShown(backupEncrypted: boolean | null): boolean {
  return backupEncrypted === true;
}

/**
 * The Encryption password field's error for an iPhone backup: required when
 * the backup is known to be encrypted, and nothing when it is not or when
 * whether it is encrypted is unknown.
 */
export function encryptedBackupPasswordError(
  backupEncrypted: boolean | null,
  backupPassword: string,
): string | null {
  return encryptionPasswordShown(backupEncrypted) && backupPassword.trim() === ""
    ? ENCRYPTED_BACKUP_PASSWORD_REQUIRED
    : null;
}
