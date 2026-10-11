import {
  checkOptionalPath,
  checkRequiredPath,
  type ImportPathStat,
  type PathKind,
} from "./pathChecks";

export const IMESSAGE_SOURCE_ID = "imessage";

export const IMESSAGE_DEFAULT_METHOD = "imessage-ios";

export const IMESSAGE_METHODS = [
  { id: "imessage-macos", label: "Mac Messages" },
  { id: "imessage-ios", label: "iPhone backup" },
  { id: "imessage-jailbreak", label: "Jailbroken iPhone" },
] as const;

export type ImessageMethodId = (typeof IMESSAGE_METHODS)[number]["id"];

const IMESSAGE_METHOD_IDS = new Set<string>(IMESSAGE_METHODS.map((m) => m.id));

export function isImessageMethod(source: string): source is ImessageMethodId {
  return IMESSAGE_METHOD_IDS.has(source);
}

export function imessageApplePlatform(method: ImessageMethodId): "macOS" | "iOS" {
  return method === "imessage-ios" ? "iOS" : "macOS";
}

export function imessageShowsPassword(method: ImessageMethodId): boolean {
  return method === "imessage-ios";
}

export function imessageShowsAttachmentRoot(method: ImessageMethodId): boolean {
  return method === "imessage-macos" || method === "imessage-jailbreak";
}

export function imessageShowsAppleContacts(method: ImessageMethodId): boolean {
  return method === "imessage-macos" || method === "imessage-jailbreak";
}

export function imessageAttachmentRootRequired(method: ImessageMethodId): boolean {
  return method === "imessage-jailbreak";
}

/** Platform choices shown in Import. Jailbreak stays off the list unless it is already selected. */
export function imessageVisiblePlatforms(
  selected: ImessageMethodId,
): ReadonlyArray<(typeof IMESSAGE_METHODS)[number]> {
  if (selected === "imessage-jailbreak") {
    return IMESSAGE_METHODS;
  }
  return IMESSAGE_METHODS.filter((m) => m.id !== "imessage-jailbreak");
}

export type ImessagePathStats = {
  backup: ImportPathStat | null;
  attachmentRoot: ImportPathStat | null;
  appleContacts: ImportPathStat | null;
  backupEncrypted: boolean | null;
};

export function emptyImessagePathStats(): ImessagePathStats {
  return {
    backup: null,
    attachmentRoot: null,
    appleContacts: null,
    backupEncrypted: null,
  };
}

export function imessageStatsForMethod(
  method: ImessageMethodId,
  stats: ImessagePathStats,
): ImessagePathStats {
  return {
    ...stats,
    backupEncrypted: method === "imessage-ios" ? stats.backupEncrypted : null,
  };
}

export const IMESSAGE_ERR_IPHONE_NOT_DIRECTORY = "Pick the backup directory.";
export const IMESSAGE_ERR_MAC_NOT_FILE = "Pick chat.db.";
export const IMESSAGE_ERR_JAILBREAK_NOT_FILE = "Pick sms.db.";
export const IMESSAGE_ERR_ATTACHMENT_NOT_DIRECTORY =
  "Pick the directory that contains Attachments and StickerCache.";
export const IMESSAGE_ERR_CONTACTS_NOT_FILE =
  "Pick AddressBook-v22.abcddb or AddressBook.sqlitedb.";
export const IMESSAGE_ERR_ENCRYPTED_PASSWORD =
  "The backup is encrypted — fill Encryption password.";

/** The kind of path each method's backup path field takes, and its message for any other. */
const IMESSAGE_BACKUP_PATH: Record<ImessageMethodId, { expected: PathKind; kindError: string }> = {
  "imessage-ios": { expected: "directory", kindError: IMESSAGE_ERR_IPHONE_NOT_DIRECTORY },
  "imessage-macos": { expected: "file", kindError: IMESSAGE_ERR_MAC_NOT_FILE },
  "imessage-jailbreak": { expected: "file", kindError: IMESSAGE_ERR_JAILBREAK_NOT_FILE },
};

type ImessageCanImportArgs = {
  method: ImessageMethodId;
  backupPath: string;
  attachmentRoot: string;
  appleContacts: string;
  backupPassword: string;
  stats: ImessagePathStats;
};

type ImessageImportErrorKey = "backupPath" | "attachmentRoot" | "appleContacts" | "backupPassword";

export function imessageCanImport(args: ImessageCanImportArgs): {
  enabled: boolean;
  errors: Partial<Record<ImessageImportErrorKey, string>>;
} {
  const errors: Partial<Record<ImessageImportErrorKey, string>> = {};

  const backupPath = args.backupPath.trim();
  if (backupPath === "") {
    return { enabled: false, errors: {} };
  }

  if (args.stats.backup === null) {
    return { enabled: false, errors: {} };
  }

  const backup = IMESSAGE_BACKUP_PATH[args.method];
  checkRequiredPath(args.stats.backup, errors, "backupPath", backup.kindError, backup.expected);

  const attachmentRoot = args.attachmentRoot.trim();

  if (imessageShowsAttachmentRoot(args.method)) {
    checkOptionalPath(
      attachmentRoot,
      args.stats.attachmentRoot,
      errors,
      "attachmentRoot",
      IMESSAGE_ERR_ATTACHMENT_NOT_DIRECTORY,
      "directory",
    );
  }

  if (imessageShowsAppleContacts(args.method)) {
    checkOptionalPath(
      args.appleContacts,
      args.stats.appleContacts,
      errors,
      "appleContacts",
      IMESSAGE_ERR_CONTACTS_NOT_FILE,
      "file",
    );
  }

  if (
    args.method === "imessage-ios" &&
    args.stats.backupEncrypted === true &&
    args.backupPassword.trim() === ""
  ) {
    errors.backupPassword = IMESSAGE_ERR_ENCRYPTED_PASSWORD;
  }

  const attachmentCheckPending =
    imessageShowsAttachmentRoot(args.method) &&
    attachmentRoot !== "" &&
    args.stats.attachmentRoot === null;
  const contactsCheckPending =
    imessageShowsAppleContacts(args.method) &&
    args.appleContacts.trim() !== "" &&
    args.stats.appleContacts === null;

  const enabled =
    Object.keys(errors).length === 0 &&
    backupPath !== "" &&
    (!imessageAttachmentRootRequired(args.method) || attachmentRoot !== "") &&
    !attachmentCheckPending &&
    !contactsCheckPending;

  return { enabled, errors };
}

export function macMessagesDbPath(homeDir: string): string {
  const trimmed = homeDir.replace(/[/\\]+$/, "");
  if (trimmed === "") {
    return "";
  }
  return `${trimmed}/Library/Messages/chat.db`;
}

export function shouldPrefillMacMessagesDb(args: {
  os: string;
  homeDir: string;
  chatDbExists: boolean;
  rememberedPath: string;
}): string {
  const remembered = args.rememberedPath.trim();
  if (remembered !== "") {
    return remembered;
  }
  if (args.os !== "macos" || !args.chatDbExists) {
    return "";
  }
  return macMessagesDbPath(args.homeDir);
}
