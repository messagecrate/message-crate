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

/** How the Import form asks for one Android source's backup. */
export type BackupField = {
  label: string;
  /** True for a directory picker, false for a file picker. */
  directory: boolean;
  /** The file picker's filters, for a backup that is one file. */
  filters?: { name: string; extensions: string[] }[];
  hint: string;
  placeholder: string;
};

/**
 * The backup field, per source. SMS Backup & Restore writes each backup as
 * one `.xml` file, and an Import Run reads one backup, so its field takes one
 * file. GO SMS Pro and SMS Backup+ backups are directories.
 */
export function backupField(source: string): BackupField {
  switch (source) {
    case SMS_BACKUP_PLUS_SOURCE:
      return {
        label: "Backup Directory",
        directory: true,
        hint: "Point at a directory of .eml files archived from SMS Backup+ (Gmail or IMAP). This does not connect to a mail server.",
        placeholder: "Directory containing .eml files",
      };
    case GO_SMS_PRO_SOURCE:
      return {
        label: "Backup Directory",
        directory: true,
        hint: "Point at the directory holding the GO SMS Pro backup files.",
        placeholder: "Directory containing gosms_sys*.xml backup files",
      };
    default:
      return {
        label: "Backup File",
        directory: false,
        filters: [{ name: "SMS Backup & Restore", extensions: ["xml"] }],
        hint: "Point at one SMS Backup & Restore .xml file (not a ZIP). To import another backup, start another import. Unlock an encrypted backup before selecting it.",
        placeholder: "Path to an sms-*.xml backup file",
      };
  }
}
