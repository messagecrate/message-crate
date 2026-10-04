import { IMESSAGE_SOURCE_ID } from "./imessageImport";
import { WHATSAPP_SOURCE_ID } from "./whatsappImport";

/** The iMazing CSV export, whose dates carry no zone. */
export const IMAZING_SOURCE_ID = "imazing";

/** Backup sources offered by Import in the desktop app. */
export const EXPORT_SOURCES: { id: string; label: string }[] = [
  { id: IMESSAGE_SOURCE_ID, label: "Apple Messages" },
  { id: WHATSAPP_SOURCE_ID, label: "WhatsApp" },
  { id: "sms-backup-restore", label: "SMS Backup & Restore" },
  { id: "go-sms-pro", label: "GO SMS Pro" },
  { id: IMAZING_SOURCE_ID, label: "iMazing" },
  { id: "sms-backup-plus", label: "SMS Backup+" },
  { id: "openextract", label: "OpenExtract" },
];

/**
 * Sources the conversation draws a bubble for that Import does not offer, by
 * the name the product gives them.
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
