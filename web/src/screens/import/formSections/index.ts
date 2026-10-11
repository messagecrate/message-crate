import type { ComponentType } from "react";
import type { ImportSourceId } from "../../../lib/exportSources";
import AndroidSmsFormSection from "./AndroidSmsFormSection";
import ImessageFormSection from "./ImessageFormSection";
import type { ImportFormSectionProps } from "./types";
import WhatsappFormSection from "./WhatsappFormSection";

/**
 * Each source's fields after its backup field, in the Import Messages
 * section, by the source's id; null for a source whose form is the backup
 * path alone. Typed by `EXPORT_SOURCES`, so a source listed there without an
 * entry here does not build.
 */
export const IMPORT_FORM_SECTIONS: Record<
  ImportSourceId,
  ComponentType<ImportFormSectionProps> | null
> = {
  imessage: ImessageFormSection,
  whatsapp: WhatsappFormSection,
  "sms-backup-restore": AndroidSmsFormSection,
  "go-sms-pro": AndroidSmsFormSection,
  imazing: null,
  "sms-backup-plus": AndroidSmsFormSection,
  openextract: null,
};
