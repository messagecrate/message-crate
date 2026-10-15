import { describe, expect, it } from "vitest";
import type { ImportJobFormValues } from "../../lib/importSources/types";
import {
  type ImportFormState,
  importFormReducer,
  importJobValues,
  newImportForm,
  type SourcePaths,
} from "./importForm";

/** A run's form with every field away from a new form's value. */
const STORED: ImportJobFormValues = {
  source: "whatsapp-ios",
  backupPath: "/backups/iphone",
  backupPassword: "",
  attachmentMedia: "compress",
  maxResolution: "1080p",
  maxFps: "24",
  minSizeMb: "50",
  ownerPhones: ["+15555550100", "+15555550101"],
  ownerEmails: ["me@example.com", "me@work.example"],
  obfuscate: true,
  timeZone: "Europe/London",
  phoneCountry: "GB",
  attachmentRoot: "/backups/attachments",
  appleContacts: "/backups/AddressBook.sqlitedb",
  whatsappKey: "",
  whatsappWa: "/backups/wa.db",
  whatsappMedia: "/backups/Media",
  whatsappDb: "/backups/msgstore.db",
  isBusinessApp: true,
  whatsappOwnerPhone: "+15555550102",
};

const NO_PATHS: SourcePaths = {
  backupPath: "",
  attachmentRoot: "",
  appleContacts: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
};

function withSecretsTypedAndSectionsOpen(form: ImportFormState): ImportFormState {
  return {
    ...form,
    backupPassword: "typed password",
    whatsappKey: "typed key",
    showBackupPassword: true,
    processingOpen: true,
  };
}

describe("importFormReducer", () => {
  it("refills every stored field, so the run starts again with the form it had", () => {
    const refilled = importFormReducer(newImportForm(""), { type: "refill", form: STORED });
    expect(importJobValues(refilled, "America/New_York", refilled.ownerPhones)).toEqual(STORED);
  });

  // The secrets are never stored with a run, and a typed one must not carry
  // over into a resumed run's form.
  it("clears the secrets and keeps how the form is shown when it refills", () => {
    const refilled = importFormReducer(withSecretsTypedAndSectionsOpen(newImportForm("")), {
      type: "refill",
      form: STORED,
    });
    expect(refilled.backupPassword).toBe("");
    expect(refilled.whatsappKey).toBe("");
    expect(refilled.showBackupPassword).toBe(true);
    expect(refilled.processingOpen).toBe(true);
  });

  it("keeps a refilled owner's numbers when the profile's arrive after", () => {
    const refilled = importFormReducer(newImportForm(""), {
      type: "refill",
      form: { ...STORED, ownerPhones: [], ownerEmails: [], whatsappOwnerPhone: "" },
    });
    const seeded = importFormReducer(refilled, {
      type: "seed",
      ownerPhones: ["+15555550199"],
      ownerEmails: "profile@example.com",
      whatsappOwnerPhone: "+15555550199",
    });
    expect(seeded.ownerPhones).toEqual([]);
    expect(seeded.ownerEmails).toBe("");
    expect(seeded.whatsappOwnerPhone).toBe("");
  });

  it("seeds the empty owner fields from the profile once", () => {
    const seeded = importFormReducer(newImportForm(""), {
      type: "seed",
      ownerPhones: ["+15555550100"],
      ownerEmails: "me@example.com",
    });
    expect(seeded.ownerPhones).toEqual(["+15555550100"]);
    expect(seeded.ownerEmails).toBe("me@example.com");

    // The person empties the list; a second read of the profile leaves it empty.
    const emptied = importFormReducer(seeded, { type: "set", field: "ownerPhones", value: [] });
    const again = importFormReducer(emptied, { type: "seed", ownerPhones: ["+15555550100"] });
    expect(again.ownerPhones).toEqual([]);
  });

  it("never overwrites a number the person typed before the profile arrived", () => {
    const typedNumber = importFormReducer(newImportForm(""), {
      type: "set",
      field: "whatsappOwnerPhone",
      value: "+15555550111",
    });
    const seeded = importFormReducer(typedNumber, {
      type: "seed",
      whatsappOwnerPhone: "+15555550100",
    });
    expect(seeded.whatsappOwnerPhone).toBe("+15555550111");
  });

  it("seeds again after a source without owner fields was picked", () => {
    const seeded = importFormReducer(newImportForm(""), {
      type: "seed",
      ownerPhones: ["+15555550100"],
    });
    const emptied = importFormReducer(seeded, { type: "set", field: "ownerPhones", value: [] });
    const unseeded = importFormReducer(emptied, { type: "unseed" });
    const again = importFormReducer(unseeded, { type: "seed", ownerPhones: ["+15555550100"] });
    expect(again.ownerPhones).toEqual(["+15555550100"]);
  });

  it("picks a source with its remembered paths, and drops WhatsApp Business off an iPhone backup", () => {
    const business = { ...newImportForm(""), source: "whatsapp-ios", isBusinessApp: true };
    const paths = {
      ...NO_PATHS,
      backupPath: "/backups/android",
      whatsappDb: "/backups/msgstore.db",
    };
    const android = importFormReducer(business, {
      type: "source",
      source: "whatsapp-android",
      paths,
    });
    expect(android.source).toBe("whatsapp-android");
    expect(android.backupPath).toBe("/backups/android");
    expect(android.whatsappDb).toBe("/backups/msgstore.db");
    expect(android.isBusinessApp).toBe(false);

    const iphone = importFormReducer(business, {
      type: "source",
      source: "whatsapp-ios",
      paths: NO_PATHS,
    });
    expect(iphone.isBusinessApp).toBe(true);
  });
});

describe("importJobValues", () => {
  it("reads iMazing dates in the account's zone until another is picked", () => {
    const form = newImportForm("");
    expect(importJobValues(form, "America/New_York", []).timeZone).toBe("America/New_York");
    const picked = importFormReducer(form, { type: "set", field: "timeZone", value: "Asia/Tokyo" });
    expect(importJobValues(picked, "America/New_York", []).timeZone).toBe("Asia/Tokyo");
  });

  it("splits the typed email addresses into a list", () => {
    const form = { ...newImportForm(""), ownerEmails: "me@example.com, me@work.example" };
    expect(importJobValues(form, "UTC", []).ownerEmails).toEqual([
      "me@example.com",
      "me@work.example",
    ]);
  });
});
