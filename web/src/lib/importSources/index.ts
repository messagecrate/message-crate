import { EXPORT_SOURCES, type ImportSourceId } from "../exportSources";
import { GO_SMS_PRO_SOURCE, SMS_BACKUP_PLUS_SOURCE, SMS_BACKUP_RESTORE_SOURCE } from "./androidSms";
import { IMESSAGE_SOURCE } from "./imessage";
import { IMAZING_SOURCE, OPENEXTRACT_SOURCE } from "./pathOnlySources";
import type { ImportSourceDescriptor } from "./types";
import { WHATSAPP_SOURCE } from "./whatsapp";

/**
 * Each source's descriptor by its id. Typed by `EXPORT_SOURCES`, so a source
 * listed there without a descriptor here, or one here that is not listed,
 * does not build.
 */
const DESCRIPTORS: Record<ImportSourceId, ImportSourceDescriptor> = {
  imessage: IMESSAGE_SOURCE,
  whatsapp: WHATSAPP_SOURCE,
  "sms-backup-restore": SMS_BACKUP_RESTORE_SOURCE,
  "go-sms-pro": GO_SMS_PRO_SOURCE,
  imazing: IMAZING_SOURCE,
  "sms-backup-plus": SMS_BACKUP_PLUS_SOURCE,
  openextract: OPENEXTRACT_SOURCE,
};

/** Every backup source Import offers, in the order the source list shows them. */
export const IMPORT_SOURCES: readonly ImportSourceDescriptor[] = EXPORT_SOURCES.map(
  (s) => DESCRIPTORS[s.id],
);

/** The source a method belongs to, or undefined for an id no source has. */
export function findImportSource(method: string): ImportSourceDescriptor | undefined {
  return IMPORT_SOURCES.find((s) => s.methods.some((m) => m.id === method));
}

/** True for the id of a method some source in `IMPORT_SOURCES` has. */
export function isImportMethod(method: string): boolean {
  return findImportSource(method) !== undefined;
}

/**
 * The source a method belongs to. The form only ever holds a method from
 * `IMPORT_SOURCES`, and a stored form with any other is refused when it is
 * read back, so an unknown id here is a bug and throws.
 */
export function importSourceFor(method: string): ImportSourceDescriptor {
  const source = findImportSource(method);
  if (source === undefined) throw new Error(`No import source has the method ${method}`);
  return source;
}

/** The source with this id (`imessage`, not a method such as `imessage-ios`), if Import offers it. */
export function importSourceById(id: string): ImportSourceDescriptor | undefined {
  return IMPORT_SOURCES.find((s) => s.id === id);
}
