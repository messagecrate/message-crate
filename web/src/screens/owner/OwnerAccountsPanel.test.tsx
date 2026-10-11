/** @vitest-environment jsdom */

/**
 * Which accounts the owner's User Accounts list marks as having no password.
 *
 * An account with no password logs in with an empty one for anyone who knows
 * its username, and the list gave the owner no way to see which accounts
 * those were. The Demo Account has no password by design, so it is not marked.
 */

import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../../test/providers";
import { OwnerAccountsPanel } from "./OwnerAccountsPanel";
import type { ManagedAccount } from "./useOwnerAccounts";

const listAccounts = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
}));

afterEach(cleanup);

function account(id: number, username: string, changes: Partial<ManagedAccount>): ManagedAccount {
  return {
    account_id: id,
    username,
    preferred_name: null,
    app: null,
    app_build: null,
    can_delete: false,
    can_export: true,
    can_import: true,
    disabled: false,
    emails: [],
    has_password: true,
    is_demo: false,
    is_owner: false,
    last_login_at: null,
    message_count: 0,
    must_set_up_profile: false,
    phones: [],
    storage_bytes: 0,
    time_zone: "UTC",
    ...changes,
  };
}

async function rowOf(username: string): Promise<HTMLElement> {
  const cell = await screen.findByText(username);
  const row = cell.closest("tr");
  if (!row) throw new Error(`no row for ${username}`);
  return row;
}

describe("OwnerAccountsPanel", () => {
  it("marks an account with no password, and not one with a password or the Demo Account", async () => {
    listAccounts.mockResolvedValue([
      account(1, "admin", { is_owner: true }),
      account(2, "alice", {}),
      account(3, "bob", { has_password: false }),
      account(4, "carol", { has_password: false, disabled: true }),
      account(5, "demo", { has_password: false, is_demo: true }),
    ]);
    render(
      <Providers>
        <MemoryRouter>
          <OwnerAccountsPanel />
        </MemoryRouter>
      </Providers>,
    );

    expect(within(await rowOf("bob")).getByText("No password")).toBeInTheDocument();
    expect(within(await rowOf("carol")).getByText("No password")).toBeInTheDocument();
    expect(within(await rowOf("carol")).getByText("Disabled")).toBeInTheDocument();
    for (const username of ["admin", "alice", "demo"]) {
      expect(within(await rowOf(username)).queryByText("No password")).not.toBeInTheDocument();
    }
  });
});
