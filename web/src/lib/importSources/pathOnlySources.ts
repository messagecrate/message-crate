import { IMAZING_SOURCE_ID, type ImportSourceId, sourceLabel } from "../exportSources";
import { PATH_MISSING, unknownPathMessage } from "../pathChecks";
import type { BackupField, ImportSourceDescriptor, ReadinessInput } from "./types";

export const OPENEXTRACT_SOURCE_ID = "openextract";

/** True for iMazing and OpenExtract, whose form is the backup path alone. */
export function isPathOnlySource(source: string): boolean {
  return source === IMAZING_SOURCE_ID || source === OPENEXTRACT_SOURCE_ID;
}

/**
 * Import can start once the backup path is checked and is there. The
 * exporter reads one CSV file or every one under a directory, so either
 * kind is taken. A path not checked yet keeps Import off with nothing said,
 * as an empty one does, and a missing one says so under the field.
 */
function pathOnlyReadiness(input: ReadinessInput): {
  enabled: boolean;
  errors: Partial<Record<"backupPath", string>>;
} {
  const errors: Partial<Record<"backupPath", string>> = {};
  const stat = input.backupPathStats.backup;
  if (input.backupPath.trim() === "" || stat === null) return { enabled: false, errors };
  if (stat.unreadable) errors.backupPath = unknownPathMessage(stat.unreadable);
  else if (!stat.exists) errors.backupPath = PATH_MISSING;
  return { enabled: errors.backupPath === undefined, errors };
}

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
    readiness: pathOnlyReadiness,
    backupEncrypted: () => null,
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
