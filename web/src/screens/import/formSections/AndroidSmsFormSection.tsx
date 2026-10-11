import { Link } from "react-router-dom";
import Checkbox from "../../../components/Checkbox";
import PhoneTokenField from "../../../components/PhoneTokenField";
import { textInputClass } from "../../../components/TextField";
import { accentLinkClass } from "../../../lib/uiStyles";
import { hintClass, StackedField } from "../ImportFormUi";
import type { ImportFormSectionProps } from "./types";

/**
 * The attachments, the backup device's phone numbers, and its email
 * addresses for SMS Backup+. One component for the three sources, so the
 * number being typed stays when the source changes among them.
 */
export default function AndroidSmsFormSection(props: ImportFormSectionProps) {
  const entry = props.ownerPhoneEntry;
  return (
    <>
      {props.attachmentFields}

      <StackedField label="Backup Device Phone Numbers" required>
        <PhoneTokenField
          ref={entry.fieldRef}
          value={props.ownerPhones}
          onChange={props.onOwnerPhonesChange}
          onDraftChange={entry.onDraftChange}
          aria-label="Backup Device Phone Numbers"
        />
        <p className={hintClass}>
          Pre-filled from your profile. Add numbers from other SIMs, if needed.
        </p>
        {props.showMissingAccountPhoneWarning ? (
          <div
            role="status"
            className="mt-2 rounded-lg border border-warn-soft-border bg-warn-soft-bg px-3 py-2 text-[0.8125rem] text-warn-soft-text"
          >
            Your user profile is missing a phone number. Add one in{" "}
            <Link to="/settings?tab=profile" className={`${accentLinkClass} text-[0.8125rem]`}>
              Settings → Profile
            </Link>{" "}
            so import can tell which messages you sent.
          </div>
        ) : null}
        {entry.mismatch && !entry.mismatchAck && entry.phonesForMatch.length > 0 ? (
          <div
            role="status"
            className="mt-2 rounded-lg border border-warn-soft-border bg-warn-soft-bg px-3 py-2 text-[0.8125rem] text-warn-soft-text"
          >
            I understand none of the entered phone numbers match my profile and that imported
            messages will not be linked to my account.
          </div>
        ) : null}
        <Checkbox
          labelClassName="mt-2 flex items-start text-[0.8125rem]"
          className="mt-0.5 shrink-0"
          checked={entry.mismatchAck}
          onChange={entry.onMismatchAckChange}
        >
          <span>Allow import from phone numbers not on my profile.</span>
        </Checkbox>
      </StackedField>

      {props.descriptor.asksOwnerEmails ? (
        <StackedField label="Backup Device Email Addresses" required>
          <input
            type="text"
            inputMode="email"
            aria-label="Backup Device Email Addresses"
            value={props.ownerEmails}
            onChange={(e) => props.onOwnerEmailsChange(e.target.value)}
            placeholder="you@example.com"
            className={textInputClass}
          />
          <p className={hintClass}>
            Pre-filled from your profile. The Gmail or IMAP account SMS Backup+ synced to; separate
            several with commas.
          </p>
        </StackedField>
      ) : null}
    </>
  );
}
