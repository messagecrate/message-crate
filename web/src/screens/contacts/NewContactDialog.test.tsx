/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { createContact } from "../../lib/serverApi";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { Providers } from "../../test/providers";
import { fill, setupUser } from "../../test/user";
import NewContactDialog from "./NewContactDialog";

vi.mock("../../hooks/useAuth", async () =>
  (await import("../../test/providers")).authModule({ accountId: 7 }),
);

vi.mock("../../lib/serverApi", () => ({ createContact: vi.fn() }));

const createMock = vi.mocked(createContact);

/** A refusal as the server sends one: `validation-failed` with every sentence in `errors`. */
function refusal(errors: string[]): ApiError {
  return new ApiError(422, errors.join("; "), {
    type: "https://messagecrate.app/docs/developer/reference/errors/validation-failed",
    title: "Validation failed",
    status: 422,
    errors,
  } as unknown as ConstructorParameters<typeof ApiError>[2]);
}

function renderDialog() {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(
    <Providers>
      <NewContactDialog onClose={onClose} onCreated={onCreated} />
    </Providers>,
  );
  return { onCreated, onClose };
}

beforeEach(() => {
  startFakeClock();
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("NewContactDialog", () => {
  it("needs a name, and adds a contact with a name and no identity", async () => {
    const created = { id: 3, name: "Aunt May" } as Awaited<ReturnType<typeof createContact>>;
    createMock.mockResolvedValue(created);
    const user = setupUser();
    const { onCreated } = renderDialog();

    expect(screen.getByRole("button", { name: "Add Contact" })).toBeDisabled();
    await fill(user, screen.getByRole("textbox", { name: "Name" }), "Aunt May");
    await user.click(screen.getByRole("button", { name: "Remove identity 1" }));
    expect(screen.queryByRole("textbox", { name: "Identity 1" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Add Contact" }));
    await settle();

    expect(createMock.mock.calls[0]?.[0]).toEqual({ name: "Aunt May", identities: [] });
    expect(onCreated).toHaveBeenCalledWith(created);
  });

  it("refuses one identity listed twice before anyone is asked", async () => {
    const user = setupUser();
    renderDialog();
    await fill(user, screen.getByRole("textbox", { name: "Name" }), "Mom");
    await fill(user, screen.getByRole("textbox", { name: "Identity 1" }), "+1 555-555-0100");
    await user.click(screen.getByRole("button", { name: "Add identity" }));
    await fill(user, screen.getByRole("textbox", { name: "Identity 2" }), "+15555550100");

    expect(screen.getByRole("textbox", { name: "Identity 2" })).toHaveAccessibleDescription(
      "This identity is already in the list.",
    );
    expect(screen.getByRole("button", { name: "Add Contact" })).toBeDisabled();
  });

  it("puts each refused identity's sentence under its own row, past a removed blank one", async () => {
    createMock.mockRejectedValue(
      refusal([
        "identities[1].address: +15555550102 is already on a contact with no name",
        "name must be at most 200 characters",
      ]),
    );
    const user = setupUser();
    renderDialog();
    await fill(user, screen.getByRole("textbox", { name: "Name" }), "Mom");
    await fill(user, screen.getByRole("textbox", { name: "Identity 1" }), "+15555550101");
    // A blank row in the middle is no identity, so it is not sent, and the
    // server's second identity is the form's third row.
    await user.click(screen.getByRole("button", { name: "Add identity" }));
    await user.click(screen.getByRole("button", { name: "Add identity" }));
    await fill(user, screen.getByRole("textbox", { name: "Identity 3" }), "+15555550102");
    await user.click(screen.getByRole("button", { name: "Add Contact" }));
    await settle();

    expect(screen.getByRole("textbox", { name: "Identity 3" })).toHaveAccessibleDescription(
      "+15555550102 is already on a contact with no name.",
    );
    expect(screen.getByRole("textbox", { name: "Identity 1" })).not.toHaveAttribute("aria-invalid");
    expect(screen.getByText("Name must be at most 200 characters")).toBeInTheDocument();
  });
});
