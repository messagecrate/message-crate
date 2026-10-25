/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../lib/api";
import { APP_BUILD } from "../lib/build";
import { productVersionOf } from "../lib/buildFormat";
import { clearNameRefusals } from "../lib/nameRefusals";
import { ThemeProvider } from "../lib/ThemeProvider";
import { account } from "../test/apiShapes";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { nth } from "../test/nth";
import { authModule, Providers } from "../test/providers";
import { MIB } from "../test/units";
import { fill, setupUser } from "../test/user";
import AdminHome from "./AdminHome";

const listAccounts = vi.hoisted(() => vi.fn());
const getServerSettings = vi.hoisted(() => vi.fn());
const getDemoAccount = vi.hoisted(() => vi.fn());
const getServerStorage = vi.hoisted(() => vi.fn());
const getServerState = vi.hoisted(() => vi.fn());
const updateServerSettings = vi.hoisted(() => vi.fn());
const updateAccount = vi.hoisted(() => vi.fn());
const setAccountPassword = vi.hoisted(() => vi.fn());
const createAccount = vi.hoisted(() => vi.fn());
const getAccountProfile = vi.hoisted(() => vi.fn());
const getAccount = vi.hoisted(() => vi.fn());
const getAccountStorage = vi.hoisted(() => vi.fn());
const listAccountIdentities = vi.hoisted(() => vi.fn());
const listAccountImports = vi.hoisted(() => vi.fn());
const getAccountImport = vi.hoisted(() => vi.fn());
const listAccountExports = vi.hoisted(() => vi.fn());
const getImportContacts = vi.hoisted(() => vi.fn());
const deleteAccountById = vi.hoisted(() => vi.fn());
const deleteAccountMessages = vi.hoisted(() => vi.fn());
const listAuditTrail = vi.hoisted(() => vi.fn());
const listAccountAuditTrail = vi.hoisted(() => vi.fn());
const listDeletedAccounts = vi.hoisted(() => vi.fn());
const listApiTokens = vi.hoisted(() => vi.fn());
const listServerLogLines = vi.hoisted(() => vi.fn());
const listServerLogFiles = vi.hoisted(() => vi.fn());

vi.mock("../hooks/useAuth", () =>
  authModule({ logout: vi.fn(), updateToken: vi.fn(), accountId: 1 }),
);

vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
  getServerSettings: (...a: unknown[]) => getServerSettings(...a),
  getDemoAccount: (...a: unknown[]) => getDemoAccount(...a),
  getServerStorage: (...a: unknown[]) => getServerStorage(...a),
  getServerState: (...a: unknown[]) => getServerState(...a),
  updateServerSettings: (...a: unknown[]) => updateServerSettings(...a),
  updateAccount: (...a: unknown[]) => updateAccount(...a),
  setAccountPassword: (...a: unknown[]) => setAccountPassword(...a),
  createAccount: (...a: unknown[]) => createAccount(...a),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  getAccount: (...a: unknown[]) => getAccount(...a),
  getAccountStorage: (...a: unknown[]) => getAccountStorage(...a),
  listAccountIdentities: (...a: unknown[]) => listAccountIdentities(...a),
  listAccountImports: (...a: unknown[]) => listAccountImports(...a),
  getAccountImport: (...a: unknown[]) => getAccountImport(...a),
  listAccountExports: (...a: unknown[]) => listAccountExports(...a),
  getImportContacts: (...a: unknown[]) => getImportContacts(...a),
  deleteAccountById: (...a: unknown[]) => deleteAccountById(...a),
  deleteAccountMessages: (...a: unknown[]) => deleteAccountMessages(...a),
  listAuditTrail: (...a: unknown[]) => listAuditTrail(...a),
  listAccountAuditTrail: (...a: unknown[]) => listAccountAuditTrail(...a),
  listDeletedAccounts: (...a: unknown[]) => listDeletedAccounts(...a),
  listApiTokens: (...a: unknown[]) => listApiTokens(...a),
  listServerLogLines: (...a: unknown[]) => listServerLogLines(...a),
  listServerLogFiles: (...a: unknown[]) => listServerLogFiles(...a),
}));

const anAccount = account({
  account_id: 101,
  username: "bob",
  preferred_name: "Bob Archer",
  time_zone: "America/New_York",
  phones: [{ address: "+15555550100", services: ["phone"] }],
  message_count: 1234,
  storage_bytes: 2048,
});

/** The administrator's own row, which leads the list and is account 1, the one logged in. */
const theAdmin = {
  ...anAccount,
  account_id: 1,
  username: "admin",
  preferred_name: null,
  phones: [],
  is_admin: true,
  message_count: 0,
  storage_bytes: 0,
};

// An account's Settings is a lazy chunk, and loading its code is real I/O the
// fake clock doesn't drive. Loaded here first, the chunk lands within a
// `settle()` once a test opens Settings.
beforeAll(async () => {
  await import("./SettingsScreen");
});

beforeEach(() => {
  // A test that leaves a refused name in the field sends it again as its
  // screen unmounts, and that refusal is held for the top of the page, which
  // outlives the test. The next test starts without it.
  clearNameRefusals();
  // jsdom has no matchMedia, and the theme reads the system color scheme from it.
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  listAccounts.mockReset();
  getServerSettings.mockReset();
  getServerStorage.mockReset();
  getServerState.mockReset();
  updateServerSettings.mockReset();
  updateAccount.mockReset();
  setAccountPassword.mockReset();
  createAccount.mockReset();
  getAccountProfile.mockReset();
  getAccount.mockReset();
  getAccountStorage.mockReset();
  listAccountImports.mockReset();
  listAccountIdentities.mockReset();
  getAccountImport.mockReset();
  listAccountExports.mockReset();
  getImportContacts.mockReset();
  deleteAccountById.mockReset();
  deleteAccountMessages.mockReset();
  listAuditTrail.mockReset();
  listAccountAuditTrail.mockReset();
  listDeletedAccounts.mockReset();
  listDeletedAccounts.mockResolvedValue([]);
  listApiTokens.mockReset();
  listApiTokens.mockResolvedValue([]);
  getAccountProfile.mockResolvedValue(theAdmin);
  getAccount.mockResolvedValue(anAccount);
  getAccountStorage.mockResolvedValue({
    total_bytes: 2048,
    attachment_count: 7,
    conversation_count: 12,
    contact_count: 34,
    top_attachments: [],
  });
  listAccountImports.mockResolvedValue({ items: [anImport], total: 1, limit: 40, offset: 0 });
  listAccountIdentities.mockResolvedValue([
    {
      address: "+15555550100",
      service: "phone",
      start_date: "2020-01-01T00:00:00Z",
      end_date: "2020-02-03T00:00:00Z",
      conversations: 2,
      one_to_one_messages: 12,
      group_messages: 30,
    },
  ]);
  getAccountImport.mockResolvedValue(anAccountImportRun);
  listAccountExports.mockResolvedValue({ items: [], total: 0, limit: 40, offset: 0 });
  deleteAccountById.mockResolvedValue(undefined);
  deleteAccountMessages.mockResolvedValue(undefined);
  listAccounts.mockResolvedValue([theAdmin, anAccount]);
  getServerSettings.mockResolvedValue({
    asset_max_bytes: 512 * MIB,
    journal: { mode: "wal", wal_error: null },
  });
  getDemoAccount.mockResolvedValue({ status: "ready", size: null, error: null });
  getServerStorage.mockResolvedValue({
    message_count: 5678,
    conversation_count: 90,
    contact_count: 120,
    attachment_count: 21,
    total_bytes: 3 * MIB,
    database_bytes: 581 * MIB,
    messages_bytes: 400 * MIB,
    fts_bytes: 149 * MIB,
    accounts: [
      {
        account_id: 1,
        username: "admin",
        message_count: 0,
        text_bytes: 0,
        estimated_message_bytes: 0,
      },
      {
        account_id: 101,
        username: "alice",
        message_count: 5000,
        text_bytes: 6 * MIB,
        estimated_message_bytes: 300 * MIB,
      },
      {
        account_id: 102,
        username: "bob",
        message_count: 678,
        text_bytes: 2 * MIB,
        estimated_message_bytes: 100 * MIB,
      },
    ],
  });
  // The server and this app are the same release unless a test says otherwise.
  getServerState.mockResolvedValue({
    state: "ready",
    version: APP_BUILD,
    schema_fingerprint: 1234567890,
  });
  updateServerSettings.mockResolvedValue({
    asset_max_bytes: 512 * MIB,
    journal: { mode: "wal", wal_error: null },
  });
  updateAccount.mockResolvedValue({ ...anAccount, disabled: true });
  setAccountPassword.mockResolvedValue(undefined);
  // A new account has no profile yet, so the row comes back with no name.
  createAccount.mockResolvedValue({
    ...anAccount,
    account_id: 102,
    username: "carol",
    preferred_name: null,
    phones: [],
  });
  startFakeClock();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

function renderHome(entries: string[] = ["/admin/accounts"], { keepUnread = false } = {}) {
  render(
    <ThemeProvider>
      <Providers keepUnread={keepUnread}>
        <MemoryRouter initialEntries={entries}>
          <Routes>
            <Route path="/admin/:section?/:accountId?" element={<AdminHome />} />
          </Routes>
        </MemoryRouter>
      </Providers>
    </ThemeProvider>,
  );
}

function sectionLinks() {
  return within(screen.getByRole("navigation", { name: "Admin Home sections" })).getAllByRole(
    "button",
  );
}

function selectedSection(): string | undefined {
  return sectionLinks().find((b) => b.getAttribute("aria-current") === "page")?.textContent ?? "";
}

/** One Import Run of bob's, as the list answers it. */
const anImport = {
  id: 9,
  source: "imessage",
  status: "completed",
  started_at: "2026-09-01T10:00:00Z",
  finished_at: "2026-09-01T10:05:00Z",
  message_count: 1234,
  attachment_count: 7,
  bytes_uploaded: 2048,
  issue_count: 0,
};

/** The same run in full, which is what opening its row reads. */
const anAccountImportRun = {
  ...anImport,
  tool: "desktop",
  mode: "full",
  stage: "done",
  duration_ms: 300000,
  parse_ms: null,
  attachments_ms: null,
  prepare_ms: null,
  upload_ms: null,
  summary: {},
  issues: [],
  contacts_new: 12,
  contacts_changed: 3,
};

describe("AdminHome", () => {
  it("lists Dashboard, Server Settings, User Accounts, Audit Trail and Logs in the side panel", () => {
    renderHome();

    expect(sectionLinks().map((b) => b.textContent)).toEqual([
      "Dashboard",
      "Server Settings",
      "User Accounts",
      "Audit Trail",
      "Logs",
    ]);
  });

  it("shows what the whole database holds on the Dashboard, as counts and a byte total", async () => {
    renderHome(["/admin/dashboard"]);

    expect(selectedSection()).toBe("Dashboard");
    expect(screen.getByRole("heading", { name: "Dashboard" })).toBeInTheDocument();
    await settle();
    expect(screen.getByRole("heading", { name: "Contents" })).toBeInTheDocument();
    expect(screen.getByText("3.0 MB")).toBeInTheDocument();
    expect(screen.getByText(/5,678 messages, 21 attachments/)).toBeInTheDocument();
    expect(screen.getByText(/90 conversations, 120 contacts/)).toBeInTheDocument();
    expect(listAccounts).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Server Settings" })).not.toBeInTheDocument();
  });

  it("shows the database's size, the messages' share of it and the search index on the Dashboard", async () => {
    renderHome(["/admin/dashboard"]);
    await settle();

    const heading = screen.getByRole("heading", { name: "Database" });
    // The section is the heading's parent; the messages figure repeats in the
    // totals row further down, so the checks stay inside it.
    const section = within(heading.closest("section") as HTMLElement);
    expect(section.getByText(/excludes attachment files/)).toBeInTheDocument();
    expect(section.getByText("581 MB")).toBeInTheDocument();
    expect(section.getByText("Database size")).toBeInTheDocument();
    expect(section.getByText("400 MB")).toBeInTheDocument();
    expect(section.getByText("Messages on disk")).toBeInTheDocument();
    expect(section.getByText("149 MB")).toBeInTheDocument();
    expect(section.getByText("Full-text search index")).toBeInTheDocument();
  });

  it("lists every account's messages, text and estimated size on disk, with a totals row", async () => {
    renderHome(["/admin/dashboard"]);
    await settle();

    expect(screen.getByRole("heading", { name: "Messages by account" })).toBeInTheDocument();
    expect(screen.getByText(/split by each account's share of text/)).toBeInTheDocument();

    const table = screen.getByRole("table", { name: "Messages by account" });
    const rows = within(table).getAllByRole("row");
    const cells = (row: HTMLElement) =>
      within(row)
        .getAllByRole("cell")
        .map((cell) => cell.textContent);
    // A heading row, one row per account in the server's order, and the totals.
    expect(rows).toHaveLength(5);
    expect(cells(nth(rows, 1))).toEqual(["admin", "0", "0 B", "0 B"]);
    expect(cells(nth(rows, 2))).toEqual(["alice", "5,000", "6.0 MB", "300 MB"]);
    expect(cells(nth(rows, 3))).toEqual(["bob", "678", "2.0 MB", "100 MB"]);
    expect(cells(nth(rows, 4))).toEqual(["All accounts", "5,678", "8.0 MB", "400 MB"]);
  });

  it("opens /admin/logs on the server's log, at warnings and up", async () => {
    listAccounts.mockResolvedValue([theAdmin, anAccount]);
    listServerLogFiles.mockResolvedValue([]);
    listServerLogLines.mockResolvedValue({
      items: [
        {
          id: 7,
          time: "2026-10-08T12:00:00.000000Z",
          level: "error",
          text: "the database is locked",
        },
      ],
      limit: 200,
      has_more: false,
    });
    renderHome(["/admin/logs"]);

    expect(selectedSection()).toBe("Logs");
    expect(screen.getByRole("heading", { name: "Logs" })).toBeInTheDocument();
    await settle();
    expect(screen.getByText("the database is locked")).toBeInTheDocument();
    expect(listServerLogLines).toHaveBeenCalledWith(
      expect.objectContaining({ level: "warn" }),
      expect.anything(),
    );
    expect(screen.queryByRole("heading", { name: "Server Settings" })).not.toBeInTheDocument();
  });

  it("opens the server's log on an Audit Trail entry's request, at every level", async () => {
    const user = setupUser();
    listAccounts.mockResolvedValue([theAdmin, anAccount]);
    listServerLogFiles.mockResolvedValue([]);
    listAuditTrail.mockResolvedValue({
      items: [
        {
          id: 4,
          action: "logged_in",
          at: "2026-10-01T09:00:00+00:00",
          actor: "holder",
          account_id: 101,
          username: "bob",
          request_id: "7f3a9c21",
        },
      ],
      total: 1,
      limit: 50,
      offset: 0,
    });
    // The log was rotated since, so it holds none of the request's lines.
    listServerLogLines.mockResolvedValue({ items: [], limit: 200, has_more: false });
    renderHome(["/admin/audit-trail"]);
    await settle();

    await user.click(screen.getByRole("link", { name: "View in log" }));
    await settle();

    expect(selectedSection()).toBe("Logs");
    expect(screen.getByRole("textbox", { name: "Search the log" })).toHaveValue("7f3a9c21");
    expect(screen.getByRole("button", { name: /^Everything/ })).toBeInTheDocument();
    expect(listServerLogLines).toHaveBeenLastCalledWith(
      expect.objectContaining({ level: undefined, text: "7f3a9c21" }),
      expect.anything(),
    );
    expect(
      screen.getByText(
        "The server's log holds no line of this request. Either its oldest files were deleted as the log grew, or the log's level left the request's lines out.",
      ),
    ).toBeInTheDocument();
  });

  it("lists every account's Audit Trail, a deleted account's under its old username, and narrows it to one account", async () => {
    const user = setupUser();
    listAccounts.mockResolvedValue([theAdmin, anAccount]);
    listAuditTrail.mockResolvedValue({
      items: [
        {
          id: 9,
          action: "account_deleted",
          at: "2026-10-02T10:00:00+00:00",
          actor: "admin",
          account_id: null,
          account_deleted: true,
          username: "carol",
        },
        {
          id: 4,
          action: "logged_in",
          at: "2026-10-01T09:00:00+00:00",
          actor: "holder",
          account_id: 101,
          username: "bob",
          app: "website",
          app_build: "0.10.0+aaaa1111",
        },
      ],
      total: 2,
      limit: 50,
      offset: 0,
    });
    listAccountAuditTrail.mockResolvedValue({
      items: [
        {
          id: 4,
          action: "logged_in",
          at: "2026-10-01T09:00:00+00:00",
          actor: "holder",
          account_id: 101,
          username: "bob",
        },
      ],
      total: 1,
      limit: 50,
      offset: 0,
    });
    renderHome(["/admin/audit-trail"]);

    expect(selectedSection()).toBe("Audit Trail");
    await settle();
    const table = screen.getByRole("table");
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(within(nth(rows, 1)).getByText("carol")).toBeInTheDocument();
    expect(within(nth(rows, 1)).getByText("deleted")).toBeInTheDocument();
    expect(within(nth(rows, 1)).getByText("Account deleted")).toBeInTheDocument();
    expect(within(nth(rows, 1)).getByText("Administrator")).toBeInTheDocument();
    expect(
      within(nth(rows, 2)).getByText("Logged in from the website (0.10.0+aaaa1111)"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Every account/ }));
    await settle();
    await user.click(screen.getByRole("option", { name: "bob" }));
    await settle();
    expect(listAccountAuditTrail).toHaveBeenCalled();
    expect(nth(listAccountAuditTrail.mock.calls, 0)[2]).toBe(101);
    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(2);
  });

  it("lists each deleted account below the live ones, and narrows the Audit Trail to one", async () => {
    const user = setupUser();
    listAccounts.mockResolvedValue([theAdmin, anAccount]);
    listDeletedAccounts.mockResolvedValue([
      { id: 31, username: "carol", deleted_at: "2026-10-03T10:00:00+00:00" },
      { id: 12, username: "carol", deleted_at: "2026-09-01T10:00:00+00:00" },
    ]);
    const carolsEntry = {
      id: 31,
      action: "account_deleted",
      at: "2026-10-03T10:00:00+00:00",
      actor: "admin",
      account_id: null,
      account_deleted: true,
      username: "carol",
    };
    listAuditTrail.mockImplementation(async (params: { deleted_account_id?: number }) => ({
      items: params.deleted_account_id
        ? [carolsEntry]
        : [
            carolsEntry,
            {
              id: 4,
              action: "logged_in",
              at: "2026-10-01T09:00:00+00:00",
              actor: "holder",
              account_id: 101,
              username: "bob",
            },
          ],
      total: params.deleted_account_id ? 1 : 2,
      limit: 50,
      offset: 0,
    }));
    renderHome(["/admin/audit-trail"]);
    await settle();
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(listDeletedAccounts).toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /Every account/ }));
    await settle();
    const options = screen.getAllByRole("option").map((option) => option.textContent);
    expect(options).toHaveLength(5);
    expect(options.slice(0, 3)).toEqual(["Every account", "admin", "bob"]);
    expect(options[3]).toMatch(/^carol, deleted .*2026/);
    expect(options[4]).toMatch(/^carol, deleted .*2026/);
    expect(options[3]).not.toBe(options[4]);
    expect(screen.getByText("Deleted accounts")).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: options[3] ?? "" }));
    await settle();

    expect(listAuditTrail).toHaveBeenCalledWith(
      expect.objectContaining({ deleted_account_id: 31, offset: 0 }),
      expect.anything(),
    );
    expect(listAccountAuditTrail).not.toHaveBeenCalled();
    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(2);
    expect(screen.getByRole("button", { name: /carol, deleted/ })).toBeInTheDocument();
  });

  it("tells apart two accounts of one username deleted in the same minute", async () => {
    const user = setupUser();
    listAccounts.mockResolvedValue([theAdmin]);
    listDeletedAccounts.mockResolvedValue([
      { id: 31, username: "demo", deleted_at: "2026-10-03T10:00:30+00:00" },
      { id: 12, username: "demo", deleted_at: "2026-10-03T10:00:10+00:00" },
    ]);
    listAuditTrail.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    renderHome(["/admin/audit-trail"]);
    await settle();
    expect(listDeletedAccounts).toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /Every account/ }));
    await settle();
    expect(screen.getByText("Deleted accounts")).toBeInTheDocument();
    const options = screen.getAllByRole("option").map((option) => option.textContent);
    expect(options[2]).toMatch(/^demo, deleted .*\(#31\)$/);
    expect(options[3]).toMatch(/^demo, deleted .*\(#12\)$/);
  });

  it("offers no Deleted accounts section when no account was deleted", async () => {
    const user = setupUser();
    listAccounts.mockResolvedValue([theAdmin, anAccount]);
    listAuditTrail.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    renderHome(["/admin/audit-trail"]);
    await settle();
    expect(listDeletedAccounts).toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /Every account/ }));
    await settle();
    const options = screen.getAllByRole("option").map((option) => option.textContent);
    expect(options).toEqual(["Every account", "admin", "bob"]);
    expect(screen.queryByText("Deleted accounts")).not.toBeInTheDocument();
  });

  it("has the header every account sees: the product name, a search bar, the account button", () => {
    renderHome();

    expect(screen.getByText("Message Crate")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Search accounts" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Account menu" })).toBeInTheDocument();
  });

  it("has nothing that frames messages", () => {
    renderHome();

    // The administrator holds no messages, so nothing that frames messages belongs here.
    expect(screen.queryByRole("combobox", { name: "Search messages" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Message Tags" })).not.toBeInTheDocument();
    expect(screen.queryByText("Conversations")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Export" })).not.toBeInTheDocument();
  });

  it("narrows the accounts table to the usernames the search bar matches", async () => {
    const user = setupUser();
    listAccounts.mockResolvedValue([
      anAccount,
      { ...anAccount, account_id: 102, username: "carol" },
    ]);
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    await fill(user, screen.getByRole("combobox", { name: "Search accounts" }), "CAR");

    expect(screen.getByText("carol")).toBeInTheDocument();
    expect(screen.queryByText("bob")).not.toBeInTheDocument();
    // No search words and no advanced form: a username is all there is to match.
    expect(screen.queryByRole("option", { name: "Advanced search" })).not.toBeInTheDocument();
  });

  it("searches accounts from another section by going to User Accounts", async () => {
    const user = setupUser();
    renderHome(["/admin/settings"]);

    await fill(user, screen.getByRole("combobox", { name: "Search accounts" }), "b");

    expect(selectedSection()).toBe("User Accounts");
    await settle();
    expect(screen.getByText("bob")).toBeInTheDocument();
  });

  it("opens the administrator's Settings from the account button", async () => {
    const user = setupUser();
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Account menu" }));
    await settle();
    await user.click(screen.getByRole("menuitem", { name: "Settings" }));

    // The administrator's own row in User Accounts is the administrator's Settings.
    await settle();
    expect(screen.getByRole("heading", { name: "Settings for Administrator" })).toBeInTheDocument();
    expect(screen.getByText("Change Password")).toBeInTheDocument();
    expect(selectedSection()).toBe("User Accounts");
    // It was opened from User Accounts, so it links back there as any account does.
    expect(screen.getByRole("link", { name: "← User Accounts" })).toBeInTheDocument();
  });

  it("lists the administrator first, with no status or permissions to set", async () => {
    renderHome();
    await settle();

    const rows = screen.getAllByRole("row").slice(1);
    expect(
      within(nth(rows, 0)).getByRole("button", { name: "Settings for admin" }),
    ).toBeInTheDocument();
    expect(within(nth(rows, 0)).getByText("Administrator")).toBeInTheDocument();
    // The administrator cannot be disabled and holds no messages to import, export or delete.
    expect(
      within(nth(rows, 0)).queryByRole("button", { name: /Status of/ }),
    ).not.toBeInTheDocument();
    expect(within(nth(rows, 0)).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(
      within(nth(rows, 1)).getByRole("button", { name: "Settings for bob" }),
    ).toBeInTheDocument();
    // The gear opens the account; the name itself is plain text.
    expect(within(nth(rows, 1)).getByText("bob").closest("button")).toBeNull();
  });

  it("shows an account's preferred name under its username, and searches it too", async () => {
    const user = setupUser();
    renderHome();
    await settle();

    expect(screen.getByText("Bob Archer")).toBeInTheDocument();
    await fill(user, screen.getByRole("combobox", { name: "Search accounts" }), "archer");

    // The header names the logged-in administrator, admin, so the search is checked in the table alone.
    const table = within(screen.getByRole("table"));
    expect(table.getByText("bob")).toBeInTheDocument();
    expect(table.queryByText("admin")).not.toBeInTheDocument();
  });

  it("opens an account's Settings from the gear in its row, with the account's own tabs", async () => {
    const user = setupUser();
    renderHome();
    await settle();

    await user.click(screen.getByRole("button", { name: "Settings for bob" }));
    await settle();

    expect(
      screen.getByRole("heading", { name: "User Settings: bob (Bob Archer)" }),
    ).toBeInTheDocument();
    expect(getAccount).toHaveBeenCalledWith(101, expect.anything());
    // System, Convert and Appearance are this device's, not bob's.
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Account",
      "Profile",
      "Storage",
      "Audit Trail",
    ]);
    // The administrator sees bob's API Tokens, to revoke a leaked one, and makes none.
    expect(screen.getByRole("heading", { name: "API Tokens" })).toBeInTheDocument();
    expect(listApiTokens).toHaveBeenCalledWith(expect.anything(), 101);
    expect(screen.queryByRole("button", { name: "Add" })).not.toBeInTheDocument();
  });

  it("sets an account's display name from its Profile, as its holder does", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("tab", { name: "Profile" }));
    await settle();

    const name = screen.getByLabelText("Display name");
    expect(name).toHaveValue("Bob Archer");
    expect(screen.getByRole("heading", { name: "Identities" })).toBeInTheDocument();
    expect(screen.getByText("+15555550100")).toBeInTheDocument();
    // The address book is the account's contacts, which the administrator does not reach.
    expect(screen.queryByText("Address book")).not.toBeInTheDocument();

    updateAccount.mockResolvedValue({ ...anAccount, preferred_name: "Robert" });
    await user.clear(name);
    await fill(user, name, "Robert");
    // Moving on to another tab is all it takes. There is no Save to forget (#2985).
    await user.click(screen.getByRole("tab", { name: "Account" }));
    await settle();
    expect(updateAccount).toHaveBeenCalledWith(101, { preferred_name: "Robert" });
  });

  it("shows a display name refused after a tab switch when the administrator comes back to Profile", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("tab", { name: "Profile" }));
    await settle();

    updateAccount.mockRejectedValue(new ApiError(400, "display name is too long"));
    const name = screen.getByLabelText("Display name");
    await user.clear(name);
    await fill(user, name, "Robert");
    // The refusal arrives after the Profile panel is gone.
    await user.click(screen.getByRole("tab", { name: "Account" }));
    await settle();
    expect(updateAccount).toHaveBeenCalledWith(101, { preferred_name: "Robert" });

    await user.click(screen.getByRole("tab", { name: "Profile" }));
    await settle();
    expect(screen.getByLabelText("Display name")).toHaveValue("Robert");
    expect(screen.getByText("Display name is too long")).toBeInTheDocument();
  });

  it("removes an identity from an account's Profile once the administrator agrees", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("tab", { name: "Profile" }));
    await settle();
    expect(screen.getByText("+15555550100")).toBeInTheDocument();

    updateAccount.mockResolvedValue({ ...anAccount, phones: [] });
    getAccount.mockResolvedValue({ ...anAccount, phones: [] });
    listAccountIdentities.mockResolvedValue([]);
    // Remove asks first; the identity goes only once the dialog agrees.
    await user.click(screen.getByRole("button", { name: "Remove +15555550100 (Text Message)" }));
    await settle();
    expect(updateAccount).not.toHaveBeenCalledWith(
      101,
      expect.objectContaining({ remove_identities: expect.anything() }),
    );
    const dialog = screen.getByRole("dialog", { name: "Remove identity?" });
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    await settle();
    expect(updateAccount).toHaveBeenCalledWith(101, {
      remove_identities: [{ address: "+15555550100", service: "phone" }],
    });
    expect(screen.getByText("No identities yet")).toBeInTheDocument();
  });

  it("shows an account's last login and its app on Account, marking another release", async () => {
    getServerState.mockResolvedValue({
      state: "ready",
      version: "0.10.0+343fe0d8",
      schema_fingerprint: 1234567890,
    });
    getAccount.mockResolvedValue({
      ...anAccount,
      last_login_at: "2026-09-01T10:00:00Z",
      app: "desktop",
      app_build: "0.9.0+aaaa1111",
    });
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    // Last Login and App are facts about the account, on its Account tab (#2988).
    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
    const lastLogin = screen.getByRole("heading", { name: "Last Login" });
    expect(lastLogin.nextElementSibling).toHaveTextContent("2026");
    const app = screen.getByRole("heading", { name: "App" }).nextElementSibling as HTMLElement;
    expect(app).toHaveTextContent("Desktop app 0.9.0+aaaa1111");
    expect(app).toHaveTextContent("The server is 0.10.0");

    // Profile holds what the holder sets about themselves, and not these.
    await user.click(screen.getByRole("tab", { name: "Profile" }));
    await settle();
    expect(screen.getByRole("heading", { name: "Display Name" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Last Login" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "App" })).not.toBeInTheDocument();
  });

  it("says Never and not connected on Account for an account that has done neither", async () => {
    renderHome(["/admin/accounts/101"]);
    await settle();

    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
    const lastLogin = screen.getByRole("heading", { name: "Last Login" });
    expect(lastLogin.nextElementSibling).toHaveTextContent("Never");
    expect(screen.getByText("Never connected.")).toBeInTheDocument();
  });

  it("shows the administrator an account's Storage as the account sees it", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("tab", { name: "Storage" }));
    await settle();

    expect(getAccountStorage).toHaveBeenCalledWith(expect.anything(), 101);
    const firstPage = { limit: 50, offset: 0 };
    expect(listAccountImports).toHaveBeenCalledWith(firstPage, expect.anything(), 101);
    expect(listAccountExports).toHaveBeenCalledWith(firstPage, expect.anything(), 101);
    // What the accounts table used to carry: the message count and the storage total.
    expect(screen.getByText(/1,234 messages/)).toBeInTheDocument();
    expect(screen.getByText(/7 attachments/)).toBeInTheDocument();
    // Counts of conversations and contacts, and never their names.
    expect(screen.getByText(/12 conversations, 34 contacts/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Import history" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /export history/i })).toBeInTheDocument();
  });

  it("lists an account's largest attachments for the administrator by name and size, with no conversation", async () => {
    getAccountStorage.mockResolvedValue({
      total_bytes: 3000,
      attachment_count: 1,
      // What the server answers the administrator: the file, and not where it sits.
      top_attachments: [
        { id: 5, original_name: "big.mov", mime_type: "video/quicktime", size_bytes: 3000 },
      ],
    });
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("tab", { name: "Storage" }));
    await settle();

    expect(screen.getByText("big.mov")).toBeInTheDocument();
    const table = screen.getByText("big.mov").closest("table") as HTMLElement;
    const headers = Array.from(table.querySelectorAll("th")).map((h) => h.textContent);
    expect(headers).toEqual(["Name", "Size"]);
  });

  it("counts the contacts an import made for the administrator, and does not name them", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("tab", { name: "Storage" }));
    await settle();
    await user.click(screen.getByText("imessage"));
    await settle();

    expect(getAccountImport).toHaveBeenCalledWith(9, expect.anything(), 101);
    expect(screen.getByText("12 new, 3 changed")).toBeInTheDocument();
    expect(getImportContacts).not.toHaveBeenCalled();
  });

  it("deletes an account from its Settings and returns to User Accounts", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    await settle();
    const dialog = screen.getByRole("dialog", { name: "Delete bob's account?" });
    // The administrator deletes on the strength of the count, so the dialog states it.
    expect(dialog).toHaveTextContent("1,234 messages");
    await user.click(within(dialog).getByRole("button", { name: "Delete account" }));
    await settle();

    expect(deleteAccountById).toHaveBeenCalledWith(101);
    expect(screen.getByRole("heading", { name: "User Accounts" })).toBeInTheDocument();
  });

  it("deletes an account's messages from its Settings", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete all messages" }));
    await settle();
    const dialog = screen.getByRole("dialog", { name: "Delete bob's messages?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete all messages" }));
    await settle();

    expect(deleteAccountMessages).toHaveBeenCalledWith(101);
  });

  it("lists each user with a status and a last login, and nothing else", async () => {
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).toEqual(["User", "Status", "Last login"]);
    // What an account holds is under its Storage tab, and its app under Account.
    expect(screen.queryByText("1,234")).not.toBeInTheDocument();
    expect(screen.queryByText(/The accounts on this Message Crate/)).not.toBeInTheDocument();
    // The table sets nothing: status reads as text, and the permissions, like
    // what was the Actions column, are in the account's Settings, behind its gear.
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete account" })).not.toBeInTheDocument();
  });

  it("has no Admin column, because no account can be made one", async () => {
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
    expect(headers).not.toContain("Admin");
  });

  it("shows when each account last logged in, or Never", async () => {
    listAccounts.mockResolvedValue([
      anAccount,
      {
        ...anAccount,
        account_id: 102,
        username: "carol",
        last_login_at: "2026-09-17T14:05:00Z",
      },
    ]);
    renderHome();
    await settle();

    expect(screen.getByText("Never")).toBeInTheDocument();
    // Rendered in the browser's own zone and locale, so match the parts
    // that survive either way.
    expect(screen.getByText(/2026/)).toBeInTheDocument();
  });

  it("states the server's version and schema fingerprint in Settings", async () => {
    renderHome(["/admin/settings"]);
    await settle();

    const version = screen.getByText("Version");
    expect(version.nextElementSibling).toHaveTextContent(APP_BUILD);
    expect(screen.getByText("Schema fingerprint").nextElementSibling).toHaveTextContent(
      "1234567890",
    );
    expect(screen.getByText("Journal mode").nextElementSibling).toHaveTextContent("wal");
  });

  it("says so on every section, with SQLite's error and what to do, when the database runs without write-ahead logging", async () => {
    getServerSettings.mockResolvedValue({
      asset_max_bytes: 512 * MIB,
      journal: { mode: "delete", wal_error: "database is locked" },
    });
    renderHome(["/admin/dashboard"]);
    await settle();

    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "The server could not turn on write-ahead logging when it started",
    );
    expect(notice).toHaveTextContent("SQLite answered: database is locked.");
    expect(notice).toHaveTextContent("If another program has the database file open, close it.");
    expect(notice).toHaveTextContent("Then restart the server");
    // The mode is the administrator's to read, never on the route anyone may ask.
    expect(getServerSettings).toHaveBeenCalled();
    cleanup();

    renderHome(["/admin/accounts"]);
    await settle();
    expect(screen.getByRole("status")).toHaveTextContent("write-ahead logging");
    // It blocks nothing: the screen under it still loads and works.
    expect(screen.getByText("bob")).toBeInTheDocument();
  });

  it("names the mode SQLite kept when it turned write-ahead logging down without an error", async () => {
    getServerSettings.mockResolvedValue({
      asset_max_bytes: 512 * MIB,
      journal: { mode: "memory", wal_error: null },
    });
    renderHome(["/admin/dashboard"]);
    await settle();

    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "SQLite kept the database in journal mode memory without giving an error.",
    );
    expect(notice).not.toHaveTextContent("SQLite answered");
  });

  it("says nothing about write-ahead logging when the server could not read the mode", async () => {
    getServerSettings.mockResolvedValue({
      asset_max_bytes: 512 * MIB,
      journal: { mode: null, wal_error: null },
    });
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    expect(getServerSettings).toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("quotes SQLite's error when the server refused write-ahead logging and cannot read the mode now", async () => {
    getServerSettings.mockResolvedValue({
      asset_max_bytes: 512 * MIB,
      journal: { mode: null, wal_error: "disk I/O error" },
    });
    renderHome(["/admin/dashboard"]);
    await settle();

    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "The server could not turn on write-ahead logging when it started",
    );
    expect(notice).toHaveTextContent("SQLite answered: disk I/O error.");
  });

  it("says nothing about write-ahead logging while the mode is wal, whatever SQLite answered at start", async () => {
    getServerSettings.mockResolvedValue({
      asset_max_bytes: 512 * MIB,
      journal: { mode: "wal", wal_error: "database is locked" },
    });
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    expect(getServerSettings).toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows the journal mode as unknown in Server Settings when the server could not read it", async () => {
    getServerSettings.mockResolvedValue({
      asset_max_bytes: 512 * MIB,
      journal: { mode: null, wal_error: null },
    });
    renderHome(["/admin/settings"]);
    await settle();
    expect(screen.getByText("Journal mode").nextElementSibling).toHaveTextContent(
      "unknown (the server's log says why)",
    );
  });

  it("says nothing under the header while the server and the app are one release", async () => {
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    expect(getServerState).toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("states both versions under the header when the server is another release", async () => {
    getServerState.mockResolvedValue({
      state: "ready",
      version: "99.0.0",
      schema_fingerprint: 1234567890,
    });
    renderHome();
    await settle();

    expect(screen.getByRole("status")).toHaveTextContent(
      `The server is 99.0.0. This app is ${productVersionOf(APP_BUILD)}.`,
    );
    // It blocks nothing: the screen under it still loads and works.
    expect(screen.getByText("bob")).toBeInTheDocument();
  });

  it("sets an account's status from its Settings", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("button", { name: /Status/ }));
    await settle();
    await user.click(screen.getByRole("option", { name: "Disabled" }));
    await settle();

    expect(updateAccount).toHaveBeenCalledWith(101, { disabled: true });
  });

  it("sets an account's permissions from its Settings, under Permissions", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    expect(screen.getByRole("heading", { name: "Message Permissions" })).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: "Delete" }));
    await settle();

    expect(updateAccount).toHaveBeenCalledWith(101, { can_delete: true });
  });

  it("gives the administrator's own account no status and no permissions", async () => {
    renderHome(["/admin/accounts/1"]);
    await settle();

    expect(screen.getByRole("heading", { name: "Change Password" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Message Permissions" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Status" })).not.toBeInTheDocument();
    // Last Login and App are the administrator's view of another account.
    expect(screen.queryByRole("heading", { name: "Last Login" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "App" })).not.toBeInTheDocument();
  });

  it("sets an account's password from its Settings, typed twice the same way", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);

    // The server judges the pair, so a differing one goes to it and its
    // sentence comes back to the screen.
    setAccountPassword.mockRejectedValueOnce(new Error("new passwords do not match"));
    await settle();
    await fill(user, screen.getByLabelText("New password"), "correct horse");
    await fill(user, screen.getByLabelText("Confirm new password"), "correct hors");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    await settle();
    expect(screen.getByText("New passwords do not match")).toBeInTheDocument();
    expect(setAccountPassword).toHaveBeenCalledWith(101, {
      password: "correct horse",
      password_confirmation: "correct hors",
    });

    await user.type(screen.getByLabelText("Confirm new password"), "e");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    await settle();

    expect(setAccountPassword).toHaveBeenCalledWith(101, {
      password: "correct horse",
      password_confirmation: "correct horse",
    });
    // Nothing about a forced change: the person keeps this password.
    expect(screen.queryByText(/made to replace/)).not.toBeInTheDocument();
  });

  it("clears a user's password from the account's Settings", async () => {
    const user = setupUser();
    renderHome(["/admin/accounts/101"]);
    await settle();

    await user.click(screen.getByRole("button", { name: "Reset password" }));
    await settle();

    expect(setAccountPassword).toHaveBeenCalledWith(101, {
      password: "",
      password_confirmation: "",
    });
  });

  it("offers the administrator no way to reset their own password to none", async () => {
    renderHome(["/admin/accounts/1"]);
    await settle();

    expect(screen.getByRole("button", { name: "Change password" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
  });

  it("opens a new account's Settings from Add account, in place of the table", async () => {
    const user = setupUser();
    renderHome();
    await settle();

    expect(screen.getByText("bob")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add account" }));
    await settle();

    expect(screen.getByRole("heading", { name: "New account" })).toBeInTheDocument();
    expect(screen.queryByText("bob")).not.toBeInTheDocument();
    // The username is typed here, where an existing account's is only shown.
    expect(screen.getByLabelText("Username")).not.toHaveAttribute("readonly");
    expect(screen.getByRole("button", { name: "Create" })).toBeInTheDocument();
    // Status, permissions and the deletions are an existing account's.
    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
    expect(screen.queryByText("Danger Zone")).not.toBeInTheDocument();
  });

  it("holds a new account's Profile and Storage back until the account exists", async () => {
    renderHome(["/admin/accounts/new?tab=storage"]);
    await settle();

    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Profile" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("tab", { name: "Storage" })).toHaveAttribute("aria-disabled", "true");
  });

  it("creates the account once its password is typed twice the same way, then opens it", async () => {
    const user = setupUser();
    // The answer to Create is written to the cache before its screen mounts.
    renderHome(["/admin/accounts/new"], { keepUnread: true });
    await settle();

    await fill(user, screen.getByLabelText("Username"), "carol");
    await fill(user, screen.getByLabelText("Password"), "hunter2hunter2");
    await fill(user, screen.getByLabelText("Confirm password"), "hunter2hunter");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await settle();
    expect(screen.getByText("Passwords do not match.")).toBeInTheDocument();
    expect(createAccount).not.toHaveBeenCalled();

    // The server is never asked for the new row here, so what the screen shows
    // can only have come from the answer to Create.
    getAccount.mockImplementation((id: number) =>
      id === 102 ? new Promise(() => {}) : Promise.resolve(anAccount),
    );
    await user.type(screen.getByLabelText("Confirm password"), "2");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await settle();
    expect(createAccount).toHaveBeenCalledWith({
      username: "carol",
      password: "hunter2hunter2",
      preferred_name: null,
      phone: null,
    });
    // The created account's own Settings, with every tab, drawn from the
    // server's answer to Create: nothing waits on a fetch, so nothing flickers.
    expect(screen.getByRole("heading", { name: "User Settings: carol" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("carol")).toHaveAttribute("readonly");
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Storage" })).not.toHaveAttribute("aria-disabled");
  });

  it("names no one in the heading while a managed account is still loading", async () => {
    getAccount.mockReturnValue(new Promise(() => {}));
    renderHome(["/admin/accounts/101"]);
    await settle();

    expect(screen.getByText("Loading…")).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Settings for Administrator" }),
    ).not.toBeInTheDocument();
  });

  it("says Invalid username when the username already belongs to an account", async () => {
    const user = setupUser();
    createAccount.mockRejectedValue(
      new ApiError(409, "username already taken: bob", {
        type: "https://messagecrate.app/docs/developer/reference/errors/username-taken",
        title: "Username taken",
        status: 409,
        detail: "username already taken: bob",
      }),
    );
    renderHome(["/admin/accounts/new"]);
    await settle();

    await fill(user, screen.getByLabelText("Username"), "bob");
    await fill(user, screen.getByLabelText("Password"), "hunter2hunter2");
    await fill(user, screen.getByLabelText("Confirm password"), "hunter2hunter2");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("Invalid username.");
    expect(screen.queryByText(/already taken/)).not.toBeInTheDocument();
  });

  it("shows only the fields a new account needs", async () => {
    renderHome(["/admin/accounts/new"]);
    await settle();

    expect(screen.getByRole("heading", { name: "Password (optional)" })).toBeInTheDocument();
    expect(screen.queryByText(/Profile and Storage open/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Hand this password over/)).not.toBeInTheDocument();
  });

  it("opens the section named in the address", async () => {
    renderHome(["/admin/settings"]);

    expect(selectedSection()).toBe("Server Settings");
    await settle();
    expect(screen.getByLabelText("Attachment size limit")).toBeInTheDocument();
  });

  it("lands /admin and an unknown section on User Accounts", async () => {
    renderHome(["/admin"]);
    expect(selectedSection()).toBe("User Accounts");
    cleanup();

    renderHome(["/admin/conversations"]);
    expect(selectedSection()).toBe("User Accounts");
    await settle();
    expect(screen.getByText("bob")).toBeInTheDocument();
  });

  it("moves between sections from the side panel", async () => {
    const user = setupUser();
    renderHome();

    await user.click(screen.getByRole("button", { name: "Server Settings" }));
    expect(selectedSection()).toBe("Server Settings");
    await settle();
    expect(screen.getByLabelText("Attachment size limit")).toBeInTheDocument();
  });

  it("gives the administrator's own Settings the tabs that mean something to an administrator", async () => {
    renderHome(["/admin/accounts/1"]);
    await settle();

    expect(screen.getByText("Change Password")).toBeInTheDocument();
    // The administrator's own account is read as the logged-in account, not as a managed one.
    expect(getAccount).not.toHaveBeenCalled();
    // No Storage, System or Convert: the administrator holds no messages.
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Account",
      "Profile",
      "Audit Trail",
      "Appearance",
    ]);
    // The administrator's account reaches every other, so its password change asks for the current one.
    expect(screen.getByLabelText("Current password")).toBeInTheDocument();
    // No API tokens and no danger zone: the administrator mints no token and cannot be deleted.
    expect(screen.queryByText(/API tokens/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Danger zone/ })).not.toBeInTheDocument();
  });

  it("shows the administrator a name and a time zone on Profile, and no identities", async () => {
    renderHome(["/admin/accounts/1?tab=profile"]);
    await settle();

    expect(screen.getByText("Display Name")).toBeInTheDocument();
    expect(screen.getByText("Time Zone")).toBeInTheDocument();
    expect(screen.queryByText("My Identities")).not.toBeInTheDocument();
  });
});
