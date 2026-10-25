import { textInputClass } from "../../components/TextField";
import { fixedSettings } from "../../lib/account";
import { AccountActivitySection } from "./AccountActivitySection";
import { AccountPermissionsSection } from "./AccountPermissionsSection";
import { ApiTokensSection } from "./ApiTokensSection";
import { ChangePasswordSection } from "./ChangePasswordSection";
import { ManagedApiTokensSection } from "./ManagedApiTokensSection";
import { ProfileDangerZone } from "./ProfileDangerZone";
import { sectionTitleClass } from "./profileStyles";
import { useSettingsAccount } from "./useSettingsAccount";

/**
 * Account settings: username, last login and app (an account the administrator
 * opened), password, status, permissions, API tokens, danger zone.
 *
 * Given `managedAccountId`, the account is one the administrator opened from
 * User Accounts. The administrator reads when it last logged in and which app it
 * connects with, and sees that account's API tokens without their
 * secrets and revokes them, so a leaked token can be ended; making and
 * renaming one are the holder's. The administrator's own account has no tokens and
 * cannot be deleted, so it has neither section.
 */
export function AccountSettingsPanel({ managedAccountId }: { managedAccountId?: number }) {
  const { profile, loading, error: loadError } = useSettingsAccount(managedAccountId);

  if (loadError) {
    return <div className="text-danger">Could not load account: {loadError}</div>;
  }

  if (loading || !profile) {
    return <div className="text-muted">Loading…</div>;
  }

  const managed = managedAccountId !== undefined;
  const isAdmin = profile.is_admin === true;
  const fixed = fixedSettings(profile);

  return (
    <div>
      <h3 className={sectionTitleClass}>Username</h3>
      <div className="mb-6 max-w-[360px]">
        <input
          type="text"
          value={profile.username}
          readOnly
          className={`${textInputClass} !text-muted`}
        />
      </div>
      {managed && <AccountActivitySection profile={profile} />}
      {/* The administrator cannot be disabled and holds no messages to import, export or delete. */}
      {!isAdmin ? (
        <AccountPermissionsSection profile={profile} managedAccountId={managedAccountId} />
      ) : null}

      {fixed.password ? (
        <>
          <h3 className={sectionTitleClass}>Password</h3>
          <p className="mb-6 mt-0 text-[0.813rem] text-muted">
            The Demo Account never has a password.
          </p>
        </>
      ) : (
        <ChangePasswordSection
          canReset={!isAdmin}
          requireCurrent={isAdmin && !managed}
          managedAccountId={managedAccountId}
        />
      )}

      {!managed && fixed.apiTokens ? (
        <>
          <h3 className={sectionTitleClass}>API Tokens</h3>
          <p className="mb-6 mt-0 text-[0.813rem] text-muted">
            The Demo Account makes no API Tokens.
          </p>
        </>
      ) : !managed && !isAdmin ? (
        <ApiTokensSection
          accountCanImport={profile.can_import ?? true}
          accountCanExport={profile.can_export ?? true}
        />
      ) : null}
      {managedAccountId !== undefined && !isAdmin ? (
        <ManagedApiTokensSection accountId={managedAccountId} />
      ) : null}

      {!isAdmin ? (
        <ProfileDangerZone
          messagesFixed={fixed.deleteMessages}
          accountFixed={fixed.deleteOwnAccount}
          username={profile.username}
          hasPassword={profile.has_password}
          canDelete={profile.can_delete ?? true}
          managedAccountId={managedAccountId}
          messageCount={profile.message_count}
        />
      ) : null}
    </div>
  );
}
