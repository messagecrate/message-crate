import { attachmentChoicesOf } from "../attachmentChoices";
import { sourceLabel } from "../exportSources";
import { imessageExtractFields } from "../imessageExtractFields";
import {
  IMESSAGE_DEFAULT_METHOD,
  IMESSAGE_METHODS,
  IMESSAGE_SOURCE_ID,
  type ImessageMethodId,
  imessageCanImport,
  imessageShowsPassword,
  imessageVisiblePlatforms,
} from "../imessageImport";
import type { BackupField, ImportSourceDescriptor } from "./types";

const SQLITE_DB_FILTERS = [{ name: "SQLite database", extensions: ["db"] }];
/** An iPhone backup is a directory; Mac Messages and a jailbroken iPhone give one database. */
function imessageBackupField(method: ImessageMethodId): BackupField {
  if (method === "imessage-ios") {
    return {
      label: "iPhone Backup Directory",
      directory: true,
      placeholder: "Path to the root of a device backup",
    };
  }
  return {
    label: "Messages database",
    directory: false,
    filters: SQLITE_DB_FILTERS,
    placeholder: method === "imessage-macos" ? "Path to chat.db" : "Path to sms.db",
  };
}

/** Apple Messages, read from an iPhone backup, a Mac, or a jailbroken iPhone. */
export const IMESSAGE_SOURCE: ImportSourceDescriptor<ImessageMethodId> = {
  id: IMESSAGE_SOURCE_ID,
  label: sourceLabel(IMESSAGE_SOURCE_ID),
  methods: IMESSAGE_METHODS,
  defaultMethod: IMESSAGE_DEFAULT_METHOD,
  visibleMethods: (selected) => imessageVisiblePlatforms(selected),
  showsAttachmentOptions: true,
  asksOwnerPhones: false,
  asksOwnerEmails: false,
  asksWhatsappOwnerPhone: false,
  rememberedPaths: ["attachmentRoot", "appleContacts"],
  needsWtsexporter: false,
  // Only an iPhone backup can be obfuscated.
  processingOptions: (method) => (method === "imessage-ios" ? ["obfuscate"] : []),
  extractFields: (form) =>
    imessageExtractFields({
      source: form.source,
      backupPassword: form.backupPassword,
      ...attachmentChoicesOf(form),
      obfuscate: form.obfuscate,
      attachmentRoot: form.attachmentRoot,
      appleContacts: form.appleContacts,
    }),
  appleIdentityRead: (method) => ({ ios: method === "imessage-ios" }),
  // The iPhone backup password; Mac Messages and a jailbroken iPhone read none.
  snapshotSecret: (method) => (imessageShowsPassword(method) ? "backupPassword" : null),
  backupField: imessageBackupField,
  readiness: (input) =>
    imessageCanImport({
      method: input.source,
      backupPath: input.backupPath,
      attachmentRoot: input.attachmentRoot,
      appleContacts: input.appleContacts,
      backupPassword: input.backupPassword,
      stats: input.pathStats,
    }),
};
