import Checkbox from "../../../components/Checkbox";
import PasswordField from "../../../components/PasswordField";
import PathPicker from "../../../components/PathPicker";
import { textInputClass } from "../../../components/TextField";
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
} from "../../../lib/whatsappImport";
import { hintClass, StackedField } from "../ImportFormUi";
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
  const { form, dispatch, errors } = props;
  const method = form.source;
  if (!isWhatsappMethod(method)) return null;
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
            value={form.whatsappKey}
            onChange={(value) => dispatch({ type: "set", field: "whatsappKey", value })}
            autoComplete="new-password"
            showPassword={form.showWhatsappKey}
            onToggle={() => dispatch({ type: "toggle", field: "showWhatsappKey" })}
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
            value={form.backupPassword}
            onChange={(value) => dispatch({ type: "set", field: "backupPassword", value })}
            autoComplete="new-password"
            showPassword={form.showBackupPassword}
            onToggle={() => dispatch({ type: "toggle", field: "showBackupPassword" })}
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
            value={form.whatsappOwnerPhone}
            onChange={(e) =>
              dispatch({ type: "set", field: "whatsappOwnerPhone", value: e.target.value })
            }
            placeholder="+1 555 555 0100"
            className={textInputClass}
          />
          <p className={hintClass}>{WHATSAPP_OWNER_PHONE_HINT_ANDROID}</p>
          <FieldStatus message={errors.ownerPhone} />
        </StackedField>
      ) : null}

      {whatsappShowsContactsDb(method) ? (
        <StackedField label="Contacts database" optional>
          <PathPicker
            value={form.whatsappWa}
            onChange={(path) => props.onPathChange("whatsappWa", path)}
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
            value={form.whatsappMedia}
            onChange={(path) => props.onPathChange("whatsappMedia", path)}
            directory
          />
          <p className={hintClass}>{WHATSAPP_MEDIA_HINT}</p>
          <FieldStatus message={errors.media} />
        </StackedField>
      ) : null}

      {whatsappShowsDb(method) ? (
        <StackedField label="Message database" optional>
          <PathPicker
            value={form.whatsappDb}
            onChange={(path) => props.onPathChange("whatsappDb", path)}
            filters={SQLITE_DB_FILTERS}
          />
          <p className={hintClass}>{WHATSAPP_DB_HINT}</p>
          <FieldStatus message={errors.db} />
        </StackedField>
      ) : null}

      {whatsappShowsBusiness(method) ? (
        <Checkbox
          labelClassName="mb-[1.1rem] flex text-[0.875rem]"
          checked={form.isBusinessApp}
          onChange={(value) => dispatch({ type: "set", field: "isBusinessApp", value })}
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
        className={textInputClass}
      />
      <p className={hintClass}>{WHATSAPP_OWNER_PHONE_HINT_IPHONE}</p>
    </StackedField>
  );
}
