import type { ReactNode, RefObject } from "react";
import type { PhoneTokenFieldHandle } from "../../../components/PhoneTokenField";
import type {
  ImportSourceDescriptor,
  OwnerPhoneState,
  ReadinessInput,
} from "../../../lib/importSources/types";
import type { PhoneCountryChoice } from "../../../lib/phoneCountries";
import type { AttachmentChoices } from "../../../lib/types";

/**
 * The Import form's props: the fields a source's readiness reads, less the
 * owner phone entry the form builds itself, and the rest of the form's
 * fields and handlers. Every source's form section is given them.
 */
export type ImportFormFieldsProps = Omit<ReadinessInput, "ownerPhoneEntry"> & {
  onSourceChange: (source: string) => void;
  onBackupPathChange: (path: string) => void;
  onBackupPasswordChange: (value: string) => void;
  showBackupPassword: boolean;
  onToggleBackupPassword: () => void;
  onAttachmentRootChange: (path: string) => void;
  onAppleContactsChange: (path: string) => void;
  onWhatsappKeyChange: (value: string) => void;
  showWhatsappKey: boolean;
  onToggleWhatsappKey: () => void;
  onWhatsappWaChange: (path: string) => void;
  onWhatsappMediaChange: (path: string) => void;
  onWhatsappDbChange: (path: string) => void;
  isBusinessApp: boolean;
  onIsBusinessAppChange: (value: boolean) => void;
  onWhatsappOwnerPhoneChange: (value: string) => void;
  attachments: AttachmentChoices;
  onAttachmentsChange: (attachments: AttachmentChoices) => void;
  onOwnerPhonesChange: (phones: string[]) => void;
  onOwnerEmailsChange: (value: string) => void;
  /** The account's phones for SBR mismatch checks (empty until loaded). */
  profilePhones: string[];
  /** True when the profile request failed (fail open on the mismatch check). */
  profilePhonesError: boolean;
  showMissingAccountPhoneWarning: boolean;
  formatOpen: boolean;
  onToggleFormat: () => void;
  processingOpen: boolean;
  onToggleProcessing: () => void;
  obfuscate: boolean;
  onObfuscateChange: (value: boolean) => void;
  /** The IANA zone iMazing dates are read in; shown only for that source. */
  timeZone: string;
  onTimeZoneChange: (zone: string) => void;
  /** The phone's country as an ISO code, or empty for none; every source has it. */
  phoneCountry: string;
  /** The countries to offer, from `GET /v1/phone-countries`; empty until loaded. */
  phoneCountries: readonly PhoneCountryChoice[];
  onPhoneCountryChange: (code: string) => void;
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
export type ImportFormSectionProps = ImportFormFieldsProps & {
  /** The selected source's own descriptor. */
  descriptor: ImportSourceDescriptor;
  ownerPhoneEntry: OwnerPhoneEntry;
  errors: Partial<Record<string, string>>;
  /** The Attachments field, for a section whose source shows it to place. */
  attachmentFields: ReactNode;
};
