import type { ImportSourceId } from "../exportSources";
import type { ImessagePathStats } from "../imessageImport";
import type { ImporterExtraField } from "../system-settings";
import type { AttachmentChoices, ExtractConfig } from "../types";
import type { WhatsappPathStats } from "../whatsappImport";

/**
 * One way Import reads a source's backup, by the id the form and the
 * desktop `extract` command use for it. Apple Messages and WhatsApp have
 * several, picked under Platform; every other source has one, whose id is
 * the source's own.
 */
export type ImportMethod = { id: string; label: string };

/** How the Import form asks for a backup. */
export type BackupField = {
  label: string;
  /** True for a directory picker, false for a file picker. */
  directory: boolean;
  /** The file picker's filters, for a backup that is one file. */
  filters?: { name: string; extensions: string[] }[];
  hint?: string;
  placeholder?: string;
};

/** A secret an Import Run can be started with: the form snapshot never stores either. */
export type SnapshotSecret = "backupPassword" | "whatsappKey";

/**
 * An option under Processing Options that only some sources have. The
 * phone's country is every source's, so it is not one of these.
 */
export type ProcessingOption = "obfuscate" | "timeZone" | "whatsappFallbackPhone";

/**
 * How a run reads an Apple Messages backup's identities before it starts,
 * through the Apple Messages Reader, so the person can add the ones missing
 * from their profile. `ios` is true for an iPhone backup and false for a
 * Messages database.
 */
export type AppleIdentityRead = { ios: boolean };

/** The Import form's values, as an Import Run is started and resumed with them. */
export type ImportJobFormValues = AttachmentChoices & {
  source: string;
  backupPath: string;
  backupPassword: string;
  ownerPhones: string[];
  /** Owner email addresses; only SMS Backup+ reads them. */
  ownerEmails: string[];
  obfuscate: boolean;
  /** The IANA zone iMazing dates are read in: the account's, or the one picked
   * under Processing Options. Only the iMazing extract reads it, because its
   * dates carry no zone of their own. */
  timeZone: string;
  /**
   * The country of the phone the backup came from, as an ISO code (`GB`), or
   * empty for none. The Import Run states it, and the server reads every
   * number the run's files write without its `+` code as a number there
   * (#1676).
   */
  phoneCountry: string;
  attachmentRoot: string;
  appleContacts: string;
  whatsappKey: string;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
  /** Whether an iPhone WhatsApp backup is from WhatsApp Business. */
  isBusinessApp: boolean;
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  /**
   * The server's attachment size limit, in bytes, as this Import Run works
   * to it. Not a field the person fills in: a new run reads it from
   * `GET /v1/server` before Staging, and it is stored with the run in the
   * form snapshot, so a resume uses the number the run was staged and
   * reviewed against even when the owner has changed the limit since.
   */
  assetMaxBytes?: number;
};

/** The fields `extract` reads for one source, beside the ones every run sends. */
export type ExtractFields = Partial<Omit<ExtractConfig, "source" | "path" | "output_dir">>;

/**
 * The owner phone numbers as they are being typed, for a source that asks
 * for them, as its readiness reads them. The Import form holds this state,
 * because the Import button reads it too.
 */
export type OwnerPhoneState = {
  /** True while a number is typed and not yet committed. */
  draftPending: boolean;
  /** The committed numbers and the typed one, as the profile check reads them. */
  phonesForMatch: string[];
  /** True when none of `phonesForMatch` is on the profile. */
  mismatch: boolean;
  mismatchAck: boolean;
};

/**
 * What a source's readiness reads from the Import form to decide whether
 * Import can start. The form's props are built on these fields, so the form
 * passes its props as they are, with the owner phone entry it builds.
 */
export type ReadinessInput = {
  /** The selected method's id. */
  source: string;
  backupPath: string;
  backupPassword: string;
  attachmentRoot: string;
  appleContacts: string;
  pathStats: ImessagePathStats;
  whatsappKey: string;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  whatsappStats: WhatsappPathStats;
  ownerPhones: string[];
  /** Owner email addresses as typed (SMS Backup+ only); commas separate several. */
  ownerEmails: string;
  profilePhonesReady: boolean;
  ownerPhoneEntry: OwnerPhoneState;
};

/** Whether the source's fields are ready for Import, and the problem with each field that is not. */
export type Readiness = {
  enabled: boolean;
  errors: Partial<Record<string, string>>;
};

/**
 * What the Import screen and the run know about one backup source, in one
 * place. Each field is read for the selected source, so a new source needs
 * only its descriptor and its form section. The form section lives in
 * `screens/import/formSections/`, because it renders the source's fields.
 * The exception is the Import screen's path checks, which run as the paths
 * are typed. Each check fills its own source's stats (`pathStats`,
 * `whatsappStats`), which the source's readiness and form section read. A
 * source that checks its paths adds its check and its stats to the Import
 * screen.
 *
 * `M` is the source's own method ids. The functions below are declared in
 * method syntax, which TypeScript checks bivariantly in their parameters, so
 * `ImportSourceDescriptor<ImessageMethodId>` is assignable to
 * `ImportSourceDescriptor`. Written as arrow-function properties they would
 * not be. The list type therefore takes any string as a method, and only
 * `importSourceFor`, which finds a descriptor by the method it is then
 * asked about, keeps the method one of the descriptor's own.
 */
export type ImportSourceDescriptor<M extends string = string> = {
  /** The Import Run's and each message's `source`. */
  id: ImportSourceId;
  /** The name the product gives the source, from `EXPORT_SOURCES`. */
  label: string;
  /** The ways Import reads it. A source read one way lists one, whose id is the source's own. */
  methods: readonly { id: M; label: string }[];
  /** The method a new form, or a pick of this source, starts on. */
  defaultMethod: M;
  /** The methods Platform lists while `selected` is picked. */
  visibleMethods(selected: M): readonly ImportMethod[];
  /**
   * True when the form shows the Attachments field. A source without it
   * copies its attachments, whatever the field held for another source.
   */
  showsAttachmentOptions: boolean;
  /** True when the form asks for the backup device's phone numbers. */
  asksOwnerPhones: boolean;
  /** True when the form also asks for the owner's email addresses. */
  asksOwnerEmails: boolean;
  /**
   * True when the form asks for the WhatsApp phone number the backup's
   * account is registered to (the form's `whatsappOwnerPhone`). The
   * profile's first phone pre-fills it.
   */
  asksWhatsappOwnerPhone: boolean;
  /**
   * The path fields, beside the backup, that the form remembers for each of
   * this source's methods. A field not listed is cleared when the source is
   * picked, and is never remembered for it.
   */
  rememberedPaths: readonly ImporterExtraField[];
  /** True when the import runs wtsexporter, so it cannot start without it. */
  needsWtsexporter: boolean;
  /** The options under Processing Options this method shows. */
  processingOptions(method: M): readonly ProcessingOption[];
  /** The fields `extract` needs from a form whose `source` is one of `methods`. */
  extractFields(form: ImportJobFormValues & { source: M }): ExtractFields;
  /**
   * How a new run of this method reads the backup's identities with the
   * Apple Messages Reader before it starts, or null for a method whose run
   * does not read them. Only an Apple Messages run reads the backup's
   * identities, so every other source returns null.
   */
  appleIdentityRead(method: M): AppleIdentityRead | null;
  /** The secret this method's extract reads, or null for none. */
  snapshotSecret(method: M): SnapshotSecret | null;
  /** How the form asks for this method's backup. */
  backupField(method: M): BackupField;
  /** Whether the source's own fields let Import start, and what is wrong with each. */
  readiness(input: ReadinessInput & { source: M }): Readiness;
};
