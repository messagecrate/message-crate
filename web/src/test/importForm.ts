/**
 * The Import form's values for an iPhone backup, for tests that start or
 * resume an Import Run. Typed as `ImportJobFormValues`, so a field added to
 * the form fails to compile here until it is given a value, rather than
 * leaving a copy in one test file behind.
 */

import type { ImportJobFormValues } from "../lib/importSources/types";
import { MIB } from "./units";

/** An iPhone backup Import with attachments copied, with `overrides` on top. */
export function importForm(overrides: Partial<ImportJobFormValues> = {}): ImportJobFormValues {
  return {
    source: "imessage-ios",
    backupPath: "/backups/iphone.tar",
    backupPassword: "",
    attachmentMedia: "copy",
    maxResolution: "",
    maxFps: "",
    minSizeMb: "",
    ownerPhones: [],
    ownerEmails: [],
    obfuscate: false,
    attachmentRoot: "",
    appleContacts: "",
    whatsappKey: "",
    whatsappWa: "",
    whatsappMedia: "",
    whatsappDb: "",
    isBusinessApp: false,
    whatsappOwnerPhone: "",
    timeZone: "America/New_York",
    phoneCountry: "",
    // What a resumed run reads back from its stored form. A new run reads the
    // server's limit instead and replaces this.
    assetMaxBytes: 512 * MIB,
    ...overrides,
  };
}
