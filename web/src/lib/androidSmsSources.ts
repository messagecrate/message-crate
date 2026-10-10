/**
 * The Android SMS backup sources share one form: the backup, media options,
 * and the owner's phone numbers so the exporter can tell sent from received.
 * The backup is one `.xml` file for SMS Backup & Restore and a directory for
 * the others. SMS Backup+ adds the owner's email addresses, because its
 * archive is Gmail-backed and the sender of a sent message is an email
 * account.
 */
export const SMS_BACKUP_RESTORE_SOURCE = "sms-backup-restore";
export const GO_SMS_PRO_SOURCE = "go-sms-pro";
export const SMS_BACKUP_PLUS_SOURCE = "sms-backup-plus";

const ANDROID_SMS_SOURCES: ReadonlySet<string> = new Set([
  SMS_BACKUP_RESTORE_SOURCE,
  GO_SMS_PRO_SOURCE,
  SMS_BACKUP_PLUS_SOURCE,
]);

/** True for a source whose form asks for the backup device's phone numbers. */
export function isAndroidSmsSource(source: string): boolean {
  return ANDROID_SMS_SOURCES.has(source);
}

/** True for the one Android source that also needs the owner's email addresses. */
export function needsOwnerEmails(source: string): boolean {
  return source === SMS_BACKUP_PLUS_SOURCE;
}

/** Split a typed list of addresses on commas, semicolons and whitespace; drops blanks. */
export function splitEmails(raw: string): string[] {
  return raw
    .split(/[,;\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/**
 * True when the source's backup is one file: SMS Backup & Restore writes each
 * backup as one `.xml` file, and an Import Run reads one backup. GO SMS Pro
 * and SMS Backup+ backups are directories.
 */
export function backupIsFile(source: string): boolean {
  return source === SMS_BACKUP_RESTORE_SOURCE;
}

/** The backup field's label, per source. */
export function backupPathLabel(source: string): string {
  return backupIsFile(source) ? "Backup File" : "Backup Directory";
}

/** What to pick for the backup, per source, for the form's hint line. */
export function backupPathHint(source: string): string {
  switch (source) {
    case SMS_BACKUP_PLUS_SOURCE:
      return "Point at a directory of .eml files archived from SMS Backup+ (Gmail or IMAP). This does not connect to a mail server.";
    case GO_SMS_PRO_SOURCE:
      return "Point at the directory holding the GO SMS Pro backup files.";
    default:
      return "Point at one SMS Backup & Restore .xml file (not a ZIP). To import another backup, start another import. Unlock an encrypted backup before selecting it.";
  }
}

/** The backup each source writes, for the backup field's placeholder. */
export function backupPathPlaceholder(source: string): string {
  switch (source) {
    case SMS_BACKUP_PLUS_SOURCE:
      return "Directory containing .eml files";
    case GO_SMS_PRO_SOURCE:
      return "Directory containing gosms_sys*.xml backup files";
    default:
      return "Path to an sms-*.xml backup file";
  }
}
