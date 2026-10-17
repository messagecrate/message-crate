/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { authModule, renderWithProviders, testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { fill, setupUser } from "../../test/user";
import { ChangePasswordSection } from "./ChangePasswordSection";

const changePassword = vi.hoisted(() => vi.fn());
const setAccountPassword = vi.hoisted(() => vi.fn());
const updateToken = vi.hoisted(() => vi.fn());

vi.mock("../../hooks/useAuth", () => authModule(() => ({ accountId: 101, updateToken })));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  changePassword: (...a: unknown[]) => changePassword(...a),
  setAccountPassword: (...a: unknown[]) => setAccountPassword(...a),
}));

beforeEach(() => {
  startFakeClock();
  changePassword.mockReset();
  updateToken.mockReset();
  setAccountPassword.mockReset();
  setAccountPassword.mockResolvedValue(undefined);
  changePassword.mockResolvedValue({ token: "mc-user-rotated" });
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("ChangePasswordSection", () => {
  it("asks for the new password twice and never the current one", () => {
    renderWithProviders(<ChangePasswordSection />);

    expect(screen.getByLabelText("New password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm new password")).toBeInTheDocument();
    expect(screen.queryByLabelText("Current password")).not.toBeInTheDocument();
  });

  it("accepts a one-character password", async () => {
    const user = setupUser();
    renderWithProviders(<ChangePasswordSection />);

    await fill(user, screen.getByLabelText("New password"), "a");
    await fill(user, screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await settle();
    expect(changePassword).toHaveBeenCalledWith({ password: "a", password_confirmation: "a" });
    expect(updateToken).toHaveBeenCalledWith("mc-user-rotated");
  });

  it("sends a differing confirmation to the server and shows its sentence", async () => {
    // The server checks the pair after the current password, so the screen
    // never judges it: the order of what a user hears is the server's.
    changePassword.mockRejectedValue(new Error("new passwords do not match"));
    const user = setupUser();
    renderWithProviders(<ChangePasswordSection />);

    await fill(user, screen.getByLabelText("New password"), "first");
    await fill(user, screen.getByLabelText("Confirm new password"), "second");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await settle();
    expect(screen.getByText("New passwords do not match")).toBeInTheDocument();
    expect(changePassword).toHaveBeenCalledWith({
      password: "first",
      password_confirmation: "second",
    });
    expect(updateToken).not.toHaveBeenCalled();
  });

  it("resets the password to none", async () => {
    const user = setupUser();
    renderWithProviders(<ChangePasswordSection />);

    await user.click(screen.getByRole("button", { name: "Reset password" }));

    await settle();
    expect(changePassword).toHaveBeenCalledWith({ password: "", password_confirmation: "" });
    expect(
      screen.getByText(
        "Password reset. This account now has no password, and its API Tokens were revoked.",
      ),
    ).toBeInTheDocument();
  });

  it("offers no Reset password when the account must keep one", () => {
    renderWithProviders(<ChangePasswordSection canReset={false} />);

    expect(screen.queryByRole("button", { name: "Reset password" })).not.toBeInTheDocument();
  });

  it("asks the owner for the current password and sends it", async () => {
    const user = setupUser();
    renderWithProviders(<ChangePasswordSection canReset={false} requireCurrent />);

    await fill(user, screen.getByLabelText("New password"), "keeperschoice");
    await fill(user, screen.getByLabelText("Confirm new password"), "keeperschoice");
    // Nothing to send until the current password is typed.
    expect(screen.getByRole("button", { name: "Change password" })).toBeDisabled();

    await fill(user, screen.getByLabelText("Current password"), "hunter2hunter2");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await settle();
    expect(changePassword).toHaveBeenCalledWith({
      password: "keeperschoice",
      password_confirmation: "keeperschoice",
      current_password: "hunter2hunter2",
    });
    expect(screen.getByLabelText("Current password")).toHaveValue("");
  });

  it("marks the API Tokens and the profile stale, and says the tokens were revoked", async () => {
    // The server deletes every API Token of the account when its password
    // changes, and the profile's has_password changes with it. The table kept
    // listing the revoked tokens, and Delete account asked for no password.
    const shown = [keys.apiTokens.all, keys.accountProfile.all];
    const client = testQueryClient();
    seedEntries(client, 101, shown);
    const user = setupUser();
    render(
      <QueryClientProvider client={client}>
        <ChangePasswordSection />
      </QueryClientProvider>,
    );

    await fill(user, screen.getByLabelText("New password"), "a");
    await fill(user, screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await settle();
    expect(
      screen.getByText("Password changed. This account's API Tokens were revoked."),
    ).toBeInTheDocument();
    expect(freshEntries(client, 101, shown)).toEqual([]);
  });

  it("marks User Accounts stale when the owner sets another account's password", async () => {
    // The owner's list says which accounts have a password.
    const shown = [keys.ownerAccounts.all];
    const client = testQueryClient();
    seedEntries(client, 101, shown);
    const user = setupUser();
    render(
      <QueryClientProvider client={client}>
        <ChangePasswordSection managedAccountId={7} />
      </QueryClientProvider>,
    );

    await fill(user, screen.getByLabelText("New password"), "a");
    await fill(user, screen.getByLabelText("Confirm new password"), "a");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await settle();
    expect(setAccountPassword).toHaveBeenCalledWith(7, {
      password: "a",
      password_confirmation: "a",
    });
    expect(changePassword).not.toHaveBeenCalled();
    expect(updateToken).not.toHaveBeenCalled();
    expect(screen.getByText("Password changed.")).toBeInTheDocument();
    expect(freshEntries(client, 101, shown)).toEqual([]);
  });
});
