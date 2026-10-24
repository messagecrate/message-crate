import type { Dispatch, ReactNode, RefObject } from "react";
import type { PhoneTokenFieldHandle } from "../../../components/PhoneTokenField";
import type { ImessagePathStats } from "../../../lib/imessageImport";
import type { ImportSourceDescriptor, OwnerPhoneState } from "../../../lib/importSources/types";
import type { BackupPathStats } from "../../../lib/pathChecks";
import type { PhoneCountryChoice } from "../../../lib/phoneCountries";
import type { WhatsappPathStats } from "../../../lib/whatsappImport";
import type { ImportFormFieldAction, ImportFormState, ImportPathField } from "../importForm";

/** The account's phones, as the owner phone fields read them. */
export type ProfilePhones = {
  /** The account's phones; empty until read, and after a failed read. */
  phones: string[];
  /** True once the account's phones have been read, or the read failed. */
  ready: boolean;
  /** True when the read failed; the mismatch check then lets Import start. */
  error: boolean;
};

/**
 * The Import form's props: the form's state and its dispatch,
 * `onSourceChange` and `onPathChange`, which the screen handles beyond
 * setting a field, and what the form shows beside its fields.
 */
export type ImportFormFieldsProps = {
  form: ImportFormState;
  /** Sets or toggles a field; picking a source and changing a path go through the callbacks below. */
  dispatch: Dispatch<ImportFormFieldAction>;
  /** A source from the source list, or a method from Platform. */
  onSourceChange: (source: string) => void;
  /** A path field changed, which the form remembers for the source. */
  onPathChange: (field: ImportPathField, path: string) => void;
  pathStats: ImessagePathStats;
  whatsappStats: WhatsappPathStats;
  /** What the backup path points at, for the Android SMS sources, iMazing and OpenExtract. */
  backupPathStats: BackupPathStats;
  profilePhones: ProfilePhones;
  /** The account's IANA zone, which iMazing dates are read in unless another is picked. */
  accountTimeZone: string;
  /** The countries to offer, from `GET /v1/phone-countries`; empty until loaded. */
  phoneCountries: readonly PhoneCountryChoice[];
  running: boolean;
  /** Why the last Import could not start, or null. */
  startError: string | null;
  /**
   * Import was pressed on a ready form: with the owner phones, a number
   * still being typed committed, and the Encryption password the run is
   * sent, which is empty while the field is hidden.
   */
  onImport: (start: { ownerPhones: string[]; backupPassword: string }) => void;
};

/**
 * The owner phone numbers as they are being typed, with the field that
 * types them and the handlers that change them, for the form section that
 * renders that field.
 */
export type OwnerPhoneEntry = OwnerPhoneState & {
  fieldRef: RefObject<PhoneTokenFieldHandle | null>;
  onDraftChange: (draft: string) => void;
  onMismatchAckChange: (value: boolean) => void;
};

/** What a source's form section is given. */
export type ImportFormSectionProps = Pick<
  ImportFormFieldsProps,
  "form" | "dispatch" | "onPathChange" | "pathStats" | "whatsappStats"
> & {
  /** The selected source's own descriptor. */
  descriptor: ImportSourceDescriptor;
  ownerPhoneEntry: OwnerPhoneEntry;
  errors: Partial<Record<string, string>>;
  /** The Attachments field, for a section whose source shows it to place. */
  attachmentFields: ReactNode;
  /**
   * True when the section shows the Encryption password: the descriptor's
   * `backupEncrypted` is true. The form reads the same answer to decide
   * whether the password is sent, so the two never disagree.
   */
  passwordShown: boolean;
  /**
   * Default country for contacts, which every section places after its
   * backup's own fields (path, password or key) and the owner fields its
   * source asks for, and before the rest.
   */
  defaultCountryField: ReactNode;
  /** True when the account's phones were read and it has none. */
  showMissingAccountPhoneWarning: boolean;
};
