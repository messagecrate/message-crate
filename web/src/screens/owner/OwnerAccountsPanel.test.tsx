/** @vitest-environment jsdom */

/**
 * Which accounts the owner's User Accounts list marks as having no password.
 *
 * An account with no password logs in with an empty one for anyone who knows
 * its username, and the list gave the owner no way to see which accounts
 * those were. A disabled account with no password is marked too: it cannot log
 * in while disabled, but enabling it opens it at once. The Demo Account has no
 * password by design, so it is not marked.
 */

import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { account } from "../../test/apiShapes";
import { mockedAuth, Providers } from "../../test/providers";
import { OwnerAccountsPanel } from "./OwnerAccountsPanel";

const listAccounts = vi.hoisted(() => vi.fn());

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  listAccounts: (...a: unknown[]) => listAccounts(...a),
}));

afterEach(cleanup);

async function rowOf(username: string): Promise<HTMLElement> {
  const cell = await screen.findByText(username);
  const row = cell.closest("tr");
  if (!row) throw new Error(`no row for ${username}`);
  return row;
}

describe("OwnerAccountsPanel", () => {
  it("marks an account with no password, and not one with a password or the Demo Account", async () => {
    listAccounts.mockResolvedValue([
      account({ account_id: 1, username: "admin", is_owner: true }),
      account({ account_id: 2, username: "alice" }),
      account({ account_id: 3, username: "bob", has_password: false }),
      account({ account_id: 4, username: "carol", has_password: false, disabled: true }),
      account({ account_id: 5, username: "demo", has_password: false, is_demo: true }),
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
