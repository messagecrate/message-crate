import { checkOptionalPath, type ImportPathStat, PATH_MISSING } from "./pathChecks";

export const WHATSAPP_SOURCE_ID = "whatsapp";

export const WHATSAPP_DEFAULT_METHOD = "whatsapp-android";

export const WHATSAPP_METHODS = [
  { id: "whatsapp-android", label: "Android" },
  { id: "whatsapp-ios", label: "iPhone" },
] as const;

export type WhatsappMethodId = (typeof WHATSAPP_METHODS)[number]["id"];

const WHATSAPP_METHOD_IDS = new Set<string>(WHATSAPP_METHODS.map((m) => m.id));

export function isWhatsappMethod(source: string): source is WhatsappMethodId {
  return WHATSAPP_METHOD_IDS.has(source);
}

export function whatsappShowsKey(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

export function whatsappShowsMedia(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

export function whatsappShowsDb(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

export function whatsappShowsBusiness(method: WhatsappMethodId): boolean {
  return method === "whatsapp-ios";
}

/** An iPhone backup can be encrypted with a password; an Android one takes a key. */
export function whatsappShowsPassword(method: WhatsappMethodId): boolean {
  return method === "whatsapp-ios";
}

export function whatsappShowsContactsDb(_method: WhatsappMethodId): boolean {
  return true;
}

export const WHATSAPP_CRYPT_NAMES = [
  "msgstore.db.crypt12",
  "msgstore.db.crypt14",
  "msgstore.db.crypt15",
] as const;

export function whatsappCryptRequired(hasMsgstoreDb: boolean, cryptName: string | null): boolean {
  return !hasMsgstoreDb && cryptName !== null;
}

export type WhatsappPathStats = {
  backup: ImportPathStat | null;
  contactsDb: ImportPathStat | null;
  media: ImportPathStat | null;
  db: ImportPathStat | null;
  hasMsgstoreDb: boolean;
  cryptName: string | null;
  /** Whether the iPhone backup directory is encrypted; null when unknown or not iPhone. */
  backupEncrypted: boolean | null;
};

export function emptyWhatsappPathStats(): WhatsappPathStats {
  return {
    backup: null,
    contactsDb: null,
    media: null,
    db: null,
    hasMsgstoreDb: false,
    cryptName: null,
    backupEncrypted: null,
  };
}

export const WHATSAPP_ERR_DIRECTORY_IS_FILE = "Pick the backup directory.";
export const WHATSAPP_ERR_CRYPT_KEY = "Decryption key is required for an encrypted backup.";
export const WHATSAPP_ERR_ENCRYPTED_PASSWORD =
  "The backup is encrypted — fill Encryption password.";
export const WHATSAPP_ERR_MUST_BE_FILE = "This path must be a file.";
export const WHATSAPP_ERR_MUST_BE_DIRECTORY = "This path must be a directory.";
export const WHATSAPP_ERR_OWNER_PHONE = "Owner's WhatsApp number is required.";

/**
 * Where the account holder's number comes from. An Android crypt backup
 * carries no owner, so the form's number is required there. An iPhone
 * backup carries it in WhatsApp's preferences; the form's number is a
 * fallback for a backup without that key, so the field is optional.
 */
export function whatsappOwnerPhoneRequired(method: WhatsappMethodId): boolean {
  return method === "whatsapp-android";
}

type WhatsappCanImportArgs = {
  method: WhatsappMethodId;
  backupPath: string;
  key: string;
  backupPassword: string;
  contactsDb: string;
  media: string;
  db: string;
  ownerPhone: string;
  stats: WhatsappPathStats;
};

type WhatsappImportErrorKey =
  | "backupPath"
  | "key"
  | "backupPassword"
  | "contactsDb"
  | "media"
  | "db"
  | "ownerPhone";

export function whatsappCanImport(args: WhatsappCanImportArgs): {
  enabled: boolean;
  errors: Partial<Record<WhatsappImportErrorKey, string>>;
} {
  const errors: Partial<Record<WhatsappImportErrorKey, string>> = {};

  const backupPath = args.backupPath.trim();
  if (backupPath === "") {
    return { enabled: false, errors: {} };
  }

  if (args.stats.backup === null) {
    return { enabled: false, errors: {} };
  }

  const backupStat = args.stats.backup;
  if (!backupStat.exists) {
    errors.backupPath = PATH_MISSING;
  } else if (backupStat.isFile) {
    errors.backupPath = WHATSAPP_ERR_DIRECTORY_IS_FILE;
  }

  if (
    args.method === "whatsapp-android" &&
    whatsappCryptRequired(args.stats.hasMsgstoreDb, args.stats.cryptName) &&
    args.key.trim() === ""
  ) {
    errors.key = WHATSAPP_ERR_CRYPT_KEY;
  }

  if (
    whatsappShowsPassword(args.method) &&
    args.stats.backupEncrypted === true &&
    args.backupPassword.trim() === ""
  ) {
    errors.backupPassword = WHATSAPP_ERR_ENCRYPTED_PASSWORD;
  }

  if (whatsappOwnerPhoneRequired(args.method) && args.ownerPhone.trim() === "") {
    errors.ownerPhone = WHATSAPP_ERR_OWNER_PHONE;
  }

  if (whatsappShowsContactsDb(args.method)) {
    checkOptionalPath(
      args.contactsDb,
      args.stats.contactsDb,
      errors,
      "contactsDb",
      WHATSAPP_ERR_MUST_BE_FILE,
      "file",
    );
  }

  if (whatsappShowsMedia(args.method)) {
    checkOptionalPath(
      args.media,
      args.stats.media,
      errors,
      "media",
      WHATSAPP_ERR_MUST_BE_DIRECTORY,
      "directory",
    );
  }

  if (whatsappShowsDb(args.method)) {
    checkOptionalPath(args.db, args.stats.db, errors, "db", WHATSAPP_ERR_MUST_BE_FILE, "file");
  }

  const contactsCheckPending =
    whatsappShowsContactsDb(args.method) &&
    args.contactsDb.trim() !== "" &&
    args.stats.contactsDb === null;
  const mediaCheckPending =
    whatsappShowsMedia(args.method) && args.media.trim() !== "" && args.stats.media === null;
  const dbCheckPending =
    whatsappShowsDb(args.method) && args.db.trim() !== "" && args.stats.db === null;

  const enabled =
    Object.keys(errors).length === 0 &&
    backupPath !== "" &&
    !contactsCheckPending &&
    !mediaCheckPending &&
    !dbCheckPending;

  return { enabled, errors };
}
