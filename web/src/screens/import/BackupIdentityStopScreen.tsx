import Button from "../../components/Button";
import { type IdentityType, identityOnProfile } from "../../lib/backupIdentity";
import BackupIdentityList from "./BackupIdentityList";

/**
 * Shown before the Import Run is created, when a probe of the backup
 * found identities and none is on the account's profile. A mismatch has no
 * mechanical consequence — attribution runs on the importing account and
 * Apple's own from-me flag either way — so the list is the information and
 * the decision is the click: Continue, or Cancel back to the form. Adding
 * an address re-runs the comparison live (the marks and heading derive
 * from the profile prop), so claiming the device's address resolves the
 * mismatch in place.
 */
export default function BackupIdentityStopScreen({
  identities,
  profile,
  onAdd,
  onContinue,
  onCancel,
  busy,
  error,
}: {
  identities: string[];
  profile: { phones: string[]; emails: string[] } | null;
  onAdd: (value: string, type: IdentityType) => Promise<void>;
  onContinue: () => void;
  onCancel: () => void;
  busy?: boolean;
  /** Set after an "Add to profile" call fails or silently didn't add the
   * address — forwarded straight to the list, which renders it under the
   * rows it belongs next to. */
  error?: string | null;
}) {
  const matched =
    profile != null && identities.some((identity) => identityOnProfile(identity, profile));

  return (
    <>
      <h1 className="m-0 mb-1 text-2xl font-bold">
        {matched
          ? "An address this backup sent from is on your profile."
          : "None of the addresses this backup sent from are on your profile."}
      </h1>
      <p className="m-0 mb-5 text-[0.875rem] text-muted">
        These are the addresses the backup's device sent messages from.
      </p>

      <BackupIdentityList
        identities={identities}
        profile={profile}
        onAdd={onAdd}
        busy={busy}
        error={error}
      />

      <div className="mt-5 flex items-center gap-3">
        <Button variant="primary" size="wide" onClick={onContinue} disabled={busy}>
          Continue import
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
      </div>
    </>
  );
}
