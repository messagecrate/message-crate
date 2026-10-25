/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fill, setupUser } from "../test/user";
import AddIdentityDialog from "./AddIdentityDialog";

afterEach(cleanup);

describe("AddIdentityDialog", () => {
  it("offers Text Message, Email and WhatsApp, and hands back the service picked", async () => {
    const user = setupUser();
    const onConfirm = vi.fn();
    render(<AddIdentityDialog open onClose={() => {}} onConfirm={onConfirm} />);

    await user.click(screen.getByRole("button", { name: /Service/ }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Text Message",
      "Email",
      "WhatsApp",
    ]);
    await user.click(screen.getByRole("option", { name: "Email" }));
    expect(screen.getByRole("textbox", { name: "Identity" })).toHaveAttribute(
      "placeholder",
      "you@example.com",
    );
    await fill(user, screen.getByRole("textbox", { name: "Identity" }), "Bob@Example.com");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(onConfirm).toHaveBeenCalledWith({ address: "Bob@Example.com", service: "email" });
  });

  it("adds a WhatsApp internal id and a sender name, which the server stores as other", async () => {
    const user = setupUser();
    const onConfirm = vi.fn();
    render(<AddIdentityDialog open onClose={() => {}} onConfirm={onConfirm} />);

    await user.click(screen.getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "WhatsApp" }));
    await fill(user, screen.getByRole("textbox", { name: "Identity" }), "123456789012345@lid");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(onConfirm).toHaveBeenLastCalledWith({
      address: "123456789012345@lid",
      service: "whatsapp",
    });

    await user.click(screen.getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "Text Message" }));
    await user.clear(screen.getByRole("textbox", { name: "Identity" }));
    await fill(user, screen.getByRole("textbox", { name: "Identity" }), "sam");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(onConfirm).toHaveBeenLastCalledWith({ address: "sam", service: "text_message" });
  });

  it("refuses a value under Email that is not an address, before asking anyone", async () => {
    const user = setupUser();
    const onConfirm = vi.fn();
    render(<AddIdentityDialog open onClose={() => {}} onConfirm={onConfirm} />);

    await user.click(screen.getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "Email" }));
    await fill(user, screen.getByRole("textbox", { name: "Identity" }), "not-an-address");
    await user.keyboard("{Enter}");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Enter an email address like you@example.com.",
    );
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("refuses an identity already in the list, on the same service only", async () => {
    const user = setupUser();
    const onConfirm = vi.fn();
    render(
      <AddIdentityDialog
        open
        existing={[{ address: "+15555550100", service: "text_message" }]}
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );

    await fill(user, screen.getByRole("textbox", { name: "Identity" }), "+1 (555) 555-0100");
    expect(screen.getByRole("alert")).toHaveTextContent("This identity is already in the list.");
    expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Service/ }));
    await user.click(screen.getByRole("option", { name: "WhatsApp" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(onConfirm).toHaveBeenCalledWith({ address: "+1 (555) 555-0100", service: "whatsapp" });
  });

  it("shows why the last attempt failed and stays open while busy", () => {
    render(
      <AddIdentityDialog
        open
        busy
        error="The server did not add that identity."
        onClose={() => {}}
        onConfirm={() => {}}
      />,
    );
    expect(screen.getByText("The server did not add that identity.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Working…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });
});
