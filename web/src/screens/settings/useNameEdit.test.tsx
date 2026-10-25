/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { Link, MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NameRefusalNotice from "../../components/NameRefusalNotice";
import type { AccountProfile } from "../../lib/account";
import { ApiError, setAccountId } from "../../lib/api";
import { account } from "../../test/apiShapes";
import { deferred } from "../../test/deferred";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { authModule, Providers } from "../../test/providers";
import { fill, setupUser } from "../../test/user";
import SettingsScreen from "../SettingsScreen";

/**
 * A display name typed on Profile saves when the person leaves Settings
 * altogether. The Settings screen is gone by the time the server refuses
 * that save, so the refusal shows across the top of the page instead, with a
 * way back to the Profile it came from (#2991).
 */

const getAccountProfile = vi.hoisted(() => vi.fn());
const updateAccountProfile = vi.hoisted(() => vi.fn());
const getAccount = vi.hoisted(() => vi.fn());
const updateAccount = vi.hoisted(() => vi.fn());
vi.mock("../../hooks/useAuth", () => authModule());
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  updateAccountProfile: (...a: unknown[]) => updateAccountProfile(...a),
  getAccount: (...a: unknown[]) => getAccount(...a),
  updateAccount: (...a: unknown[]) => updateAccount(...a),
  listAccountIdentities: async () => [],
}));

// The administrator's own profile, so Profile has no identities or address book.
const own = {
  ...account({ account_id: 7, username: "admin", preferred_name: "Stored Name" }),
  is_admin: true,
} as AccountProfile;
// An account the administrator opened from User Accounts.
const bob = account({ account_id: 5, username: "bob", preferred_name: "Bob" }) as AccountProfile;

beforeEach(() => {
  startFakeClock();
  setAccountId(7);
  getAccountProfile.mockReset().mockResolvedValue(own);
  updateAccountProfile.mockReset();
  getAccount.mockReset().mockResolvedValue(bob);
  updateAccount.mockReset();
});
afterEach(() => {
  cleanup();
  setAccountId(null);
  stopFakeClock();
});

function Where() {
  const location = useLocation();
  return <output aria-label="Location">{`${location.pathname}${location.search}`}</output>;
}

function renderApp(path: string) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <NameRefusalNotice />
        <Link to="/search">Search</Link>
        <Link to="/settings?tab=profile">Settings</Link>
        <Routes>
          <Route path="/settings" element={<SettingsScreen />} />
          <Route
            path="/admin/accounts/5"
            element={<SettingsScreen managedAccountId={5} backToAccounts />}
          />
          <Route path="/search" element={<p>Search screen</p>} />
        </Routes>
        <Where />
      </MemoryRouter>
    </Providers>,
  );
}

function nameField() {
  return screen.getByRole("textbox", { name: "Display name" }) as HTMLInputElement;
}

describe("a display name refused after Settings is left", () => {
  it("is reported across the top of the page, with a link back to Profile", async () => {
    const save = deferred<AccountProfile>();
    updateAccountProfile.mockReturnValue(save.promise);
    const user = setupUser();
    renderApp("/settings?tab=profile");
    await settle();

    await fill(user, nameField(), " Jr");
    await user.click(screen.getByRole("link", { name: "Search" }));
    expect(updateAccountProfile).toHaveBeenCalledWith({ preferred_name: "Stored Name Jr" });
    expect(screen.getByText("Search screen")).toBeInTheDocument();

    save.reject(new ApiError(503, "database is busy"));
    await settle();

    const notice = screen.getByRole("alert");
    expect(notice).toHaveTextContent(
      `Your display name "Stored Name Jr" wasn't saved: Database is busy`,
    );

    await user.click(screen.getByRole("link", { name: "Go to Profile" }));
    expect(screen.getByRole("status", { name: "Location" })).toHaveTextContent(
      "/settings?tab=profile",
    );
    // It stays until the person dismisses it.
    expect(screen.getByRole("alert")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("names the account an administrator was changing, and links back to its Profile", async () => {
    const save = deferred<AccountProfile>();
    updateAccount.mockReturnValue(save.promise);
    const user = setupUser();
    renderApp("/admin/accounts/5?tab=profile");
    await settle();

    await fill(user, nameField(), " Smith");
    await user.click(screen.getByRole("link", { name: "← User Accounts" }));
    expect(updateAccount).toHaveBeenCalledWith(5, { preferred_name: "Bob Smith" });

    save.reject(new ApiError(400, "display name is too long"));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent(
      `bob's display name "Bob Smith" wasn't saved: Display name is too long`,
    );
    expect(screen.getByRole("link", { name: "Go to Profile" })).toHaveAttribute(
      "href",
      "/admin/accounts/5?tab=profile",
    );
  });

  it("is shown on Profile, not across the page, while Settings is still open", async () => {
    updateAccountProfile.mockRejectedValue(new ApiError(503, "database is busy"));
    const user = setupUser();
    renderApp("/settings?tab=profile");
    await settle();

    await fill(user, nameField(), " Jr");
    await user.keyboard("{Enter}");
    await settle();

    expect(screen.getByText("Database is busy")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("is not shown once another account has logged in", async () => {
    const save = deferred<AccountProfile>();
    updateAccountProfile.mockReturnValue(save.promise);
    const user = setupUser();
    renderApp("/settings?tab=profile");
    await settle();

    await fill(user, nameField(), " Jr");
    await user.click(screen.getByRole("link", { name: "Search" }));
    setAccountId(8);
    save.reject(new ApiError(401, "session expired"));
    await settle();

    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("goes once a later save from the same Profile goes through", async () => {
    const save = deferred<AccountProfile>();
    updateAccountProfile.mockReturnValueOnce(save.promise);
    const user = setupUser();
    renderApp("/settings?tab=profile");
    await settle();

    await fill(user, nameField(), " Jr");
    await user.click(screen.getByRole("link", { name: "Search" }));
    save.reject(new ApiError(503, "database is busy"));
    await settle();
    expect(screen.getByRole("alert")).toBeInTheDocument();

    updateAccountProfile.mockResolvedValue({ ...own, preferred_name: "Stored Name Jr" });
    await user.click(screen.getByRole("link", { name: "Go to Profile" }));
    await settle();
    await fill(user, nameField(), " Jr");
    await user.keyboard("{Enter}");
    await settle();

    expect(updateAccountProfile).toHaveBeenLastCalledWith({ preferred_name: "Stored Name Jr" });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("stays when a save sent before it goes through later", async () => {
    const first = deferred<AccountProfile>();
    const second = deferred<AccountProfile>();
    updateAccountProfile.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const user = setupUser();
    renderApp("/settings?tab=profile");
    await settle();

    // The first save is still on the way when Settings is left.
    await fill(user, nameField(), " Jr");
    await user.click(screen.getByRole("link", { name: "Search" }));
    // Settings, opened again, sends a second name and is left again.
    await user.click(screen.getByRole("link", { name: "Settings" }));
    await settle();
    await fill(user, nameField(), " Sr");
    await user.click(screen.getByRole("link", { name: "Search" }));
    expect(updateAccountProfile).toHaveBeenLastCalledWith({ preferred_name: "Stored Name Sr" });

    second.reject(new ApiError(503, "database is busy"));
    await settle();
    first.resolve({ ...own, preferred_name: "Stored Name Jr" });
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent(
      `Your display name "Stored Name Sr" wasn't saved: Database is busy`,
    );
  });

  it("goes when the account that typed it logs out", async () => {
    const save = deferred<AccountProfile>();
    updateAccountProfile.mockReturnValue(save.promise);
    const user = setupUser();
    renderApp("/settings?tab=profile");
    await settle();

    await fill(user, nameField(), " Jr");
    await user.click(screen.getByRole("link", { name: "Search" }));
    save.reject(new ApiError(503, "database is busy"));
    await settle();
    expect(screen.getByRole("alert")).toBeInTheDocument();

    setAccountId(null);
    await settle();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
