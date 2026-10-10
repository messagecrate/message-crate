import Checkbox from "../../components/Checkbox";
import PasswordField from "../../components/PasswordField";
import PathPicker from "../../components/PathPicker";
import { fieldClass, hintClass, StackedField } from "../../screens/import/ImportFormUi";
import {
  isWhatsappMethod,
  whatsappCryptRequired,
  whatsappOwnerPhoneRequired,
  whatsappShowsBusiness,
  whatsappShowsContactsDb,
  whatsappShowsDb,
  whatsappShowsKey,
  whatsappShowsMedia,
  whatsappShowsPassword,
} from "../whatsappImport";
import { FieldStatus } from "./FieldStatus";
import type { ImportFormSectionProps } from "./types";

const SQLITE_DB_FILTERS = [{ name: "SQLite database", extensions: ["db"] }];
const WHATSAPP_CONTACTS_FILTERS = [{ name: "SQLite database", extensions: ["db", "sqlite"] }];

const WHATSAPP_KEY_HINT =
  "Key file or crypt15 hex. Needed when the directory has an encrypted backup and no msgstore.db.";
const WHATSAPP_CONTACTS_HINT_ANDROID = "Leave empty if wa.db is in the backup directory.";
const WHATSAPP_CONTACTS_HINT_IPHONE = "Leave empty if ContactsV2.sqlite is in the backup.";
const WHATSAPP_MEDIA_HINT =
  "Leave empty if the WhatsApp media directory is in the backup directory.";
const WHATSAPP_DB_HINT = "Leave empty if msgstore.db is in the backup directory.";

/** The label of the holder's WhatsApp number, in the form and under Processing Options. */
const WHATSAPP_OWNER_PHONE_LABEL = "WhatsApp phone number";
const WHATSAPP_OWNER_PHONE_HINT_ANDROID =
  "Pre-filled from your profile. The number your WhatsApp account is registered to.";
const WHATSAPP_OWNER_PHONE_HINT_IPHONE =
  "Fallback, used when the backup does not contain your phone number.";

/**
 * The key, password, owner number, contacts, media, message database,
 * Business choice, and attachments. On iPhone the owner number is a
 * fallback, so it sits under Processing Options instead.
 */
export default function WhatsappFormSection(props: ImportFormSectionProps) {
  const method = props.source;
  if (!isWhatsappMethod(method)) return null;
  const { errors } = props;
  const keyRequired = whatsappCryptRequired(
    props.whatsappStats.hasMsgstoreDb,
    props.whatsappStats.cryptName,
  );
  return (
    <>
      {whatsappShowsKey(method) ? (
        <StackedField label="Decryption key" required={keyRequired} optional={!keyRequired}>
          <PasswordField
            aria-label={keyRequired ? "Decryption key" : "Decryption key (Optional)"}
            value={props.whatsappKey}
            onChange={props.onWhatsappKeyChange}
            autoComplete="new-password"
            showPassword={props.showWhatsappKey}
            onToggle={props.onToggleWhatsappKey}
          />
          <p className={hintClass}>{WHATSAPP_KEY_HINT}</p>
          <FieldStatus message={errors.key} />
        </StackedField>
      ) : null}

      {whatsappShowsPassword(method) ? (
        <StackedField
          label="Encryption password"
          required={props.whatsappStats.backupEncrypted === true}
          optional={props.whatsappStats.backupEncrypted !== true}
        >
          <PasswordField
            aria-label={
              props.whatsappStats.backupEncrypted === true
                ? "Encryption password"
                : "Encryption password (Optional)"
            }
            value={props.backupPassword}
            onChange={props.onBackupPasswordChange}
            autoComplete="new-password"
            showPassword={props.showBackupPassword}
            onToggle={props.onToggleBackupPassword}
          />
          <FieldStatus message={errors.backupPassword} />
        </StackedField>
      ) : null}

      {whatsappOwnerPhoneRequired(method) ? (
        <StackedField label={WHATSAPP_OWNER_PHONE_LABEL} required>
          <input
            type="text"
            inputMode="tel"
            aria-label={WHATSAPP_OWNER_PHONE_LABEL}
            value={props.whatsappOwnerPhone}
            onChange={(e) => props.onWhatsappOwnerPhoneChange(e.target.value)}
            placeholder="+1 555 555 0100"
            className={fieldClass}
          />
          <p className={hintClass}>{WHATSAPP_OWNER_PHONE_HINT_ANDROID}</p>
          <FieldStatus message={errors.ownerPhone} />
        </StackedField>
      ) : null}

      {whatsappShowsContactsDb(method) ? (
        <StackedField label="Contacts database" optional>
          <PathPicker
            value={props.whatsappWa}
            onChange={props.onWhatsappWaChange}
            filters={WHATSAPP_CONTACTS_FILTERS}
          />
          <p className={hintClass}>
            {method === "whatsapp-ios"
              ? WHATSAPP_CONTACTS_HINT_IPHONE
              : WHATSAPP_CONTACTS_HINT_ANDROID}
          </p>
          <FieldStatus message={errors.contactsDb} />
        </StackedField>
      ) : null}

      {whatsappShowsMedia(method) ? (
        <StackedField label="Media directory" optional>
          <PathPicker
            value={props.whatsappMedia}
            onChange={props.onWhatsappMediaChange}
            directory
          />
          <p className={hintClass}>{WHATSAPP_MEDIA_HINT}</p>
          <FieldStatus message={errors.media} />
        </StackedField>
      ) : null}

      {whatsappShowsDb(method) ? (
        <StackedField label="Message database" optional>
          <PathPicker
            value={props.whatsappDb}
            onChange={props.onWhatsappDbChange}
            filters={SQLITE_DB_FILTERS}
          />
          <p className={hintClass}>{WHATSAPP_DB_HINT}</p>
          <FieldStatus message={errors.db} />
        </StackedField>
      ) : null}

      {whatsappShowsBusiness(method) ? (
        <Checkbox
          labelClassName="mb-[1.1rem] flex text-[0.875rem]"
          checked={props.isBusinessApp}
          onChange={props.onIsBusinessAppChange}
        >
          WhatsApp Business
        </Checkbox>
      ) : null}

      {props.attachmentFields}
    </>
  );
}

/**
 * The holder's WhatsApp number as a fallback, for an iPhone backup that may
 * carry it itself. The Import form shows it under Processing Options.
 */
export function WhatsappFallbackPhoneField(props: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <StackedField label={WHATSAPP_OWNER_PHONE_LABEL} optional>
      <input
        type="text"
        inputMode="tel"
        aria-label={`${WHATSAPP_OWNER_PHONE_LABEL} (Optional)`}
        value={props.value}
        onChange={(e) => props.onChange(e.target.value)}
        placeholder="+1 555 555 0100"
        className={fieldClass}
      />
      <p className={hintClass}>{WHATSAPP_OWNER_PHONE_HINT_IPHONE}</p>
    </StackedField>
  );
}
