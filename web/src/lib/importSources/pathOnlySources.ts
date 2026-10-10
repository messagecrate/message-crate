import { IMAZING_SOURCE_ID, type ImportSourceId, sourceLabel } from "../exportSources";
import type { BackupField, ImportSourceDescriptor } from "./types";

export const OPENEXTRACT_SOURCE_ID = "openextract";

const BACKUP_PATH_FIELD: BackupField = { label: "Backup path", directory: true };

/**
 * A source whose form is the backup path alone: no Attachments field, so
 * its run copies attachments, and nothing else of its own to fill in.
 */
function pathOnlySource(
  id: ImportSourceId,
  rest: Pick<ImportSourceDescriptor, "processingOptions" | "extractFields">,
): ImportSourceDescriptor {
  const label = sourceLabel(id);
  const method = { id, label };
  return {
    id,
    label,
    methods: [method],
    defaultMethod: id,
    visibleMethods: () => [method],
    showsAttachmentOptions: false,
    asksOwnerPhones: false,
    asksOwnerEmails: false,
    asksWhatsappOwnerPhone: false,
    rememberedPaths: [],
    needsWtsexporter: false,
    appleIdentityRead: () => null,
    snapshotSecret: () => null,
    backupField: () => BACKUP_PATH_FIELD,
    readiness: (input) => ({ enabled: Boolean(input.backupPath), errors: {} }),
    FormSection: () => null,
    ...rest,
  };
}

/** iMazing's CSV export. Its dates carry no zone, so the form asks which one to read them in. */
export const IMAZING_SOURCE = pathOnlySource(IMAZING_SOURCE_ID, {
  processingOptions: () => ["timeZone"],
  extractFields: (form) => ({ timezone: form.timeZone }),
});

/** OpenExtract's export. */
export const OPENEXTRACT_SOURCE = pathOnlySource(OPENEXTRACT_SOURCE_ID, {
  processingOptions: () => [],
  extractFields: () => ({}),
});
