import { IMESSAGE_SOURCE_ID } from "./imessageImport";
import { WHATSAPP_SOURCE_ID } from "./whatsappImport";

/** The iMazing CSV export, whose dates carry no zone. */
export const IMAZING_SOURCE_ID = "imazing";

/**
 * Backup sources offered by Import in the desktop app, by id and name, in
 * the order Import lists them.
 *
 * Everything else the Import screen knows about a source is in its
 * descriptor in `importSources/`, which takes its name from here. The list
 * stays apart from the descriptors so the screens that only name a source,
 * such as a message's bubble and the search form, do not load the Import
 * form with them. `IMPORT_SOURCES` is typed to hold a descriptor for each id
 * here, so a source added without one does not build.
 */
export const EXPORT_SOURCES = [
  { id: IMESSAGE_SOURCE_ID, label: "Apple Messages" },
  { id: WHATSAPP_SOURCE_ID, label: "WhatsApp" },
  { id: "sms-backup-restore", label: "SMS Backup & Restore" },
  { id: "go-sms-pro", label: "GO SMS Pro" },
  { id: IMAZING_SOURCE_ID, label: "iMazing" },
  { id: "sms-backup-plus", label: "SMS Backup+" },
  { id: "openextract", label: "OpenExtract" },
] as const;

/** The id of a source Import offers. */
export type ImportSourceId = (typeof EXPORT_SOURCES)[number]["id"];

/**
 * Sources the conversation draws a bubble for that Import does not offer, by
 * the name the product gives them. A source moves to `EXPORT_SOURCES` once
 * Import offers it, so each source is named in one place.
 */
const OTHER_SOURCE_LABELS: Record<string, string> = {
  discord: "Discord",
  instagram: "Instagram",
};

/**
 * The name the product gives a message's import source: "Apple Messages" for
 * `imessage`, as Import lists it. A source the product does not know keeps its id.
 */
export function sourceLabel(source: string): string {
  return (
    EXPORT_SOURCES.find((s) => s.id === source)?.label ?? OTHER_SOURCE_LABELS[source] ?? source
  );
}
