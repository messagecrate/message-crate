import { attachmentChoicesOf } from "../attachmentChoices";
import { type ImportSourceId, sourceLabel } from "../exportSources";
import { checkRequiredPath, type ImportPathStat, type PathRequirement } from "../pathChecks";
import { sbrExtractFields } from "../sbrExtractFields";
import type { BackupField, ImportSourceDescriptor, ReadinessInput } from "./types";

/**
 * The Android SMS backup sources share one form: the backup, media options,
 * and the owner's phone numbers so the exporter can tell sent from received.
 * The backup is one `.xml` file for SMS Backup & Restore and a directory for
 * the others. SMS Backup+ adds the owner's email addresses, because its
 * archive is Gmail-backed and the sender of a sent message is an email
 * account.
 */
export const SMS_BACKUP_RESTORE_SOURCE_ID = "sms-backup-restore";
export const GO_SMS_PRO_SOURCE_ID = "go-sms-pro";
export const SMS_BACKUP_PLUS_SOURCE_ID = "sms-backup-plus";

const ANDROID_SMS_SOURCE_IDS = new Set<string>([
  SMS_BACKUP_RESTORE_SOURCE_ID,
  GO_SMS_PRO_SOURCE_ID,
  SMS_BACKUP_PLUS_SOURCE_ID,
]);

/** True for the three Android SMS backup sources, which share one form. */
export function isAndroidSmsSource(source: string): boolean {
  return ANDROID_SMS_SOURCE_IDS.has(source);
}

/** What the Android SMS backup path points at: null until it is checked, and always in the browser. */
export type AndroidSmsPathStats = { backup: ImportPathStat | null };

export const ANDROID_SMS_NO_STATS: AndroidSmsPathStats = { backup: null };

export const SMS_BACKUP_RESTORE_ERR_NOT_FILE = "Pick the SMS Backup & Restore .xml file.";
export const ANDROID_SMS_ERR_NOT_DIRECTORY = "Pick the backup directory.";

/** Split a typed list of addresses on commas, semicolons and whitespace; drops blanks. */
export function splitEmails(raw: string): string[] {
  return raw
    .split(/[,;\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/**
 * Import can start once the backup is the kind of path the source takes, a
 * phone number is given (and an email address for SMS Backup+), the
 * profile's phones have been read, and a number off the profile has been
 * allowed. A backup path not checked yet keeps Import off with nothing said,
 * as an empty one does, so a directory is never imported as an `.xml` file.
 */
function androidSmsReadiness(
  input: ReadinessInput,
  asksOwnerEmails: boolean,
  requirement: PathRequirement,
): { enabled: boolean; errors: Partial<Record<"backupPath", string>> } {
  const errors: Partial<Record<"backupPath", string>> = {};
  const stat = input.androidSmsStats.backup;
  if (input.backupPath.trim() !== "" && stat !== null) {
    checkRequiredPath(stat, errors, "backupPath", requirement);
  }
  const entry = input.ownerPhoneEntry;
  const hasPhones = input.ownerPhones.length > 0 || entry.draftPending;
  const hasEmail = !asksOwnerEmails || splitEmails(input.ownerEmails).length > 0;
  const enabled =
    input.backupPath.trim() !== "" &&
    stat !== null &&
    errors.backupPath === undefined &&
    hasPhones &&
    hasEmail &&
    input.profilePhonesReady &&
    (!entry.mismatch || entry.mismatchAck);
  return { enabled, errors };
}

function androidSmsSource(args: {
  id: ImportSourceId;
  asksOwnerEmails: boolean;
  backupField: BackupField;
}): ImportSourceDescriptor {
  const label = sourceLabel(args.id);
  const method = { id: args.id, label };
  return {
    id: args.id,
    label,
    methods: [method],
    defaultMethod: args.id,
    visibleMethods: () => [method],
    showsAttachmentOptions: true,
    asksOwnerPhones: true,
    asksOwnerEmails: args.asksOwnerEmails,
    asksWhatsappOwnerPhone: false,
    rememberedPaths: [],
    needsWtsexporter: false,
    processingOptions: () => ["obfuscate"],
    extractFields: (form) =>
      sbrExtractFields({
        ...attachmentChoicesOf(form),
        ownerPhones: form.ownerPhones,
        ownerEmails: form.ownerEmails,
        obfuscate: form.obfuscate,
      }),
    appleIdentityRead: () => null,
    snapshotSecret: () => null,
    backupField: () => args.backupField,
    readiness: (input) =>
      androidSmsReadiness(input, args.asksOwnerEmails, {
        expected: args.backupField.directory ? "directory" : "file",
        kindError: args.backupField.directory
          ? ANDROID_SMS_ERR_NOT_DIRECTORY
          : SMS_BACKUP_RESTORE_ERR_NOT_FILE,
      }),
  };
}

/**
 * SMS Backup & Restore writes each backup as one `.xml` file, and an Import
 * Run reads one backup, so its field takes one file.
 */
export const SMS_BACKUP_RESTORE_SOURCE = androidSmsSource({
  id: SMS_BACKUP_RESTORE_SOURCE_ID,
  asksOwnerEmails: false,
  backupField: {
    label: "Backup File",
    directory: false,
    filters: [{ name: "SMS Backup & Restore", extensions: ["xml"] }],
    hint: "Point at one SMS Backup & Restore .xml file (not a ZIP). To import another backup, start another import. Unlock an encrypted backup before selecting it.",
    placeholder: "Path to an sms-*.xml backup file",
  },
});

/** A GO SMS Pro backup is a directory. */
export const GO_SMS_PRO_SOURCE = androidSmsSource({
  id: GO_SMS_PRO_SOURCE_ID,
  asksOwnerEmails: false,
  backupField: {
    label: "Backup Directory",
    directory: true,
    hint: "Point at the directory holding the GO SMS Pro backup files.",
    placeholder: "Directory containing gosms_sys*.xml backup files",
  },
});

/** An SMS Backup+ archive is a directory of `.eml` files. */
export const SMS_BACKUP_PLUS_SOURCE = androidSmsSource({
  id: SMS_BACKUP_PLUS_SOURCE_ID,
  asksOwnerEmails: true,
  backupField: {
    label: "Backup Directory",
    directory: true,
    hint: "Point at a directory of .eml files archived from SMS Backup+ (Gmail or IMAP). This does not connect to a mail server.",
    placeholder: "Directory containing .eml files",
  },
});
