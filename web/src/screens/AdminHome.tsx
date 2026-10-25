import { Suspense, useState } from "react";
import { Navigate, useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom";
import AppHeader from "../components/AppHeader";
import { loadWidth } from "../components/columnResize";
import ErrorBoundary from "../components/ErrorBoundary";
import {
  LEFT_PANEL_DEFAULT_WIDTH,
  LEFT_PANEL_MAX_WIDTH,
  LEFT_PANEL_MIN_WIDTH,
  LEFT_PANEL_STORAGE_KEY,
  leftPanelWindowMaxWidth,
} from "../components/leftPanelWidth";
import { NAV_LEADING_ROW_CLASS } from "../components/navSectionLayout";
import PlainButton from "../components/PlainButton";
import { useAuth } from "../hooks/useAuth";
import { useWindowWidth } from "../hooks/useWindowWidth";
import { parseSelectKey } from "../lib/selectKey";
import { AdminAccountsPanel } from "./admin/AdminAccountsPanel";
import { AdminAuditTrailPanel } from "./admin/AdminAuditTrailPanel";
import { AdminDashboardPanel } from "./admin/AdminDashboardPanel";
import { AdminLogsPanel } from "./admin/AdminLogsPanel";
import { JournalModeNotice } from "./admin/JournalModeNotice";
import { ServerSettingsPanel } from "./admin/ServerSettingsPanel";
import { REQUEST_PARAM } from "./admin/serverLogOfRequest";
import { SettingsScreen } from "./lazySettingsScreen";

/** What the side panel lists, in its order. */
const SECTIONS = ["dashboard", "settings", "accounts", "audit-trail", "logs"] as const;

const SECTION_LABELS: Record<(typeof SECTIONS)[number], string> = {
  dashboard: "Dashboard",
  // Not "Settings": that is the screen an account is managed from, here and in the app.
  settings: "Server Settings",
  accounts: "User Accounts",
  "audit-trail": "Audit Trail",
  logs: "Logs",
};

function sectionLinkClass(active: boolean): string {
  return `${NAV_LEADING_ROW_CLASS} box-border w-full cursor-pointer rounded border-none px-2 py-1.5 text-left text-[0.875rem] text-text hover:bg-hover ${
    active ? "bg-hover font-semibold" : "bg-transparent font-normal"
  }`;
}

/**
 * Admin Home: where the administrator lands at login and works from, the way
 * any other account lands in Messages.
 *
 * The frame is the one every account sees: the header with the product name,
 * a search bar and the account button, over a side panel and a content pane.
 * What fills it is the administrator's own. The administrator has no conversations, no
 * contacts, no import, no export and no trash, so the side panel lists
 * Dashboard, Server Settings, User Accounts, Audit Trail and Logs, and the
 * search bar filters the accounts table. Dashboard shows what the whole
 * database holds, Audit Trail what each user did and when, and Logs the
 * server's log and, in the desktop app, the Import Run logs on this computer.
 *
 * `/admin/accounts/{id}` is one account's Settings, the screen its holder
 * sees, opened from the gear in the account's row. The administrator's own row
 * opens the administrator's own Settings, which is also where the account button's
 * Settings goes. `/admin/accounts/new` is the same screen for an account that
 * does not exist yet, which Add account opens in place of the table. See
 * `docs/adr/0008-the-administrator-holds-no-messages.md`.
 */
export default function AdminHome() {
  const { section: raw, accountId: rawAccountId } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const { accountId: ownAccountId } = useAuth();
  const [searchParams, setSearchParams] = useSearchParams();
  const section = parseSelectKey(raw ?? null, SECTIONS);
  // The width the message shell's side panel was last dragged to, so the
  // product name sits over a panel of the same width on both screens.
  const [navWidth] = useState(() =>
    loadWidth(
      LEFT_PANEL_STORAGE_KEY,
      LEFT_PANEL_DEFAULT_WIDTH,
      LEFT_PANEL_MIN_WIDTH,
      LEFT_PANEL_MAX_WIDTH,
    ),
  );
  // Capped for this window as the message shell's panel is (#1718).
  const navMax = leftPanelWindowMaxWidth(useWindowWidth());

  // `/admin` and any unknown section land on User Accounts, and the address
  // bar says so, so a reload comes back to the same place.
  if (!section) {
    return <Navigate to="/admin/accounts" replace />;
  }

  // `new` in place of an id is the account the administrator is adding.
  const creatingAccount = section === "accounts" && rawAccountId === "new";
  // An id that is not a number names no account; the list is the way back.
  const openAccountId =
    section === "accounts" && rawAccountId && /^\d+$/.test(rawAccountId)
      ? Number(rawAccountId)
      : null;
  if (rawAccountId && openAccountId === null && !creatingAccount) {
    return <Navigate to="/admin/accounts" replace />;
  }

  const accountSearch = searchParams.get("q") || "";

  // The bar searches the accounts table, so typing anywhere else goes to it.
  const handleSearchChange = (q: string) => {
    if (section !== "accounts" || openAccountId !== null || creatingAccount) {
      navigate(`/admin/accounts${q ? `?q=${encodeURIComponent(q)}` : ""}`);
      return;
    }
    // `replace`, so typing does not fill the history with one entry per keystroke.
    setSearchParams(q ? { q } : {}, { replace: true });
  };

  return (
    <div className="flex h-screen flex-col bg-bg font-sans text-text">
      <AppHeader
        search={{ target: "accounts", query: accountSearch }}
        onSearchChange={handleSearchChange}
        onSearch={handleSearchChange}
      />
      <JournalModeNotice />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <nav
          aria-label="Admin Home sections"
          className="flex h-full shrink-0 flex-col gap-0.5 overflow-auto border-r border-border bg-panel px-3 py-2"
          style={{ width: Math.min(navWidth, navMax) }}
        >
          {SECTIONS.map((id) => (
            <PlainButton
              key={id}
              aria-current={id === section ? "page" : undefined}
              className={sectionLinkClass(id === section)}
              onPress={() => navigate(`/admin/${id}`)}
            >
              {SECTION_LABELS[id]}
            </PlainButton>
          ))}
        </nav>

        <main className="min-w-0 flex-1 overflow-auto bg-bg text-text">
          {creatingAccount || openAccountId !== null ? (
            // Settings' code is downloaded when it is first opened. Code that
            // doesn't arrive shows why here, beside the side panel, which
            // stays usable.
            <ErrorBoundary scope="screen" resetKey={location.pathname}>
              <Suspense fallback={null}>
                {openAccountId === null ? (
                  <SettingsScreen key="new" creating backToAccounts />
                ) : (
                  // The administrator's own row is the administrator's own Settings, not a managed account's.
                  <SettingsScreen
                    key={openAccountId}
                    backToAccounts
                    managedAccountId={openAccountId === ownAccountId ? undefined : openAccountId}
                  />
                )}
              </Suspense>
            </ErrorBoundary>
          ) : (
            <div className="max-w-[900px] p-6">
              {section === "dashboard" && <AdminDashboardPanel />}
              {section === "settings" && <ServerSettingsPanel />}
              {section === "accounts" && <AdminAccountsPanel filter={accountSearch} />}
              {section === "audit-trail" && <AdminAuditTrailPanel />}
              {section === "logs" && (
                <AdminLogsPanel request={searchParams.get(REQUEST_PARAM) || undefined} />
              )}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
