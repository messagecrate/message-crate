/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { authModule, Providers } from "../../test/providers";
import LocalAuthCard from "./LocalAuthCard";
import type { ServerState } from "./useServerState";

vi.mock("../../hooks/useAuth", () =>
  authModule(() => ({ login: vi.fn(), setServer: vi.fn(), serverUrl: "" })),
);

afterEach(cleanup);

function renderCard(serverState: ServerState) {
  render(
    <Providers>
      <MemoryRouter>
        <LocalAuthCard serverUrl="http://127.0.0.1:8080" serverState={serverState} />
      </MemoryRouter>
    </Providers>,
  );
}

/**
 * The card offers what the server says is on offer. These two cases are the
 * whole of the entry screen's behavior, and the reason the server reports one
 * value rather than the facts behind it: the mapping lives in one place.
 */
describe("LocalAuthCard", () => {
  it("offers only Set Up on a Message Crate in setup", () => {
    renderCard("setup");
    expect(screen.getByRole("heading", { name: "Set Up" })).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Username")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Password", { exact: true })).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });

  it("offers only Login on a ready Message Crate", () => {
    renderCard("ready");
    expect(screen.getByRole("heading", { name: "Login" })).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByText("Create Account")).not.toBeInTheDocument();
  });

  it("explains that the password is the administrator's", () => {
    renderCard("setup");
    expect(screen.getByText(/password for admin/i)).toBeInTheDocument();
  });
});
