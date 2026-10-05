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

export type PathStat = {
  exists: boolean;
  isFile: boolean;
  isDirectory: boolean;
};

export type ImessagePathStats = {
  backup: PathStat | null;
  attachmentRoot: PathStat | null;
  appleContacts: PathStat | null;
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

export const IMESSAGE_ERR_PATH_MISSING = "This path does not exist.";
export const IMESSAGE_ERR_IPHONE_PATH_IS_FILE = "Pick the backup directory.";
export const IMESSAGE_ERR_MAC_PATH_IS_DIR = "Pick chat.db.";
export const IMESSAGE_ERR_JAILBREAK_PATH_IS_DIR = "Pick sms.db.";
export const IMESSAGE_ERR_ATTACHMENT_IS_FILE =
  "Pick the directory that contains Attachments and StickerCache.";
export const IMESSAGE_ERR_CONTACTS_IS_DIR = "Pick AddressBook-v22.abcddb or AddressBook.sqlitedb.";
export const IMESSAGE_ERR_ENCRYPTED_PASSWORD =
  "The backup is encrypted — fill Encryption password.";

type ImessageCanImportArgs = {
  method: ImessageMethodId;
  backupPath: string;
  attachmentRoot: string;
  appleContacts: string;
  backupPassword: string;
  stats: ImessagePathStats;
};

type ImessageImportErrorKey = "backupPath" | "attachmentRoot" | "appleContacts" | "backupPassword";

function checkOptionalPath(
  path: string,
  stat: PathStat | null,
  errors: Partial<Record<ImessageImportErrorKey, string>>,
  key: "attachmentRoot" | "appleContacts",
  fileError: string,
  expectDirectory: boolean,
): void {
  const trimmed = path.trim();
  if (trimmed === "") {
    return;
  }
  if (stat === null) {
    return;
  }
  if (!stat.exists) {
    errors[key] = IMESSAGE_ERR_PATH_MISSING;
    return;
  }
  if (expectDirectory) {
    if (stat.isFile) {
      errors[key] = fileError;
    }
  } else if (stat.isDirectory) {
    errors[key] = fileError;
  }
}

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

  const backupStat = args.stats.backup;
  if (!backupStat.exists) {
    errors.backupPath = IMESSAGE_ERR_PATH_MISSING;
  } else {
    switch (args.method) {
      case "imessage-ios":
        if (backupStat.isFile) {
          errors.backupPath = IMESSAGE_ERR_IPHONE_PATH_IS_FILE;
        }
        break;
      case "imessage-macos":
        if (backupStat.isDirectory) {
          errors.backupPath = IMESSAGE_ERR_MAC_PATH_IS_DIR;
        }
        break;
      case "imessage-jailbreak":
        if (backupStat.isDirectory) {
          errors.backupPath = IMESSAGE_ERR_JAILBREAK_PATH_IS_DIR;
        }
        break;
    }
  }

  const attachmentRoot = args.attachmentRoot.trim();

  if (imessageShowsAttachmentRoot(args.method) && attachmentRoot !== "") {
    checkOptionalPath(
      attachmentRoot,
      args.stats.attachmentRoot,
      errors,
      "attachmentRoot",
      IMESSAGE_ERR_ATTACHMENT_IS_FILE,
      true,
    );
  }

  if (imessageShowsAppleContacts(args.method)) {
    checkOptionalPath(
      args.appleContacts,
      args.stats.appleContacts,
      errors,
      "appleContacts",
      IMESSAGE_ERR_CONTACTS_IS_DIR,
      false,
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
