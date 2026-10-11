import { describe, expect, it } from "vitest";
import { DIRECTORY_STAT, FILE_STAT, MISSING_STAT, NEITHER_STAT } from "../test/pathStats";
import {
  IMESSAGE_ERR_ATTACHMENT_NOT_DIRECTORY,
  IMESSAGE_ERR_CONTACTS_NOT_FILE,
  IMESSAGE_ERR_ENCRYPTED_PASSWORD,
  IMESSAGE_ERR_IPHONE_NOT_DIRECTORY,
  IMESSAGE_ERR_JAILBREAK_NOT_FILE,
  IMESSAGE_ERR_MAC_NOT_FILE,
  IMESSAGE_METHODS,
  imessageApplePlatform,
  imessageAttachmentRootRequired,
  imessageCanImport,
  imessageShowsAppleContacts,
  imessageShowsAttachmentRoot,
  imessageShowsPassword,
  imessageStatsForMethod,
  imessageVisiblePlatforms,
  isImessageMethod,
  macMessagesDbPath,
  shouldPrefillMacMessagesDb,
} from "./imessageImport";
import { PATH_MISSING } from "./pathChecks";

describe("iMessage methods", () => {
  it("lists three methods", () => {
    expect(IMESSAGE_METHODS.map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
      "imessage-jailbreak",
    ]);
    expect(IMESSAGE_METHODS.map((m) => m.label)).toEqual([
      "Mac Messages",
      "iPhone backup",
      "Jailbroken iPhone",
    ]);
  });

  it("derives converter platform from the method", () => {
    expect(imessageApplePlatform("imessage-ios")).toBe("iOS");
    expect(imessageApplePlatform("imessage-macos")).toBe("macOS");
    expect(imessageApplePlatform("imessage-jailbreak")).toBe("macOS");
  });

  it("shows password only for iPhone backup", () => {
    expect(imessageShowsPassword("imessage-ios")).toBe(true);
    expect(imessageShowsPassword("imessage-macos")).toBe(false);
    expect(imessageShowsPassword("imessage-jailbreak")).toBe(false);
  });

  it("shows attachment root and Apple Contacts on Mac and jailbreak only", () => {
    expect(imessageShowsAttachmentRoot("imessage-macos")).toBe(true);
    expect(imessageShowsAttachmentRoot("imessage-jailbreak")).toBe(true);
    expect(imessageShowsAttachmentRoot("imessage-ios")).toBe(false);
    expect(imessageShowsAppleContacts("imessage-macos")).toBe(true);
    expect(imessageShowsAppleContacts("imessage-jailbreak")).toBe(true);
    expect(imessageShowsAppleContacts("imessage-ios")).toBe(false);
  });

  it("treats only the three method ids as iMessage methods", () => {
    expect(isImessageMethod("imessage-ios")).toBe(true);
    expect(isImessageMethod("whatsapp-android")).toBe(false);
    expect(isImessageMethod("imessage")).toBe(false);
  });

  it("hides jailbreak from the platform list unless it is already selected", () => {
    expect(imessageVisiblePlatforms("imessage-ios").map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
    ]);
    expect(imessageVisiblePlatforms("imessage-macos").map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
    ]);
    expect(imessageVisiblePlatforms("imessage-jailbreak").map((m) => m.id)).toEqual([
      "imessage-macos",
      "imessage-ios",
      "imessage-jailbreak",
    ]);
  });
});

describe("imessageCanImport", () => {
  it("enables iPhone backup when the directory exists", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: DIRECTORY_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
    expect(result.errors).toEqual({});
  });

  it("keeps password optional when encryption is unknown", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: DIRECTORY_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
  });

  it("requires password when Manifest.plist is marked encrypted", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: DIRECTORY_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPassword).toBe(IMESSAGE_ERR_ENCRYPTED_PASSWORD);
  });

  it("enables an encrypted backup when the password is filled", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "secret",
      stats: {
        backup: DIRECTORY_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      },
    });
    expect(result.enabled).toBe(true);
  });

  it("rejects an iPhone backup path that is a .db file", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/copy/sms.db",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.backupPath).toBe(IMESSAGE_ERR_IPHONE_NOT_DIRECTORY);
    // The platform list hides the jailbreak method, so the error must not
    // send the person to it.
    expect(result.errors.backupPath).not.toMatch(/jailbr/i);
  });

  it("enables Mac Messages when chat.db exists", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/Users/sam/Library/Messages/chat.db",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(true);
  });

  it("rejects Mac or jailbreak when the path is a directory", () => {
    const mac = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/Users/sam/Library/Messages",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: DIRECTORY_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(mac.enabled).toBe(false);
    expect(mac.errors.backupPath).toBe(IMESSAGE_ERR_MAC_NOT_FILE);

    const jail = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/Library/SMS",
      attachmentRoot: "/mnt/iphone/Library/SMS",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: DIRECTORY_STAT,
        attachmentRoot: DIRECTORY_STAT,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(jail.enabled).toBe(false);
    expect(jail.errors.backupPath).toBe(IMESSAGE_ERR_JAILBREAK_NOT_FILE);
  });

  it("rejects a backup path that is neither a file nor a directory, for every method", () => {
    const expected = {
      "imessage-ios": IMESSAGE_ERR_IPHONE_NOT_DIRECTORY,
      "imessage-macos": IMESSAGE_ERR_MAC_NOT_FILE,
      "imessage-jailbreak": IMESSAGE_ERR_JAILBREAK_NOT_FILE,
    } as const;
    for (const method of IMESSAGE_METHODS.map((m) => m.id)) {
      const result = imessageCanImport({
        method,
        backupPath: "/dev/null",
        attachmentRoot: "",
        appleContacts: "",
        backupPassword: "",
        stats: {
          backup: NEITHER_STAT,
          attachmentRoot: null,
          appleContacts: null,
          backupEncrypted: null,
        },
      });
      expect(result.enabled, method).toBe(false);
      expect(result.errors.backupPath, method).toBe(expected[method]);
    }
  });

  it("treats attachment root as required only for jailbreak", () => {
    expect(imessageAttachmentRootRequired("imessage-jailbreak")).toBe(true);
    expect(imessageAttachmentRootRequired("imessage-macos")).toBe(false);
    expect(imessageAttachmentRootRequired("imessage-ios")).toBe(false);
  });

  it("requires jailbreak sms.db and attachment directory", () => {
    const missingRoot = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(missingRoot.enabled).toBe(false);

    const ready = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "/mnt/iphone/Library/SMS",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: DIRECTORY_STAT,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(ready.enabled).toBe(true);
  });

  it("disables Import when an optional extra path is set but missing", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "/tmp/missing-attachments",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: MISSING_STAT,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.attachmentRoot).toBe(PATH_MISSING);
  });

  it("rejects an attachment directory that is a file", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "/tmp/chat.db",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: FILE_STAT,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.attachmentRoot).toBe(IMESSAGE_ERR_ATTACHMENT_NOT_DIRECTORY);
  });

  it("rejects an Apple Contacts path that is a directory", () => {
    const result = imessageCanImport({
      method: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "",
      appleContacts: "/tmp/AddressBook",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: null,
        appleContacts: DIRECTORY_STAT,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.appleContacts).toBe(IMESSAGE_ERR_CONTACTS_NOT_FILE);
  });

  it("still validates Apple Contacts when jailbreak attachment root is empty", () => {
    const result = imessageCanImport({
      method: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "",
      appleContacts: "/tmp/AddressBook",
      backupPassword: "",
      stats: {
        backup: FILE_STAT,
        attachmentRoot: null,
        appleContacts: DIRECTORY_STAT,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
    expect(result.errors.appleContacts).toBe(IMESSAGE_ERR_CONTACTS_NOT_FILE);
  });

  it("disables Import while a non-empty path has not been checked yet", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "/backups/iphone",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: null,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
  });

  it("disables Import when the backup path is empty", () => {
    const result = imessageCanImport({
      method: "imessage-ios",
      backupPath: "  ",
      attachmentRoot: "",
      appleContacts: "",
      backupPassword: "",
      stats: {
        backup: null,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(result.enabled).toBe(false);
  });
});

describe("imessageStatsForMethod", () => {
  it("clears encryption state when leaving iPhone backup", () => {
    expect(
      imessageStatsForMethod("imessage-macos", {
        backup: FILE_STAT,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      }),
    ).toEqual({
      backup: FILE_STAT,
      attachmentRoot: null,
      appleContacts: null,
      backupEncrypted: null,
    });
  });
});

describe("Mac Messages pre-fill", () => {
  it("joins chat.db under the home Library directory", () => {
    expect(macMessagesDbPath("/Users/sam")).toBe("/Users/sam/Library/Messages/chat.db");
    expect(macMessagesDbPath("/Users/sam/")).toBe("/Users/sam/Library/Messages/chat.db");
  });

  it("pre-fills only on macOS when the file exists and nothing is remembered", () => {
    expect(
      shouldPrefillMacMessagesDb({
        os: "macos",
        homeDir: "/Users/sam",
        chatDbExists: true,
        rememberedPath: "",
      }),
    ).toBe("/Users/sam/Library/Messages/chat.db");
    expect(
      shouldPrefillMacMessagesDb({
        os: "linux",
        homeDir: "/home/sam",
        chatDbExists: true,
        rememberedPath: "",
      }),
    ).toBe("");
    expect(
      shouldPrefillMacMessagesDb({
        os: "macos",
        homeDir: "/Users/sam",
        chatDbExists: false,
        rememberedPath: "",
      }),
    ).toBe("");
    expect(
      shouldPrefillMacMessagesDb({
        os: "macos",
        homeDir: "/Users/sam",
        chatDbExists: true,
        rememberedPath: "/copied/chat.db",
      }),
    ).toBe("/copied/chat.db");
  });
});
