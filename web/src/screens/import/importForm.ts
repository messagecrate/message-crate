/**
 * The Import form's state: every field the person fills in, and how the
 * form opens and shows them, in one value changed by one reducer.
 *
 * `ImportScreen` holds it with `useReducer` and hands the state and the
 * dispatch to `ImportFormFields`, which hands them to the selected source's
 * form section, so a field added to the form is added here and where it is
 * rendered, and nowhere between.
 */
import { attachmentChoicesOf, DEFAULT_ATTACHMENT_CHOICES } from "../../lib/attachmentChoices";
import { IMESSAGE_DEFAULT_METHOD } from "../../lib/imessageImport";
import { splitEmails } from "../../lib/importSources/androidSms";
import type { ImportJobFormValues } from "../../lib/importSources/types";
import type { ImporterExtraField } from "../../lib/systemSettings";
import type { AttachmentChoices } from "../../lib/types";

/**
 * The owner fields the account's profile pre-fills, each true once it holds
 * a value the profile must not overwrite: one the profile filled, one the
 * person typed, or one a resumed run restored.
 */
type Seeded = { ownerPhones: boolean; ownerEmails: boolean; whatsappOwnerPhone: boolean };

export type ImportFormState = {
  /** The selected method's id. */
  source: string;
  backupPath: string;
  backupPassword: string;
  showBackupPassword: boolean;
  attachmentRoot: string;
  appleContacts: string;
  whatsappKey: string;
  showWhatsappKey: boolean;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
  isBusinessApp: boolean;
  /** The holder's WhatsApp number; the profile's first phone pre-fills it. */
  whatsappOwnerPhone: string;
  attachments: AttachmentChoices;
  ownerPhones: string[];
  /** Owner email addresses as typed; split into a list when the import starts. */
  ownerEmails: string;
  /** Whether the Import Messages section is open. */
  formatOpen: boolean;
  /** Whether the Processing Options section is open. */
  processingOpen: boolean;
  obfuscate: boolean;
  /** The phone's country as an ISO code, or empty for none. */
  phoneCountry: string;
  /**
   * The zone iMazing dates are read in, picked under Processing Options, or
   * null for the account's own zone.
   */
  timeZone: string | null;
  seeded: Seeded;
};

/** The fields a `set` action changes: every one but the seeded marks. */
type FormField = Exclude<keyof ImportFormState, "seeded">;

/** The path fields the form remembers: the backup, and the ones beside it. */
export type ImportPathField = "backupPath" | ImporterExtraField;

/** The paths a source is picked with, as the remembered paths give them. */
export type SourcePaths = Record<ImportPathField, string>;

export type ImportFormAction =
  /** One field, as the person changed it. */
  | { [K in FormField]: { type: "set"; field: K; value: ImportFormState[K] } }[FormField]
  /** A source or a method picked, with the paths remembered for it. */
  | { type: "source"; source: string; paths: SourcePaths }
  /** The form a resumed or restarted Import Run was started with. */
  | { type: "refill"; form: ImportJobFormValues }
  /**
   * The profile's values for the owner fields; each fills its field only
   * when the field is empty and not yet seeded. An empty value fills nothing.
   */
  | { type: "seed"; ownerPhones?: string[]; ownerEmails?: string; whatsappOwnerPhone?: string }
  /** A source without owner fields was picked: the next one that has them is seeded again. */
  | { type: "unseed" };

const NOT_SEEDED: Seeded = { ownerPhones: false, ownerEmails: false, whatsappOwnerPhone: false };

/** A new form, on the default method, with its remembered backup path. */
export function newImportForm(backupPath: string): ImportFormState {
  return {
    source: IMESSAGE_DEFAULT_METHOD,
    backupPath,
    backupPassword: "",
    showBackupPassword: false,
    attachmentRoot: "",
    appleContacts: "",
    whatsappKey: "",
    showWhatsappKey: false,
    whatsappWa: "",
    whatsappMedia: "",
    whatsappDb: "",
    isBusinessApp: false,
    whatsappOwnerPhone: "",
    attachments: DEFAULT_ATTACHMENT_CHOICES,
    ownerPhones: [],
    ownerEmails: "",
    formatOpen: true,
    processingOpen: false,
    obfuscate: false,
    phoneCountry: "",
    timeZone: null,
    seeded: NOT_SEEDED,
  };
}

function isSeededField(field: FormField): field is keyof Seeded {
  return field === "ownerPhones" || field === "ownerEmails" || field === "whatsappOwnerPhone";
}

function seed(
  state: ImportFormState,
  action: ImportFormAction & { type: "seed" },
): ImportFormState {
  const next = { ...state, seeded: { ...state.seeded } };
  if (
    action.whatsappOwnerPhone &&
    !state.seeded.whatsappOwnerPhone &&
    state.whatsappOwnerPhone.trim() === ""
  ) {
    next.whatsappOwnerPhone = action.whatsappOwnerPhone;
    next.seeded.whatsappOwnerPhone = true;
  }
  if (action.ownerEmails && !state.seeded.ownerEmails && state.ownerEmails.trim() === "") {
    next.ownerEmails = action.ownerEmails;
    next.seeded.ownerEmails = true;
  }
  if (action.ownerPhones?.length && !state.seeded.ownerPhones && state.ownerPhones.length === 0) {
    next.ownerPhones = action.ownerPhones;
    next.seeded.ownerPhones = true;
  }
  return next;
}

export function importFormReducer(
  state: ImportFormState,
  action: ImportFormAction,
): ImportFormState {
  switch (action.type) {
    case "set": {
      const next: ImportFormState = { ...state, [action.field]: action.value };
      return isSeededField(action.field)
        ? { ...next, seeded: { ...state.seeded, [action.field]: true } }
        : next;
    }
    case "source":
      return {
        ...state,
        ...action.paths,
        source: action.source,
        // Only an iPhone WhatsApp backup can be from WhatsApp Business.
        isBusinessApp: action.source === "whatsapp-ios" ? state.isBusinessApp : false,
      };
    case "refill": {
      const { form } = action;
      return {
        ...state,
        source: form.source,
        backupPath: form.backupPath,
        // The secrets are never stored with a run; a resume that needs one asks for it.
        backupPassword: "",
        attachmentRoot: form.attachmentRoot,
        appleContacts: form.appleContacts,
        whatsappKey: "",
        whatsappWa: form.whatsappWa,
        whatsappMedia: form.whatsappMedia,
        whatsappDb: form.whatsappDb,
        isBusinessApp: form.isBusinessApp,
        whatsappOwnerPhone: form.whatsappOwnerPhone,
        attachments: attachmentChoicesOf(form),
        ownerPhones: form.ownerPhones,
        ownerEmails: form.ownerEmails.join(", "),
        obfuscate: form.obfuscate,
        phoneCountry: form.phoneCountry,
        timeZone: form.timeZone,
        // Restored values count as seeded: the profile must not overwrite them.
        seeded: { ownerPhones: true, ownerEmails: true, whatsappOwnerPhone: true },
      };
    }
    case "seed":
      return seed(state, action);
    case "unseed":
      return { ...state, seeded: NOT_SEEDED };
  }
}

/** The zone iMazing dates are read in: the one picked, or the account's. */
export function formTimeZone(form: ImportFormState, accountTimeZone: string): string {
  return form.timeZone ?? accountTimeZone;
}

/**
 * The values an Import Run starts with, from the form. `ownerPhones` is the
 * list the form committed when Import was pressed, which can hold a number
 * the state has not caught up with yet.
 */
export function importJobValues(
  form: ImportFormState,
  accountTimeZone: string,
  ownerPhones: string[],
): ImportJobFormValues {
  return {
    source: form.source,
    backupPath: form.backupPath,
    backupPassword: form.backupPassword,
    ...form.attachments,
    ownerPhones,
    ownerEmails: splitEmails(form.ownerEmails),
    obfuscate: form.obfuscate,
    timeZone: formTimeZone(form, accountTimeZone),
    phoneCountry: form.phoneCountry,
    attachmentRoot: form.attachmentRoot,
    appleContacts: form.appleContacts,
    whatsappKey: form.whatsappKey,
    whatsappWa: form.whatsappWa,
    whatsappMedia: form.whatsappMedia,
    whatsappDb: form.whatsappDb,
    isBusinessApp: form.isBusinessApp,
    whatsappOwnerPhone: form.whatsappOwnerPhone,
  };
}
