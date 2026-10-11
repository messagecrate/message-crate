import type { ReactNode, RefObject } from "react";
import type { PhoneTokenFieldHandle } from "../../../components/PhoneTokenField";
import type { ImessagePathStats } from "../../../lib/imessageImport";
import type { ImportSourceDescriptor, OwnerPhoneState } from "../../../lib/importSources/types";
import type { PhoneCountryChoice } from "../../../lib/phoneCountries";
import type { AttachmentChoices } from "../../../lib/types";
import type { WhatsappPathStats } from "../../../lib/whatsappImport";

/**
 * The Import form's props. They carry every field a source's readiness
 * reads (`ReadinessInput`, less the owner phone entry the form builds), and
 * every source's form section is given them.
 */
export type ImportFormFieldsProps = {
  source: string;
  onSourceChange: (source: string) => void;
  backupPath: string;
  onBackupPathChange: (path: string) => void;
  backupPassword: string;
  onBackupPasswordChange: (value: string) => void;
  showBackupPassword: boolean;
  onToggleBackupPassword: () => void;
  attachmentRoot: string;
  onAttachmentRootChange: (path: string) => void;
  appleContacts: string;
  onAppleContactsChange: (path: string) => void;
  pathStats: ImessagePathStats;
  whatsappKey: string;
  onWhatsappKeyChange: (value: string) => void;
  showWhatsappKey: boolean;
  onToggleWhatsappKey: () => void;
  whatsappWa: string;
  onWhatsappWaChange: (path: string) => void;
  whatsappMedia: string;
  onWhatsappMediaChange: (path: string) => void;
  whatsappDb: string;
  onWhatsappDbChange: (path: string) => void;
  isBusinessApp: boolean;
  onIsBusinessAppChange: (value: boolean) => void;
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  onWhatsappOwnerPhoneChange: (value: string) => void;
  whatsappStats: WhatsappPathStats;
  attachments: AttachmentChoices;
  onAttachmentsChange: (attachments: AttachmentChoices) => void;
  ownerPhones: string[];
  onOwnerPhonesChange: (phones: string[]) => void;
  /** Owner email addresses as typed (SMS Backup+ only); commas separate several. */
  ownerEmails: string;
  onOwnerEmailsChange: (value: string) => void;
  /** The account's phones for SBR mismatch checks (empty until loaded). */
  profilePhones: string[];
  profilePhonesReady: boolean;
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
