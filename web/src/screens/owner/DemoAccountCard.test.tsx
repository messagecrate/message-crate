/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advanceTimersAndSettle, settle, useFakeClock, useRealClock } from "../../test/fakeTimers";
import { Providers } from "../../test/providers";
import { setupUser } from "../../test/user";
import { BUILD_POLL_MS, DemoAccountCard } from "./DemoAccountCard";

const getDemoAccount = vi.hoisted(() => vi.fn());
const replaceDemoAccount = vi.hoisted(() => vi.fn());

vi.mock("../../lib/authContext", () => ({
  useAuth: () => ({ accountId: 1 }),
}));

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
  useFakeClock();
});

afterEach(() => {
  cleanup();
  useRealClock();
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
    // The card asks again only when its poll comes round.
    await advanceTimersAndSettle(BUILD_POLL_MS - 1);
    expect(screen.getByRole("status")).toHaveTextContent("Building the Demo Account");
    await advanceTimersAndSettle(1);
    expect(screen.getByRole("button", { name: "Reset Demo Account" })).toBeEnabled();
  });

  it("says why the last build failed and offers to add it again", async () => {
    getDemoAccount.mockResolvedValue({ status: "failed", size: null, error: "disk full" });
    renderCard();
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("disk full");
    expect(screen.getByRole("button", { name: "Add Demo Account" })).toBeEnabled();
  });
});
