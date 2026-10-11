import { describe, expect, it } from "vitest";
import { DIRECTORY_STAT, FILE_STAT, MISSING_STAT, NEITHER_STAT } from "../test/pathStats";
import { PATH_MISSING } from "./pathChecks";
import {
  isWhatsappMethod,
  WHATSAPP_ERR_BACKUP_NOT_DIRECTORY,
  WHATSAPP_ERR_CRYPT_KEY,
  WHATSAPP_ERR_ENCRYPTED_PASSWORD,
  WHATSAPP_ERR_MUST_BE_DIRECTORY,
  WHATSAPP_ERR_MUST_BE_FILE,
  WHATSAPP_ERR_OWNER_PHONE,
  whatsappCanImport,
  whatsappCryptRequired,
  whatsappShowsBusiness,
  whatsappShowsKey,
} from "./whatsappImport";

describe("whatsappImport", () => {
  it("knows its two methods and the fields each shows", () => {
    expect(isWhatsappMethod("whatsapp-android")).toBe(true);
    expect(isWhatsappMethod("whatsapp-ios")).toBe(true);
    expect(isWhatsappMethod("whatsapp")).toBe(false);
    expect(whatsappShowsKey("whatsapp-android")).toBe(true);
    expect(whatsappShowsKey("whatsapp-ios")).toBe(false);
    expect(whatsappShowsBusiness("whatsapp-ios")).toBe(true);
    expect(whatsappShowsBusiness("whatsapp-android")).toBe(false);
  });

  it("disables Import when the backup directory does not exist", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/missing-wa",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: MISSING_STAT,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPath).toBe(PATH_MISSING);
  });

  it("rejects an optional contacts path that does not exist", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      backupPassword: "",
      contactsDb: "/tmp/missing-wa.db",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: MISSING_STAT,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.contactsDb).toBe(PATH_MISSING);
  });

  it("requires a key only when a crypt file is used", () => {
    expect(whatsappCryptRequired(true, "msgstore.db.crypt15")).toBe(false);
    expect(whatsappCryptRequired(false, "msgstore.db.crypt15")).toBe(true);
    expect(whatsappCryptRequired(false, null)).toBe(false);
  });

  it("disables Import when the backup path is empty", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "  ",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: null,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors).toEqual({});
  });

  it("disables Import when the Android directory is a file", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/msgstore.db",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: FILE_STAT,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPath).toBe(WHATSAPP_ERR_BACKUP_NOT_DIRECTORY);
  });

  it("disables Import when the backup path is neither a file nor a directory, for every method", () => {
    for (const method of ["whatsapp-android", "whatsapp-ios"] as const) {
      const result = whatsappCanImport({
        method,
        backupPath: "/dev/null",
        key: "",
        backupPassword: "",
        contactsDb: "",
        media: "",
        db: "",
        ownerPhone: "+15555550100",
        stats: {
          backup: NEITHER_STAT,
          contactsDb: null,
          media: null,
          db: null,
          hasMsgstoreDb: true,
          cryptName: null,
          backupEncrypted: null,
        },
      });
      expect(result.enabled, method).toBe(false);
      expect(result.errors.backupPath, method).toBe(WHATSAPP_ERR_BACKUP_NOT_DIRECTORY);
    }
  });

  it("requires the key when only a crypt file is present", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: "msgstore.db.crypt15",
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.key).toBe(WHATSAPP_ERR_CRYPT_KEY);
  });

  it("enables Android Import for a directory with msgstore.db and no key", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: "msgstore.db.crypt15",
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
    expect(result.errors).toEqual({});
  });

  it("rejects an optional contacts path that is a directory", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      backupPassword: "",
      contactsDb: "/tmp/wa.db",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: DIRECTORY_STAT,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.contactsDb).toBe(WHATSAPP_ERR_MUST_BE_FILE);
  });

  it("rejects an optional media path that is a file", () => {
    const result = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "/tmp/media.txt",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: null,
        media: FILE_STAT,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.media).toBe(WHATSAPP_ERR_MUST_BE_DIRECTORY);
  });

  it("enables iPhone Import for a backup directory with no key", () => {
    const result = whatsappCanImport({
      method: "whatsapp-ios",
      backupPath: "/backups/iphone",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
    expect(result.errors).toEqual({});
  });

  // wtsexporter cannot be given an iPhone backup password, so the app
  // decrypts WhatsApp's files itself and needs the password before the run
  // starts. Android has no such password, whatever the probe says.
  it("requires the password for an encrypted iPhone backup", () => {
    const args = {
      backupPath: "/backups/iphone",
      key: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "+15555550100",
      stats: {
        backup: DIRECTORY_STAT,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
        backupEncrypted: true,
      },
    };
    const empty = whatsappCanImport({ ...args, method: "whatsapp-ios", backupPassword: " " });
    expect(empty.enabled).toBe(false);
    expect(empty.errors.backupPassword).toBe(WHATSAPP_ERR_ENCRYPTED_PASSWORD);

    const filled = whatsappCanImport({ ...args, method: "whatsapp-ios", backupPassword: "pw" });
    expect(filled.enabled).toBe(true);
    expect(filled.errors).toEqual({});

    const android = whatsappCanImport({ ...args, method: "whatsapp-android", backupPassword: "" });
    expect(android.errors.backupPassword).toBeUndefined();
  });

  // An Android crypt backup carries no owner number, so the form's number
  // is the only source; an iPhone backup carries it in WhatsApp's
  // preferences, so the field is a fallback and may stay empty.
  it("requires the owner's number on Android and not on iPhone", () => {
    const stats = {
      backup: DIRECTORY_STAT,
      contactsDb: null,
      media: null,
      db: null,
      hasMsgstoreDb: true,
      cryptName: null,
      backupEncrypted: null,
    };
    const android = whatsappCanImport({
      method: "whatsapp-android",
      backupPath: "/tmp/wa",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "  ",
      stats,
    });
    expect(android.enabled).toBe(false);
    expect(android.errors.ownerPhone).toBe(WHATSAPP_ERR_OWNER_PHONE);

    const iphone = whatsappCanImport({
      method: "whatsapp-ios",
      backupPath: "/backups/iphone",
      key: "",
      backupPassword: "",
      contactsDb: "",
      media: "",
      db: "",
      ownerPhone: "",
      stats,
    });
    expect(iphone.enabled).toBe(true);
    expect(iphone.errors).toEqual({});
  });
});
