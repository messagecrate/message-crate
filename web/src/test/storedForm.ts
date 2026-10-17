/**
 * A stored form snapshot, as `formSnapshot` writes it on an Import Run, for
 * tests that resume or restore a run. Typed as `StoredFormSnapshot`, so a
 * field added to the snapshot fails to compile here until it is given a
 * value. Written out rather than built with `formSnapshot`, so a test of
 * reading a snapshot back does not depend on the code that writes one.
 */

import type { StoredFormSnapshot } from "../screens/import/formSnapshot";
import { MIB } from "./units";

/** The stored snapshot of an iPhone backup Import, with `overrides` on top. */
export function storedForm(overrides: Partial<StoredFormSnapshot> = {}): StoredFormSnapshot {
  return {
    source: "imessage-ios",
    backupPath: "/backups/iphone.tar",
    attachmentMedia: "copy",
    maxResolution: "720p",
    maxFps: "30",
    minSizeMb: "20",
    ownerPhones: [],
    ownerEmails: [],
    obfuscate: false,
    attachmentRoot: "",
    appleContacts: "",
    whatsappWa: "",
    whatsappMedia: "",
    whatsappDb: "",
    isBusinessApp: false,
    whatsappOwnerPhone: "",
    timeZone: "America/New_York",
    phoneCountry: "",
    backupPasswordGiven: false,
    whatsappKeyGiven: false,
    assetMaxBytes: 512 * MIB,
    ...overrides,
  };
}
