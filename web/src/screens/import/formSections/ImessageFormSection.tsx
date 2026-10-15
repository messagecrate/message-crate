import PasswordField from "../../../components/PasswordField";
import PathPicker from "../../../components/PathPicker";
import {
  imessageAttachmentRootRequired,
  imessageShowsAppleContacts,
  imessageShowsAttachmentRoot,
  imessageShowsPassword,
  isImessageMethod,
} from "../../../lib/imessageImport";
import { hintClass, StackedField } from "../ImportFormUi";
import { FieldStatus } from "./FieldStatus";
import type { ImportFormSectionProps } from "./types";

const APPLE_CONTACTS_FILTERS = [{ name: "Apple AddressBook", extensions: ["abcddb", "sqlitedb"] }];

const ATTACHMENT_DIRECTORY_HINT_MAC =
  "Leave empty if Attachments and StickerCache are next to chat.db. Set this only when those directories live somewhere else.";
const ATTACHMENT_DIRECTORY_HINT_JAILBREAK = "Directory that contains Attachments and StickerCache.";
const APPLE_CONTACTS_HINT_MAC =
  "Default: use the local AddressBook. Pick AddressBook-v22.abcddb or AddressBook.sqlitedb only if that file is not in the usual Contacts location.";
const APPLE_CONTACTS_HINT_JAILBREAK =
  "AddressBook-v22.abcddb or AddressBook.sqlitedb. A local Mac AddressBook scan will not find a phone copy.";

/** The encryption password, attachment directory, Apple Contacts file, and attachments. */
export default function ImessageFormSection(props: ImportFormSectionProps) {
  const { form, dispatch, errors } = props;
  const method = form.source;
  if (!isImessageMethod(method)) return null;
  return (
    <>
      {imessageShowsPassword(method) ? (
        <StackedField
          label="Encryption password"
          required={props.pathStats.backupEncrypted === true}
          optional={props.pathStats.backupEncrypted !== true}
        >
          <PasswordField
            aria-label={
              props.pathStats.backupEncrypted === true
                ? "Encryption password"
                : "Encryption password (Optional)"
            }
            value={form.backupPassword}
            onChange={(value) => dispatch({ type: "set", field: "backupPassword", value })}
            autoComplete="new-password"
            showPassword={form.showBackupPassword}
            onToggle={() =>
              dispatch({
                type: "set",
                field: "showBackupPassword",
                value: !form.showBackupPassword,
              })
            }
          />
          <FieldStatus message={errors.backupPassword} />
        </StackedField>
      ) : null}

      {imessageShowsAttachmentRoot(method) ? (
        <StackedField
          label="Attachment directory"
          required={imessageAttachmentRootRequired(method)}
          optional={!imessageAttachmentRootRequired(method)}
        >
          <PathPicker
            value={form.attachmentRoot}
            onChange={(path) => props.onPathChange("attachmentRoot", path)}
            directory
          />
          <p className={hintClass}>
            {method === "imessage-macos"
              ? ATTACHMENT_DIRECTORY_HINT_MAC
              : ATTACHMENT_DIRECTORY_HINT_JAILBREAK}
          </p>
          <FieldStatus message={errors.attachmentRoot} />
        </StackedField>
      ) : null}

      {imessageShowsAppleContacts(method) ? (
        <StackedField label="Apple Contacts file" optional>
          <PathPicker
            value={form.appleContacts}
            onChange={(path) => props.onPathChange("appleContacts", path)}
            filters={APPLE_CONTACTS_FILTERS}
          />
          <p className={hintClass}>
            {method === "imessage-macos" ? APPLE_CONTACTS_HINT_MAC : APPLE_CONTACTS_HINT_JAILBREAK}
          </p>
          <FieldStatus message={errors.appleContacts} />
        </StackedField>
      ) : null}

      {props.attachmentFields}
    </>
  );
}
