/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { routeQueryKey } from "../../lib/routeQueryKey";
import { testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { setupUser } from "../../test/user";
import { ProfileDangerZone } from "./ProfileDangerZone";

const deleteAccount = vi.hoisted(() => vi.fn());
const deleteAllMessages = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());
const desktop = vi.hoisted(() => ({ value: false }));
const accountStagingDirectories = vi.hoisted(() => vi.fn());

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => desktop.value,
}));

vi.mock("../../lib/importSession", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/importSession")>()),
  accountStagingDirectories: (...a: unknown[]) => accountStagingDirectories(...a),
}));

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ accountId: 7, logout }),
}));

vi.mock("../owner/useOwnerAccounts", () => ({
  useDeleteAccount: () => ({ mutateAsync: vi.fn() }),
  useDeleteAccountMessages: () => ({ mutateAsync: vi.fn() }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  deleteAccount: (...a: unknown[]) => deleteAccount(...a),
  deleteAllMessages: (...a: unknown[]) => deleteAllMessages(...a),
}));

beforeEach(() => {
  deleteAccount.mockReset();
  deleteAllMessages.mockReset();
  logout.mockReset();
  desktop.value = false;
  accountStagingDirectories.mockReset();
});

afterEach(cleanup);

describe("ProfileDangerZone", () => {
  it("shows the server's refusal inside the delete account dialog", async () => {
    deleteAccount.mockRejectedValue(new Error("Current password is incorrect."));
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("dialog");
    await user.type(within(dialog).getByRole("textbox", { name: /Type your username/ }), "carol");
    await user.type(within(dialog).getByLabelText("Current password"), "wrong");
    await user.click(within(dialog).getByRole("button", { name: "Permanently delete my account" }));

    expect(deleteAccount).toHaveBeenCalledWith({ confirm: true, current_password: "wrong" });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Current password is incorrect.",
    );
    expect(logout).not.toHaveBeenCalled();
  });

  it("names the account's Staging Directories on this computer, and deletes them with the account", async () => {
    desktop.value = true;
    accountStagingDirectories.mockResolvedValue(["/home/carol/staging/iphone-2026-10-04"]);
    deleteAccount.mockResolvedValue(undefined);
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword={false}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("dialog");
    expect(
      await within(dialog).findByText("/home/carol/staging/iphone-2026-10-04"),
    ).toBeInTheDocument();
    expect(dialog).toHaveTextContent(
      "Deleting the account also deletes its Staging Directories on this computer:",
    );
    await user.type(within(dialog).getByRole("textbox", { name: /Type your username/ }), "carol");
    await user.click(within(dialog).getByRole("button", { name: "Permanently delete my account" }));

    await waitFor(() =>
      expect(logout).toHaveBeenCalledWith({
        ask: false,
        deletedAccountDirectories: ["/home/carol/staging/iphone-2026-10-04"],
      }),
    );
    expect(deleteAccount).toHaveBeenCalledWith({ confirm: true, current_password: undefined });
  });

  it("holds the confirm while it looks for the folders again, rather than send the last list", async () => {
    desktop.value = true;
    // The list from the last time the dialog was open, and a new look that
    // has not answered yet.
    const client = testQueryClient();
    seedEntries(client, 7, [keys.imports.stagingDirectories]);
    client.setQueryData(routeQueryKey(7, keys.imports.stagingDirectories), ["/home/carol/old"]);
    accountStagingDirectories.mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword={false}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("dialog");
    await user.type(within(dialog).getByRole("textbox", { name: /Type your username/ }), "carol");

    expect(accountStagingDirectories).toHaveBeenCalled();
    expect(
      within(dialog).getByRole("button", { name: "Permanently delete my account" }),
    ).toBeDisabled();
    expect(within(dialog).queryByText("/home/carol/old")).toBeNull();
  });

  it("deletes no Staging Directory when it could not look for them, even with an earlier list cached", async () => {
    desktop.value = true;
    deleteAccount.mockResolvedValue(undefined);
    const client = testQueryClient();
    seedEntries(client, 7, [keys.imports.stagingDirectories]);
    client.setQueryData(routeQueryKey(7, keys.imports.stagingDirectories), ["/home/carol/old"]);
    accountStagingDirectories.mockRejectedValue(new Error("path_stat failed"));
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword={false}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("dialog");
    expect(await within(dialog).findByText(/so it deletes none/)).toBeInTheDocument();
    await user.type(within(dialog).getByRole("textbox", { name: /Type your username/ }), "carol");
    await user.click(within(dialog).getByRole("button", { name: "Permanently delete my account" }));

    await waitFor(() =>
      expect(logout).toHaveBeenCalledWith({ ask: false, deletedAccountDirectories: [] }),
    );
  });

  it("deletes no folder outside the desktop app", async () => {
    deleteAccount.mockResolvedValue(undefined);
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword={false}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).not.toHaveTextContent(/Staging Director/);
    await user.type(within(dialog).getByRole("textbox", { name: /Type your username/ }), "carol");
    await user.click(within(dialog).getByRole("button", { name: "Permanently delete my account" }));

    await waitFor(() =>
      expect(logout).toHaveBeenCalledWith({ ask: false, deletedAccountDirectories: [] }),
    );
    expect(accountStagingDirectories).not.toHaveBeenCalled();
  });

  it("tells an account without the delete permission to ask the owner", async () => {
    // The server refuses both deletes to such an account, and deleting the
    // account would delete its messages, so neither is offered.
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword
            canDelete={false}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    expect(screen.getByText(/Ask the owner/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete account" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all messages" })).toBeDisabled();
  });

  it("lets the owner delete an account that may not delete itself", async () => {
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword
            canDelete={false}
            managedAccountId={7}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    expect(screen.queryByText(/Ask the owner/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete account" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Delete all messages" })).toBeEnabled();
  });

  it("marks every screen that shows the account's messages stale once they are deleted", async () => {
    // Messages, contacts, the Trash, Storage and the counts on the profile all
    // showed the deleted messages from the cache until they were next fetched.
    const shown = [
      keys.conversations.all,
      keys.contacts.all,
      keys.trash.all,
      keys.storage.all,
      keys.accountProfile.all,
      keys.accountProfile.identities,
    ];
    deleteAllMessages.mockResolvedValue({ conversations: 3, attachments: 0 });
    const client = testQueryClient();
    seedEntries(client, 7, shown);
    const user = userEvent.setup({ delay: null });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed={false}
            accountFixed={false}
            username="carol"
            hasPassword
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    await user.click(screen.getByRole("button", { name: "Delete all messages" }));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Delete all messages" }),
    );

    expect(deleteAllMessages).toHaveBeenCalledWith({ confirm: true });
    await waitFor(() => expect(freshEntries(client, 7, shown)).toEqual([]));
  });

  it("offers neither delete when both are fixed for the account itself", async () => {
    const user = setupUser();
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone messagesFixed accountFixed username="demo" hasPassword={false} />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    for (const name of ["Delete all messages", "Delete account"]) {
      const button = screen.getByRole("button", { name });
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute("title", "Unavailable on the Demo Account");
    }
  });

  it("lets the owner delete an account that may not delete itself, but not empty it", async () => {
    const user = setupUser();
    render(
      <QueryClientProvider client={testQueryClient()}>
        <MemoryRouter>
          <ProfileDangerZone
            messagesFixed
            accountFixed
            username="demo"
            hasPassword={false}
            managedAccountId={7}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await user.click(screen.getByRole("button", { name: /Danger zone/ }));
    expect(screen.getByRole("button", { name: "Delete all messages" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete account" })).toBeEnabled();
  });
});
