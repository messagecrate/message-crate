import type { ComponentType, ReactNode, RefObject } from "react";
import type { PhoneTokenFieldHandle } from "../../components/PhoneTokenField";
import type { ImportFormFieldsProps } from "../../screens/import/ImportFormFields";
import type { ImportJobFormValues } from "../../screens/import/useImportJob";
import type { ImportSourceId } from "../exportSources";
import type { ImporterExtraField } from "../system-settings";
import type { ExtractConfig } from "../types";

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
 * How a run reads the backup's identities before it starts, so the person
 * can add the ones missing from their profile. `ios` is true for an iPhone
 * backup and false for a Messages database.
 */
export type BackupIdentityRead = { ios: boolean };

/** The fields `extract` reads for one source, beside the ones every run sends. */
export type ExtractFields = Partial<Omit<ExtractConfig, "source" | "path" | "output_dir">>;

/**
 * The owner phone numbers as they are being typed, for a source that asks
 * for them. The Import form holds this state, because the Import button
 * reads it too.
 */
export type OwnerPhoneEntry = {
  fieldRef: RefObject<PhoneTokenFieldHandle | null>;
  onDraftChange: (draft: string) => void;
  /** True while a number is typed and not yet committed. */
  draftPending: boolean;
  /** The committed numbers and the typed one, as the profile check reads them. */
  phonesForMatch: string[];
  /** True when none of `phonesForMatch` is on the profile. */
  mismatch: boolean;
  mismatchAck: boolean;
  onMismatchAckChange: (value: boolean) => void;
};

/** What the Import form reads to decide whether Import can start. */
export type ReadinessInput = ImportFormFieldsProps & { ownerPhoneEntry: OwnerPhoneEntry };

/** Whether the source's fields are ready for Import, and the problem with each field that is not. */
export type Readiness = {
  enabled: boolean;
  errors: Partial<Record<string, string>>;
};

/** What a source's form section is given. */
export type ImportFormSectionProps = ReadinessInput & {
  /** The selected source's own descriptor. */
  descriptor: ImportSourceDescriptor;
  errors: Partial<Record<string, string>>;
  /** The Attachments field, for a section whose source shows it to place. */
  attachmentFields: ReactNode;
};

/**
 * What the Import screen and the run know about one backup source, in one
 * place. Each field is read for the selected source, so a new source needs
 * only its descriptor. The exception is the Import screen's path checks as
 * the paths are typed: each fills its own source's stats (`pathStats`,
 * `whatsappStats`), which the source's readiness and form section read, so
 * a source that checks its paths adds a check and its stats there too.
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
   * True when the form asks for the one phone number the backup's account
   * is registered to. The profile's first phone pre-fills it.
   */
  asksAccountPhone: boolean;
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
   * How a new run of this method reads the backup's identities before it
   * starts, or null for a method whose run does not read them.
   */
  backupIdentityRead(method: M): BackupIdentityRead | null;
  /** The secret this method's extract reads, or null for none. */
  snapshotSecret(method: M): SnapshotSecret | null;
  /** How the form asks for this method's backup. */
  backupField(method: M): BackupField;
  /** Whether the source's own fields let Import start, and what is wrong with each. */
  readiness(input: ReadinessInput & { source: M }): Readiness;
  /** The source's fields after its backup field, in the Import Messages section. */
  FormSection: ComponentType<ImportFormSectionProps>;
};
