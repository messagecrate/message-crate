/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { setupUser } from "../../test/user";
import CredentialFields from "./CredentialFields";

afterEach(cleanup);

const noop = () => {};

function renderFields(autoComplete: "current-password" | "new-password", withConfirm: boolean) {
  render(
    <form>
      <CredentialFields
        username=""
        onUsernameChange={noop}
        password=""
        onPasswordChange={noop}
        autoComplete={autoComplete}
        confirmPassword={withConfirm ? { value: "", onChange: noop } : undefined}
      />
    </form>,
  );
}

/**
 * A password manager decides what to do from `autoComplete`: fill a stored
 * password into a login, or generate and store one for a new account. The
 * wrong value on either form breaks that, and nothing on screen shows it.
 */
describe("CredentialFields", () => {
  it("marks a login's password as the current one, with no confirm field", () => {
    renderFields("current-password", false);

    expect(screen.getByRole("textbox", { name: "Username" })).toHaveAttribute(
      "autocomplete",
      "username",
    );
    const password = screen.getByLabelText("Password");
    expect(password).toHaveAttribute("autocomplete", "current-password");
    expect(password).toHaveAttribute("name", "password");
    expect(screen.queryByLabelText("Confirm Password")).not.toBeInTheDocument();
  });

  it("marks both of a new account's passwords as new ones", () => {
    renderFields("new-password", true);

    const password = screen.getByLabelText("Password");
    expect(password).toHaveAttribute("autocomplete", "new-password");
    expect(password).toHaveAttribute("name", "new-password");
    const confirm = screen.getByLabelText("Confirm Password");
    expect(confirm).toHaveAttribute("autocomplete", "new-password");
    expect(confirm).toHaveAttribute("name", "confirm-password");
  });

  it("shows one password without showing the other", async () => {
    const user = setupUser();
    renderFields("new-password", true);

    const [showPassword] = screen.getAllByRole("button", { name: "Show password" });
    await user.click(showPassword);

    expect(screen.getByLabelText("Password")).toHaveAttribute("type", "text");
    expect(screen.getByLabelText("Confirm Password")).toHaveAttribute("type", "password");
  });
});
