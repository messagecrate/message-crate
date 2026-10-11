import { describe, expect, it } from "vitest";
import type { ImportJobFormValues } from "../../lib/importSources/types";
import { formSnapshot, restoreFormFromSnapshot } from "./formSnapshot";

/** Every form field filled with a value no other field holds. */
const markedForm: ImportJobFormValues = {
  source: "imessage-ios",
  backupPath: "marker-backupPath",
  backupPassword: "marker-backupPassword",
  attachmentMedia: "compress",
  maxResolution: "marker-maxResolution",
  maxFps: "marker-maxFps",
  minSizeMb: "marker-minSizeMb",
  ownerPhones: ["marker-ownerPhones"],
  ownerEmails: ["marker-ownerEmails"],
  obfuscate: true,
  timeZone: "marker-timeZone",
  phoneCountry: "marker-phoneCountry",
  attachmentRoot: "marker-attachmentRoot",
  appleContacts: "marker-appleContacts",
  whatsappKey: "marker-whatsappKey",
  whatsappWa: "marker-whatsappWa",
  whatsappMedia: "marker-whatsappMedia",
  whatsappDb: "marker-whatsappDb",
  isBusinessApp: true,
  whatsappOwnerPhone: "marker-whatsappOwnerPhone",
  assetMaxBytes: 12345,
};

describe("formSnapshot", () => {
  it("stores only the fields restoreFormFromSnapshot reads, and neither secret", () => {
    const snapshot = formSnapshot(markedForm);

    expect(Object.keys(snapshot).sort()).toEqual(
      [
        "source",
        "backupPath",
        "attachmentMedia",
        "maxResolution",
        "maxFps",
        "minSizeMb",
        "ownerPhones",
        "ownerEmails",
        "obfuscate",
        "timeZone",
        "phoneCountry",
        "attachmentRoot",
        "appleContacts",
        "whatsappWa",
        "whatsappMedia",
        "whatsappDb",
        "isBusinessApp",
        "whatsappOwnerPhone",
        "assetMaxBytes",
        "backupPasswordGiven",
        "whatsappKeyGiven",
      ].sort(),
    );
    const stored = JSON.stringify(snapshot);
    expect(stored).not.toContain("marker-backupPassword");
    expect(stored).not.toContain("marker-whatsappKey");
  });

  it("leaves out a field it does not list, so a new secret is not stored by default", () => {
    const withNewField = {
      ...markedForm,
      keyFilePassphrase: "marker-keyFilePassphrase",
    } as ImportJobFormValues;

    const snapshot = formSnapshot(withNewField);

    expect(snapshot).not.toHaveProperty("keyFilePassphrase");
    expect(JSON.stringify(snapshot)).not.toContain("marker-keyFilePassphrase");
  });

  it("gives back every field but the secrets when the snapshot is read back", () => {
    const stored = JSON.parse(JSON.stringify(formSnapshot(markedForm)));

    expect(restoreFormFromSnapshot(stored)).toEqual({
      ...markedForm,
      backupPassword: "",
      whatsappKey: "",
    });
  });
});
