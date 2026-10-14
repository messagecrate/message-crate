import { useEffect, useRef, useState } from "react";
import { textInputClass } from "../../components/TextField";
import TimeZoneField from "../../components/TimeZoneField";
import { fixedSettings } from "../../lib/account";
import { errorLine } from "../../lib/apiErrorMessage";
import { useSettingsAccount, useUpdateSettingsProfile } from "../../lib/useSettingsAccount";
import { AccountActivitySection } from "./AccountActivitySection";
import { AddressBookSection } from "./AddressBookSection";
import { IdentitiesSection } from "./IdentitiesSection";
import { sectionTitleClass } from "./profileStyles";

/**
 * Profile settings: display name, time zone, identities, address book.
 *
 * Given `managedAccountId`, the account is one the owner opened from
 * User Accounts. The owner sets its name, zone and identities as the holder
 * does, and reads when it last logged in and which app it connects with. The
 * address book is the account's contacts, which the owner does not reach.
 */
export function ProfileSettingsPanel({ managedAccountId }: { managedAccountId?: number }) {
  const { profile, loading, error: loadError } = useSettingsAccount(managedAccountId);
  const updateProfile = useUpdateSettingsProfile(managedAccountId);
  const managed = managedAccountId !== undefined;
  // What the person has typed and not yet saved; null shows the stored name.
  const [nameDraft, setNameDraft] = useState<string | null>(null);
  const [nameError, setNameError] = useState("");
  const [zoneError, setZoneError] = useState("");
  const [zoneSaving, setZoneSaving] = useState(false);
  // The name a save has sent and not yet heard back about, so leaving the
  // field and then the panel cannot send it twice.
  const nameInFlight = useRef<string | null>(null);
  // This render's name save. A tab switch removes the field without a blur,
  // so the panel runs the last render's save as it goes.
  const saveNameOnLeave = useRef<() => Promise<void>>(async () => {});
  useEffect(() => () => void saveNameOnLeave.current(), []);

  if (loadError) {
    return <div className="text-danger">Could not load profile: {loadError}</div>;
  }

  if (loading || !profile) {
    return <div className="text-muted">Loading…</div>;
  }

  const name = nameDraft ?? profile.preferred_name ?? "";
  const fixed = fixedSettings(profile);

  // Leaving the field is the whole gesture, as picking a zone is: no Save
  // button, so a typed name cannot be left behind unsaved (#2985). A name that
  // reads as the stored one sends nothing.
  const handleSaveName = async () => {
    if (nameDraft === null) return;
    const next = nameDraft.trim();
    if (next === (profile.preferred_name ?? "")) {
      setNameDraft(null);
      return;
    }
    if (nameInFlight.current === next) return;
    nameInFlight.current = next;
    setNameError("");
    try {
      await updateProfile.mutateAsync({ preferred_name: next || null });
      setNameDraft(null);
    } catch (e) {
      setNameError(errorLine(e));
    } finally {
      nameInFlight.current = null;
    }
  };
  saveNameOnLeave.current = handleSaveName;

  // Picking a zone is the whole gesture: no Save button, the server answers
  // with the profile as it now stands and every date label re-renders from it.
  const handleChangeZone = async (zone: string) => {
    if (zone === profile.time_zone) return;
    setZoneError("");
    setZoneSaving(true);
    try {
      await updateProfile.mutateAsync({ time_zone: zone });
    } catch (e) {
      setZoneError(errorLine(e));
    } finally {
      setZoneSaving(false);
    }
  };

  return (
    <div>
      <h3 className={sectionTitleClass}>Display Name</h3>
      <div className="mb-[0.35rem] flex items-center gap-2">
        <input
          type="text"
          aria-label="Display name"
          value={name}
          readOnly={fixed.displayName}
          onChange={(e) => setNameDraft(e.target.value)}
          onBlur={() => void handleSaveName()}
          // Enter leaves the field, so the save runs once, from the blur.
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          className={`${textInputClass} flex-1${fixed.displayName ? " !text-muted" : ""}`}
        />
      </div>
      {nameError && <div className="mb-6 text-[0.813rem] text-danger">{nameError}</div>}
      {!nameError && <div className="mb-6" />}

      <h3 className={sectionTitleClass}>Time Zone</h3>
      <TimeZoneField
        value={profile.time_zone}
        onChange={(zone) => void handleChangeZone(zone)}
        isDisabled={fixed.timeZone || zoneSaving}
        className="mb-[0.35rem] max-w-[28rem]"
      />
      <div className="text-[0.813rem] text-muted">
        {fixed.displayName && fixed.timeZone
          ? "Message times are shown in this zone. The Demo Account's display name and time zone are fixed."
          : "Message times are shown in this zone."}
      </div>
      {zoneError && <div className="mb-6 text-[0.813rem] text-danger">{zoneError}</div>}
      {!zoneError && <div className="mb-6" />}

      {/* The owner holds no messages: it has no handles to match a sender
          against and no contacts, so neither section is its to fill in. */}
      {profile.is_owner ? null : (
        <>
          <IdentitiesSection profile={profile} managedAccountId={managedAccountId} />
          {managed && <AccountActivitySection profile={profile} />}
          {!managed && !fixed.addressBook && <AddressBookSection />}
        </>
      )}
    </div>
  );
}
