/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fill, setupUser } from "../test/user";
import IdentityDialogs from "./IdentityDialogs";

vi.mock("../lib/phoneCountries", () => ({
  usePhoneCountries: () => ({ countries: [], loading: false, error: null }),
}));

afterEach(cleanup);

function countryPick(target: { address: string; service: string | null } | null) {
  return { target, error: null, close: vi.fn(), confirm: vi.fn(async () => {}) };
}

describe("IdentityDialogs", () => {
  it("opens Pick country for the identity picked, and closes it through the pick", async () => {
    const user = setupUser();
    const pick = countryPick({ address: "07700900123", service: "phone" });
    render(
      <IdentityDialogs
        countryPick={pick}
        busy={false}
        adding={false}
        addError=""
        existing={[]}
        onCloseAdd={() => {}}
        onConfirmAdd={() => {}}
      />,
    );

    expect(screen.getByText(/07700900123 was written without its country code/)).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: "Identity" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(pick.close).toHaveBeenCalledOnce();
  });

  it("opens Add identity over the rows already listed, with the add's error", async () => {
    const user = setupUser();
    const onCloseAdd = vi.fn();
    const onConfirmAdd = vi.fn();
    render(
      <IdentityDialogs
        countryPick={countryPick(null)}
        busy={false}
        adding
        addError="The server did not add that identity."
        existing={[{ address: "+15555550119", service: "phone" }]}
        onCloseAdd={onCloseAdd}
        onConfirmAdd={onConfirmAdd}
      />,
    );

    expect(screen.queryByText(/written without its country code/)).toBeNull();
    expect(screen.getByText("The server did not add that identity.")).toBeTruthy();
    await fill(user, screen.getByRole("textbox", { name: "Identity" }), "+15555550119");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByText("This identity is already in the list.")).toBeTruthy();
    expect(onConfirmAdd).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCloseAdd).toHaveBeenCalledOnce();
  });
});
