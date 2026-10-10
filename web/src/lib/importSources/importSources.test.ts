import { describe, expect, it } from "vitest";
import type { ImportJobFormValues } from "../../screens/import/useImportJob";
import { EXPORT_SOURCES, IMAZING_SOURCE_ID } from "../exportSources";
import { IMESSAGE_METHODS } from "../imessageImport";
import { WHATSAPP_METHODS } from "../whatsappImport";
import {
  GO_SMS_PRO_SOURCE_ID,
  SMS_BACKUP_PLUS_SOURCE_ID,
  SMS_BACKUP_RESTORE_SOURCE_ID,
} from "./androidSms";
import { findImportSource, IMPORT_SOURCES, importSourceFor, isImportMethod } from "./index";
import { OPENEXTRACT_SOURCE_ID } from "./pathOnlySources";

const form: ImportJobFormValues = {
  source: "",
  backupPath: "/backups/phone",
  backupPassword: "secret",
  attachmentMedia: "compress",
  maxResolution: "720p",
  maxFps: "30",
  minSizeMb: "20",
  ownerPhones: ["+15555550100"],
  ownerEmails: ["me@example.com"],
  obfuscate: true,
  timeZone: "Europe/London",
  phoneCountry: "",
  attachmentRoot: "",
  appleContacts: "",
  whatsappKey: "abc123",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
  isBusinessApp: false,
  whatsappOwnerPhone: "",
};

const allMethods = IMPORT_SOURCES.flatMap((s) => s.methods.map((m) => m.id));

describe("IMPORT_SOURCES", () => {
  // A source is added by adding its descriptor here, so each one must answer
  // everything the Import form, the run, and the snapshot ask of it.
  it("gives every source each field the Import form reads, for each of its methods", () => {
    for (const source of IMPORT_SOURCES) {
      expect(source.label, source.id).not.toBe("");
      expect(source.methods.length, source.id).toBeGreaterThan(0);
      expect(source.methods.map((m) => m.id)).toContain(source.defaultMethod);
      expect(typeof source.showsAttachmentOptions, source.id).toBe("boolean");
      expect(typeof source.asksOwnerPhones, source.id).toBe("boolean");
      expect(typeof source.asksOwnerEmails, source.id).toBe("boolean");
      expect(typeof source.needsWtsexporter, source.id).toBe("boolean");
      expect(typeof source.readiness, source.id).toBe("function");
      expect(typeof source.FormSection, source.id).toBe("function");
      for (const { id: method } of source.methods) {
        const backup = source.backupField(method);
        expect(backup.label, method).not.toBe("");
        expect(typeof backup.directory, method).toBe("boolean");
        expect(
          source.visibleMethods(method).map((m) => m.id),
          method,
        ).toContain(method);
        expect(Array.isArray(source.processingOptions(method)), method).toBe(true);
        expect([null, "backupPassword", "whatsappKey"], method).toContain(
          source.snapshotSecret(method),
        );
        expect(typeof source.extractFields({ ...form, source: method }), method).toBe("object");
      }
    }
  });

  it("names each source and each method once", () => {
    const ids = IMPORT_SOURCES.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(new Set(allMethods).size).toBe(allMethods.length);
  });

  it("is the list Import offers, in its order", () => {
    expect(EXPORT_SOURCES).toEqual(IMPORT_SOURCES.map(({ id, label }) => ({ id, label })));
  });
});

describe("importSourceFor", () => {
  it("finds the source of each method", () => {
    for (const source of IMPORT_SOURCES) {
      for (const method of source.methods) {
        expect(importSourceFor(method.id)).toBe(source);
        expect(isImportMethod(method.id)).toBe(true);
      }
    }
  });

  it("finds Apple Messages and WhatsApp by their methods, not by their source ids", () => {
    expect(importSourceFor("imessage-macos").id).toBe("imessage");
    expect(importSourceFor("whatsapp-ios").id).toBe("whatsapp");
    expect(findImportSource("imessage")).toBeUndefined();
    expect(findImportSource("whatsapp")).toBeUndefined();
  });

  // Before the descriptors, an unknown source fell through to SMS Backup &
  // Restore's backup field without a word.
  it("throws for a method no source has", () => {
    expect(isImportMethod("telegram")).toBe(false);
    expect(() => importSourceFor("telegram")).toThrow("telegram");
  });
});

describe("showsAttachmentOptions", () => {
  it("is true for every source whose form has the Attachments field", () => {
    const methods = [
      ...IMESSAGE_METHODS.map((m) => m.id),
      ...WHATSAPP_METHODS.map((m) => m.id),
      SMS_BACKUP_RESTORE_SOURCE_ID,
      GO_SMS_PRO_SOURCE_ID,
      SMS_BACKUP_PLUS_SOURCE_ID,
    ];
    for (const method of methods) {
      expect(importSourceFor(method).showsAttachmentOptions, method).toBe(true);
    }
  });

  it("is false for iMazing and OpenExtract, whose forms have none", () => {
    expect(importSourceFor(IMAZING_SOURCE_ID).showsAttachmentOptions).toBe(false);
    expect(importSourceFor(OPENEXTRACT_SOURCE_ID).showsAttachmentOptions).toBe(false);
  });
});

describe("backupField", () => {
  // An Import Run reads one backup, and SMS Backup & Restore writes each
  // backup as one file: a directory picker would let a run take several.
  it("asks SMS Backup & Restore for one .xml file", () => {
    const field = importSourceFor(SMS_BACKUP_RESTORE_SOURCE_ID).backupField(
      SMS_BACKUP_RESTORE_SOURCE_ID,
    );
    expect(field.directory).toBe(false);
    expect(field.filters).toEqual([{ name: "SMS Backup & Restore", extensions: ["xml"] }]);
  });

  it("asks GO SMS Pro and SMS Backup+ for a directory", () => {
    for (const source of [GO_SMS_PRO_SOURCE_ID, SMS_BACKUP_PLUS_SOURCE_ID]) {
      const field = importSourceFor(source).backupField(source);
      expect(field.directory, source).toBe(true);
      expect(field.filters, source).toBeUndefined();
    }
  });

  it("asks an iPhone backup for a directory and Mac Messages for its database file", () => {
    expect(importSourceFor("imessage-ios").backupField("imessage-ios").directory).toBe(true);
    const mac = importSourceFor("imessage-macos").backupField("imessage-macos");
    expect(mac.directory).toBe(false);
    expect(mac.placeholder).toBe("Path to chat.db");
  });
});

describe("snapshotSecret", () => {
  it("names the secret each method's extract reads, and none for the rest", () => {
    const expected: Record<string, string | null> = {
      "imessage-ios": "backupPassword",
      "imessage-macos": null,
      "imessage-jailbreak": null,
      "whatsapp-android": "whatsappKey",
      "whatsapp-ios": "backupPassword",
      [SMS_BACKUP_RESTORE_SOURCE_ID]: null,
      [GO_SMS_PRO_SOURCE_ID]: null,
      [IMAZING_SOURCE_ID]: null,
      [SMS_BACKUP_PLUS_SOURCE_ID]: null,
      [OPENEXTRACT_SOURCE_ID]: null,
    };
    expect(Object.keys(expected).sort()).toEqual([...allMethods].sort());
    for (const method of allMethods) {
      expect(importSourceFor(method).snapshotSecret(method), method).toBe(expected[method]);
    }
  });
});

describe("rememberedPaths", () => {
  // A path not listed for a source is cleared when the source is picked, and never stored for it.
  it("names the paths beside the backup that each source keeps", () => {
    const expected: Record<string, readonly string[]> = {
      imessage: ["attachmentRoot", "appleContacts"],
      whatsapp: ["whatsappWa", "whatsappMedia", "whatsappDb"],
      [SMS_BACKUP_RESTORE_SOURCE_ID]: [],
      [GO_SMS_PRO_SOURCE_ID]: [],
      [IMAZING_SOURCE_ID]: [],
      [SMS_BACKUP_PLUS_SOURCE_ID]: [],
      [OPENEXTRACT_SOURCE_ID]: [],
    };
    expect(Object.keys(expected).sort()).toEqual(IMPORT_SOURCES.map((s) => s.id).sort());
    for (const source of IMPORT_SOURCES) {
      expect(source.rememberedPaths, source.id).toEqual(expected[source.id]);
    }
  });
});

describe("appleIdentityRead", () => {
  it("reads the identities of an Apple Messages backup only, as an iPhone backup for the iPhone method", () => {
    const expected: Record<string, { ios: boolean } | null> = {
      "imessage-ios": { ios: true },
      "imessage-macos": { ios: false },
      "imessage-jailbreak": { ios: false },
      "whatsapp-android": null,
      "whatsapp-ios": null,
      [SMS_BACKUP_RESTORE_SOURCE_ID]: null,
      [GO_SMS_PRO_SOURCE_ID]: null,
      [IMAZING_SOURCE_ID]: null,
      [SMS_BACKUP_PLUS_SOURCE_ID]: null,
      [OPENEXTRACT_SOURCE_ID]: null,
    };
    expect(Object.keys(expected).sort()).toEqual([...allMethods].sort());
    for (const method of allMethods) {
      expect(importSourceFor(method).appleIdentityRead(method), method).toEqual(expected[method]);
    }
  });
});

describe("extractFields", () => {
  it("sends iMazing only the time zone its dates are read in", () => {
    const source = IMAZING_SOURCE_ID;
    expect(importSourceFor(source).extractFields({ ...form, source })).toEqual({
      timezone: "Europe/London",
    });
  });

  it("sends OpenExtract nothing of its own", () => {
    const source = OPENEXTRACT_SOURCE_ID;
    expect(importSourceFor(source).extractFields({ ...form, source })).toEqual({});
  });

  it("sends an Android SMS source the owner's phones, and SMS Backup+ the emails too", () => {
    const source = SMS_BACKUP_PLUS_SOURCE_ID;
    expect(importSourceFor(source).extractFields({ ...form, source })).toMatchObject({
      attachment_media: "compress",
      owner_phones: ["+15555550100"],
      owner_emails: ["me@example.com"],
      obfuscate: true,
    });
  });

  it("sends an iPhone backup its password and a Mac none", () => {
    expect(
      importSourceFor("imessage-ios").extractFields({ ...form, source: "imessage-ios" }),
    ).toMatchObject({ backup_password: "secret" });
    expect(
      importSourceFor("imessage-macos").extractFields({ ...form, source: "imessage-macos" }),
    ).not.toHaveProperty("backup_password");
  });
});
