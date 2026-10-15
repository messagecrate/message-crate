/** @vitest-environment jsdom */

import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keys } from "../../lib/queryKeys";
import {
  advanceTimersAndSettle,
  settle,
  startFakeClock,
  stopFakeClock,
} from "../../test/fakeTimers";
import { authModule, Providers, testQueryClient } from "../../test/providers";
import { freshEntries, seedEntries } from "../../test/staleEntries";
import { setupUser } from "../../test/user";
import { BUILD_POLL_MS, DemoAccountCard } from "./DemoAccountCard";

const getDemoAccount = vi.hoisted(() => vi.fn());
const replaceDemoAccount = vi.hoisted(() => vi.fn());

vi.mock("../../lib/authContext", () => authModule({ accountId: 1 }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getDemoAccount: (...a: unknown[]) => getDemoAccount(...a),
  replaceDemoAccount: (...a: unknown[]) => replaceDemoAccount(...a),
}));

const absent = { status: "absent", size: null, error: null };
const ready = { status: "ready", size: null, error: null };
const building = { status: "building", size: "medium", error: null };

function renderCard() {
  return render(
    <Providers>
      <DemoAccountCard />
    </Providers>,
  );
}

beforeEach(() => {
  getDemoAccount.mockReset();
  replaceDemoAccount.mockReset();
  replaceDemoAccount.mockResolvedValue(building);
  startFakeClock();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("DemoAccountCard", () => {
  it("adds the Demo Account at once when there is none, and shows it working", async () => {
    getDemoAccount.mockResolvedValueOnce(absent);
    // The read the write starts never answers, so only the write's own answer
    // can show the build.
    getDemoAccount.mockReturnValue(new Promise(() => {}));
    renderCard();
    await settle();

    const user = setupUser();
    await user.click(screen.getByRole("button", { name: "Add Demo Account" }));
    await settle();

    expect(replaceDemoAccount).toHaveBeenCalledWith({ size: "medium" });
    expect(screen.getByRole("status")).toHaveTextContent("Building the Demo Account");
    expect(screen.queryByRole("button", { name: /Demo Account$/ })).not.toBeInTheDocument();
  });

  it("asks before resetting, because a reset removes what visitors changed", async () => {
    getDemoAccount.mockResolvedValue(ready);
    renderCard();
    await settle();

    const user = setupUser();
    await user.click(screen.getByRole("button", { name: "Reset Demo Account" }));
    await settle();
    expect(replaceDemoAccount).not.toHaveBeenCalled();

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("No other account is touched.");
    await user.click(within(dialog).getByRole("button", { name: "Reset Demo Account" }));
    await settle();

    expect(replaceDemoAccount).toHaveBeenCalledWith({ size: "medium" });
  });

  it("keeps reading while a build runs and offers the action again when it ends", async () => {
    getDemoAccount.mockResolvedValueOnce(building).mockResolvedValue(ready);
    renderCard();
    await settle();

    expect(screen.getByRole("status")).toHaveTextContent("Building the Demo Account");
    // The card asks again only when its poll comes around.
    await advanceTimersAndSettle(BUILD_POLL_MS - 1);
    expect(screen.getByRole("status")).toHaveTextContent("Building the Demo Account");
    await advanceTimersAndSettle(1);
    expect(screen.getByRole("button", { name: "Reset Demo Account" })).toBeEnabled();
  });

  it("marks User Accounts stale when a build ends, and not before", async () => {
    // The build adds the Demo Account to the owner's list, which the server
    // finishes on its own after the write that started it has settled.
    const shown = [keys.ownerAccounts.all];
    const client = testQueryClient();
    seedEntries(client, 1, shown);
    getDemoAccount.mockResolvedValueOnce(building).mockResolvedValue(ready);
    render(
      <QueryClientProvider client={client}>
        <DemoAccountCard />
      </QueryClientProvider>,
    );
    await settle();

    expect(screen.getByRole("status")).toHaveTextContent("Building the Demo Account");
    expect(freshEntries(client, 1, shown)).toEqual(shown);
    await advanceTimersAndSettle(BUILD_POLL_MS);
    expect(screen.getByRole("button", { name: "Reset Demo Account" })).toBeEnabled();
    expect(freshEntries(client, 1, shown)).toEqual([]);
  });

  it("says why the last build failed and offers to add it again", async () => {
    getDemoAccount.mockResolvedValue({ status: "failed", size: null, error: "disk full" });
    renderCard();
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("disk full");
    expect(screen.getByRole("button", { name: "Add Demo Account" })).toBeEnabled();
  });
});
