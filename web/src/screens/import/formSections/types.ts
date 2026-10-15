import type { Dispatch, ReactNode, RefObject } from "react";
import type { PhoneTokenFieldHandle } from "../../../components/PhoneTokenField";
import type { ImessagePathStats } from "../../../lib/imessageImport";
import type { AndroidSmsPathStats } from "../../../lib/importSources/androidSms";
import type { ImportSourceDescriptor, OwnerPhoneState } from "../../../lib/importSources/types";
import type { PhoneCountryChoice } from "../../../lib/phoneCountries";
import type { WhatsappPathStats } from "../../../lib/whatsappImport";
import type { ImportFormAction, ImportFormState, ImportPathField } from "../importForm";

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
  dispatch: Dispatch<ImportFormAction>;
  /** A source from the source list, or a method from Platform. */
  onSourceChange: (source: string) => void;
  /** A path field changed, which the form remembers for the source. */
  onPathChange: (field: ImportPathField, path: string) => void;
  pathStats: ImessagePathStats;
  whatsappStats: WhatsappPathStats;
  /** What the Android SMS backup path points at. */
  androidSmsStats: AndroidSmsPathStats;
  profilePhones: ProfilePhones;
  /** The account's IANA zone, which iMazing dates are read in unless another is picked. */
  accountTimeZone: string;
  /** The countries to offer, from `GET /v1/phone-countries`; empty until loaded. */
  phoneCountries: readonly PhoneCountryChoice[];
  running: boolean;
  /** Optional flushed owner phones (SBR commits draft before import). */
  onImport: (ownerPhones?: string[]) => void;
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
  /** True when the account's phones were read and it has none. */
  showMissingAccountPhoneWarning: boolean;
};
