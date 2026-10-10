import { attachmentChoicesOf } from "../../screens/import/attachmentChoices";
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
  isImessageMethod,
} from "../imessageImport";
import ImessageFormSection from "./ImessageFormSection";
import type { BackupField, ImportSourceDescriptor } from "./types";

const SQLITE_DB_FILTERS = [{ name: "SQLite database", extensions: ["db"] }];
/** The method as its own type; a descriptor is only asked about its own methods. */
function imessageMethod(method: string): ImessageMethodId {
  if (isImessageMethod(method)) return method;
  throw new Error(`${method} is not an Apple Messages method`);
}

/** An iPhone backup is a directory; Mac Messages and a jailbroken iPhone give one database. */
function imessageBackupField(method: string): BackupField {
  const m = imessageMethod(method);
  if (m === "imessage-ios") {
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
    placeholder: m === "imessage-macos" ? "Path to chat.db" : "Path to sms.db",
  };
}

/** Apple Messages, read from an iPhone backup, a Mac, or a jailbroken iPhone. */
export const IMESSAGE_SOURCE: ImportSourceDescriptor = {
  id: IMESSAGE_SOURCE_ID,
  label: sourceLabel(IMESSAGE_SOURCE_ID),
  methods: IMESSAGE_METHODS,
  defaultMethod: IMESSAGE_DEFAULT_METHOD,
  visibleMethods: (selected) => imessageVisiblePlatforms(imessageMethod(selected)),
  showsAttachmentOptions: true,
  asksOwnerPhones: false,
  asksOwnerEmails: false,
  needsWtsexporter: false,
  // Only an iPhone backup can be obfuscated.
  processingOptions: (method) => (imessageMethod(method) === "imessage-ios" ? ["obfuscate"] : []),
  extractFields: (form) =>
    imessageExtractFields({
      source: imessageMethod(form.source),
      backupPassword: form.backupPassword,
      ...attachmentChoicesOf(form),
      obfuscate: form.obfuscate,
      attachmentRoot: form.attachmentRoot,
      appleContacts: form.appleContacts,
    }),
  // The iPhone backup password; Mac Messages and a jailbroken iPhone read none.
  snapshotSecret: (method) =>
    imessageShowsPassword(imessageMethod(method)) ? "backupPassword" : null,
  backupField: imessageBackupField,
  readiness: (input) =>
    imessageCanImport({
      method: imessageMethod(input.source),
      backupPath: input.backupPath,
      attachmentRoot: input.attachmentRoot,
      appleContacts: input.appleContacts,
      backupPassword: input.backupPassword,
      stats: input.pathStats,
    }),
  FormSection: ImessageFormSection,
};
