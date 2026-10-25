/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { authModule, testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { fill, setupUser } from "../../test/user";
import { NewAccountPanel } from "./NewAccountPanel";

const createAccount = vi.hoisted(() => vi.fn());

vi.mock("../../hooks/useAuth", () => authModule({ accountId: 1 }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  createAccount: (...a: unknown[]) => createAccount(...a),
}));

beforeEach(() => {
  startFakeClock();
  createAccount.mockReset();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("NewAccountPanel", () => {
  it("marks User Accounts stale once the account is created", async () => {
    // The owner's list was drawn before the account existed, and Back from
    // the new account's Settings goes to it.
    const shown = [keys.ownerAccounts.all];
    const client = testQueryClient();
    seedEntries(client, 1, shown);
    createAccount.mockResolvedValue({ account_id: 102, username: "carol" });
    const user = setupUser();
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={["/owner/accounts/new"]}>
          <Routes>
            <Route path="/owner/accounts/new" element={<NewAccountPanel />} />
            <Route path="/owner/accounts/102" element={<p>carol's Settings</p>} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await fill(user, screen.getByLabelText("Username"), "carol");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await settle();

    expect(screen.getByText("carol's Settings")).toBeInTheDocument();
    expect(freshEntries(client, 1, shown)).toEqual([]);
  });
});
