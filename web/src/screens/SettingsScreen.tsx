import { TabList, TabPanel, Tabs } from "react-aria-components";
import { Link, useLocation, useSearchParams } from "react-router-dom";
import Tab from "../components/Tab";
import { canUseConvert } from "../lib/desktopFeatures";
import { parseSelectKey } from "../lib/selectKey";
import { isTauri } from "../lib/tauriCheck";
import { AccountSettingsPanel } from "./settings/AccountSettingsPanel";
import { AppearanceSection } from "./settings/AppearanceSection";
import { AuditTrailSection } from "./settings/AuditTrailSection";
import { ConvertSection } from "./settings/ConvertSection";
import { NewAccountPanel } from "./settings/NewAccountPanel";
import { ProfileSettingsPanel } from "./settings/ProfileSettingsPanel";
import { StorageSection } from "./settings/StorageSection";
import { SystemSection } from "./settings/SystemSection";
import { useNameEdit } from "./settings/useNameEdit";
import { useSettingsAccount } from "./settings/useSettingsAccount";

const ALL_TABS = [
  "account",
  "profile",
  "storage",
  "audit-trail",
  "system",
  "convert",
  "appearance",
] as const;
type SettingsTab = (typeof ALL_TABS)[number];

const TAB_LABELS: Record<SettingsTab, string> = {
  account: "Account",
  profile: "Profile",
  storage: "Storage",
  "audit-trail": "Audit Trail",
  system: "System",
  convert: "Convert",
  appearance: "Appearance",
};

/** System, Convert and Appearance are this device's, not an account's. */
const DEVICE_TABS: readonly SettingsTab[] = ["system", "convert", "appearance"];

/** Tabs about messages, which the administrator does not hold. */
const ADMIN_HIDDEN_TABS: readonly SettingsTab[] = ["storage", "system", "convert"];

/**
 * Tabs this person can open, in display order.
 *
 * - Convert is a desktop-only tool: it runs `reexport` in the desktop
 *   process, so a browser visiting the website never sees it.
 * - An account the administrator opened from User Accounts has the tabs that
 *   are the account's. The device tabs would change the administrator's own browser,
 *   so they are in the administrator's own Settings only.
 * - The administrator holds no messages, so its own Settings have no Storage,
 *   and none of the tools that work on messages: System and Convert.
 */
function visibleTabs(isDesktop: boolean, managed: boolean, isAdmin: boolean): SettingsTab[] {
  return ALL_TABS.filter((id) => {
    if (managed && DEVICE_TABS.includes(id)) return false;
    if (isAdmin && !managed && ADMIN_HIDDEN_TABS.includes(id)) return false;
    if (id === "convert") return canUseConvert(isDesktop);
    return true;
  });
}

/** "User Settings: bob (Bob Smith)", the name in plain weight, or without one "User Settings: bob". */
function managedHeading(username: string, preferredName: string | null | undefined) {
  const name = preferredName?.trim() ?? "";
  return (
    <>
      User Settings: {username}
      {name ? (
        <>
          {" "}
          <span className="font-normal">({name})</span>
        </>
      ) : null}
    </>
  );
}

function tabFromSearchParam(raw: string | null, allowed: readonly SettingsTab[]): SettingsTab {
  return parseSelectKey(raw, allowed) ?? "account";
}

/** A new account has an Account section to fill in; the rest waits for the account. */
const NEW_ACCOUNT_DISABLED_TABS: readonly SettingsTab[] = ["profile", "storage", "audit-trail"];

/**
 * Settings for the logged-in account, or, given `managedAccountId`, for an
 * account the administrator opened from User Accounts. The same screen and the
 * same tabs either way, so the administrator sees an account's settings laid out as
 * the account holder does.
 *
 * Given `creating`, the account is one the administrator is adding. It has the tabs a
 * managed account has, so the administrator sees what the account will hold, but only
 * Account opens: a profile and storage belong to an account that exists.
 * Creating it opens that account's Settings, with every tab.
 *
 * Given `backToAccounts`, the screen was opened from User Accounts and carries
 * a link back to it above the heading, which says whose Settings these are.
 * Admin Home sets it for every account it opens, the administrator's own included.
 */
export default function SettingsScreen({
  managedAccountId,
  creating = false,
  backToAccounts = false,
}: {
  managedAccountId?: number;
  creating?: boolean;
  backToAccounts?: boolean;
}) {
  const [searchParams, setSearchParams] = useSearchParams();
  const { pathname } = useLocation();
  const { profile } = useSettingsAccount(managedAccountId);
  const managed = managedAccountId !== undefined;
  // Held here, not in the Profile panel, so a tab switch keeps a typed name
  // and the server's refusal of it. A refusal that arrives after this screen
  // has gone names the account and the Profile to go back to.
  const nameEdit = useNameEdit({
    profilePath: `${pathname}?tab=profile`,
    username: managed ? (profile?.username ?? null) : null,
  });
  const tabs = creating
    ? visibleTabs(isTauri(), true, false)
    : visibleTabs(isTauri(), managed, profile?.is_admin === true);
  const tab = creating ? "account" : tabFromSearchParam(searchParams.get("tab"), tabs);

  return (
    <div className="max-w-[820px] p-4 text-text sm:p-6">
      <header>
        {backToAccounts ? (
          <Link
            to="/admin/accounts"
            className="mb-2 inline-block text-[0.813rem] text-muted no-underline hover:text-text"
          >
            ← User Accounts
          </Link>
        ) : null}
        <h2 className="m-0 text-text">
          {creating
            ? "New account"
            : managed
              ? // Blank until the account is read, so the administrator's wording never shows first.
                profile
                ? managedHeading(profile.username, profile.preferred_name)
                : "\u00a0"
              : backToAccounts
                ? "Settings for Administrator"
                : "Settings"}
        </h2>
      </header>

      <Tabs
        disabledKeys={creating ? NEW_ACCOUNT_DISABLED_TABS : undefined}
        selectedKey={tab}
        onSelectionChange={(key) => {
          const next = parseSelectKey(key, tabs);
          if (!next) return;
          const params = new URLSearchParams(searchParams);
          params.set("tab", next);
          setSearchParams(params, { replace: true });
        }}
      >
        <TabList
          aria-label="Settings sections"
          className="relative mt-5 flex flex-wrap gap-x-1 border-b border-border"
        >
          {tabs.map((id) => (
            <Tab key={id} id={id} className="text-[0.813rem]">
              {TAB_LABELS[id]}
            </Tab>
          ))}
        </TabList>

        <TabPanel id="account" className="mt-6">
          {creating ? (
            <NewAccountPanel />
          ) : (
            <AccountSettingsPanel managedAccountId={managedAccountId} />
          )}
        </TabPanel>
        <TabPanel id="profile" className="mt-6">
          <ProfileSettingsPanel managedAccountId={managedAccountId} nameEdit={nameEdit} />
        </TabPanel>
        {tabs.includes("storage") ? (
          <TabPanel id="storage" className="mt-6">
            <StorageSection managedAccountId={managedAccountId} />
          </TabPanel>
        ) : null}
        <TabPanel id="audit-trail" className="mt-6">
          <AuditTrailSection managedAccountId={managedAccountId} />
        </TabPanel>
        {tabs.includes("system") ? (
          <TabPanel id="system" className="mt-6">
            <SystemSection />
          </TabPanel>
        ) : null}
        {tabs.includes("convert") ? (
          <TabPanel id="convert" className="mt-6">
            <ConvertSection />
          </TabPanel>
        ) : null}
        {tabs.includes("appearance") ? (
          <TabPanel id="appearance" className="mt-6">
            <AppearanceSection />
          </TabPanel>
        ) : null}
      </Tabs>
    </div>
  );
}
