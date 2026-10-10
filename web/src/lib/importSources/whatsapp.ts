import { attachmentChoicesOf } from "../../screens/import/attachmentChoices";
import { sourceLabel } from "../exportSources";
import { whatsappExtractFields } from "../whatsappExtractFields";
import {
  isWhatsappMethod,
  WHATSAPP_DEFAULT_METHOD,
  WHATSAPP_METHODS,
  WHATSAPP_SOURCE_ID,
  type WhatsappMethodId,
  whatsappCanImport,
  whatsappOwnerPhoneRequired,
  whatsappShowsKey,
  whatsappShowsPassword,
} from "../whatsappImport";
import type { ImportSourceDescriptor } from "./types";
import WhatsappFormSection from "./WhatsappFormSection";

const WHATSAPP_DIRECTORY_HINT_ANDROID =
  "Directory that contains msgstore.db or msgstore.db.crypt12 / crypt14 / crypt15.";
const WHATSAPP_DIRECTORY_HINT_IPHONE = "Path to the root of a device backup";
/** The method as its own type; a descriptor is only asked about its own methods. */
function whatsappMethod(method: string): WhatsappMethodId {
  if (isWhatsappMethod(method)) return method;
  throw new Error(`${method} is not a WhatsApp method`);
}

/** WhatsApp, read from an Android backup directory or an iPhone backup. */
export const WHATSAPP_SOURCE: ImportSourceDescriptor = {
  id: WHATSAPP_SOURCE_ID,
  label: sourceLabel(WHATSAPP_SOURCE_ID),
  methods: WHATSAPP_METHODS,
  defaultMethod: WHATSAPP_DEFAULT_METHOD,
  visibleMethods: () => WHATSAPP_METHODS,
  showsAttachmentOptions: true,
  asksOwnerPhones: false,
  asksOwnerEmails: false,
  needsWtsexporter: true,
  processingOptions: (method) =>
    whatsappOwnerPhoneRequired(whatsappMethod(method)) ? [] : ["whatsappFallbackPhone"],
  extractFields: (form) =>
    whatsappExtractFields({
      source: whatsappMethod(form.source),
      ...attachmentChoicesOf(form),
      key: form.whatsappKey,
      backupPassword: form.backupPassword,
      wa: form.whatsappWa,
      media: form.whatsappMedia,
      db: form.whatsappDb,
      business: form.whatsappBusiness,
      ownerPhone: form.whatsappOwnerPhone,
    }),
  // The iPhone backup password, or the Android backup's key.
  snapshotSecret: (method) => {
    const m = whatsappMethod(method);
    if (whatsappShowsPassword(m)) return "backupPassword";
    return whatsappShowsKey(m) ? "whatsappKey" : null;
  },
  backupField: (method) => ({
    label: "Backup directory",
    directory: true,
    hint:
      whatsappMethod(method) === "whatsapp-ios"
        ? WHATSAPP_DIRECTORY_HINT_IPHONE
        : WHATSAPP_DIRECTORY_HINT_ANDROID,
  }),
  readiness: (input) =>
    whatsappCanImport({
      method: whatsappMethod(input.source),
      backupPath: input.backupPath,
      key: input.whatsappKey,
      backupPassword: input.backupPassword,
      contactsDb: input.whatsappWa,
      media: input.whatsappMedia,
      db: input.whatsappDb,
      ownerPhone: input.whatsappOwnerPhone,
      stats: input.whatsappStats,
    }),
  FormSection: WhatsappFormSection,
};
