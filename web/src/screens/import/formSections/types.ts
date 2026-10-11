import type { ReactNode, RefObject } from "react";
import type { PhoneTokenFieldHandle } from "../../../components/PhoneTokenField";
import type {
  ImportSourceDescriptor,
  OwnerPhoneState,
  ReadinessInput,
} from "../../../lib/importSources/types";

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
export type ImportFormSectionProps = ReadinessInput & {
  /** The selected source's own descriptor. */
  descriptor: ImportSourceDescriptor;
  ownerPhoneEntry: OwnerPhoneEntry;
  errors: Partial<Record<string, string>>;
  /** The Attachments field, for a section whose source shows it to place. */
  attachmentFields: ReactNode;
};
