/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const login = vi.hoisted(() => vi.fn());
const setUpServer = vi.hoisted(() => vi.fn());

vi.mock("../../hooks/useAuth", () => authModule(() => ({ login, serverUrl: "" })));
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  setUpServer: (...a: unknown[]) => setUpServer(...a),
}));

import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { authModule, Providers } from "../../test/providers";
import { fill, setupUser } from "../../test/user";
import SetupForm from "./SetupForm";

beforeEach(() => {
  login.mockReset();
  setUpServer.mockReset();
  startFakeClock();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

function renderForm() {
  render(
    <Providers>
      <SetupForm serverUrl="http://127.0.0.1:8080" />
    </Providers>,
  );
}

/**
 * The administrator is `admin`, made with the database, so setting up a
 * Message Crate is choosing that account's password and nothing else.
 */
describe("SetupForm", () => {
  it("sends the password alone, with no username", async () => {
    setUpServer.mockResolvedValue({ token: "mc-user-admin", account_id: 1, username: "admin" });
    const user = setupUser();
    renderForm();

    await fill(user, screen.getByLabelText("Password", { exact: true }), "hunter22");
    await fill(user, screen.getByLabelText("Confirm Password"), "hunter22");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await settle();

    expect(setUpServer).toHaveBeenCalledWith({ password: "hunter22" });
  });

  it("refuses two passwords that differ, without asking the server", async () => {
    const user = setupUser();
    renderForm();

    await fill(user, screen.getByLabelText("Password", { exact: true }), "hunter22");
    await fill(user, screen.getByLabelText("Confirm Password"), "hunter23");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await settle();

    expect(screen.getByText("Passwords do not match.")).toBeInTheDocument();
    expect(setUpServer).not.toHaveBeenCalled();
    expect(login).not.toHaveBeenCalled();
  });

  it("logs in with the session the server answers", async () => {
    setUpServer.mockResolvedValue({ token: "mc-user-admin", account_id: 1, username: "admin" });
    const user = setupUser();
    renderForm();

    await fill(user, screen.getByLabelText("Password", { exact: true }), "hunter22");
    await fill(user, screen.getByLabelText("Confirm Password"), "hunter22");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await settle();

    expect(login).toHaveBeenCalledWith(
      "http://127.0.0.1:8080",
      "mc-user-admin",
      1,
      expect.anything(),
    );
  });
});
