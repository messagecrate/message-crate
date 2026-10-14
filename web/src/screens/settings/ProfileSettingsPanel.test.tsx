/** @vitest-environment jsdom */

import { cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountProfile } from "../../lib/account";
import { ApiError } from "../../lib/api";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { mockedAuth, renderWithProviders as render } from "../../test/providers";
import { fill, setupUser } from "../../test/user";
import { useNameEdit } from "./nameEdit";
import { ProfileSettingsPanel } from "./ProfileSettingsPanel";

const getAccountProfile = vi.hoisted(() => vi.fn());
const updateAccountProfile = vi.hoisted(() => vi.fn());
vi.mock("../../lib/authContext", () => ({ useAuth: () => mockedAuth }));
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getAccountProfile: (...a: unknown[]) => getAccountProfile(...a),
  updateAccountProfile: (...a: unknown[]) => updateAccountProfile(...a),
}));

// The owner's profile, so the identities and address book sections stay out
// of the way: the bug is between the name field and the time zone.
const stored = {
  account_id: 7,
  username: "owner",
  preferred_name: "Stored Name",
  time_zone: "Etc/UTC",
  is_owner: true,
  phones: [],
  emails: [],
} as unknown as AccountProfile;

beforeEach(() => {
  startFakeClock();
  getAccountProfile.mockReset();
  updateAccountProfile.mockReset();
  // The server keeps what a write stored, so the read every write starts
  // answers the new profile, as the server does.
  let current = stored;
  getAccountProfile.mockImplementation(async () => ({ ...current }));
  // The server answers with the profile as it now stands: a new object. A
  // field the body leaves out, as JSON leaves out `undefined`, stays as it
  // was; `null` clears it.
  updateAccountProfile.mockImplementation(async (body: Partial<AccountProfile>) => {
    const sent = Object.fromEntries(Object.entries(body).filter(([, v]) => v !== undefined));
    current = { ...current, ...sent };
    return { ...current };
  });
});
afterEach(() => {
  cleanup();
  stopFakeClock();
});

// The Settings screen holds the typed name; here the panel holds its own.
function Panel() {
  return <ProfileSettingsPanel nameEdit={useNameEdit()} />;
}

function nameField() {
  return screen.getByRole("textbox", { name: "Display name" }) as HTMLInputElement;
}

function zoneField() {
  return screen.getByRole("combobox", { name: "Time zone" }) as HTMLInputElement;
}

describe("ProfileSettingsPanel", () => {
  // Focusing the time zone field draws all of its ~320 rows before the search
  // narrows them, inside the whole settings panel. The test takes about 0.5 s
  // on an idle machine and 3.3 to 5.4 s at a load average near 100, past the
  // 5000 ms test budget (#1417).
  it("saves a typed display name when the person moves on to the time zone", {
    timeout: 15_000,
  }, async () => {
    const user = setupUser();
    render(<Panel />);
    await settle();
    expect(nameField().value).toBe("Stored Name");

    await user.clear(nameField());
    await fill(user, nameField(), "Typed Name");

    // Leaving the field for the time zone saves the name. There is no Save
    // to forget (#2985).
    await fill(user, zoneField(), "dallas");
    expect(updateAccountProfile).toHaveBeenCalledWith({ preferred_name: "Typed Name" });
    await user.click(within(screen.getByRole("listbox")).getAllByRole("option")[0]);
    expect(updateAccountProfile).toHaveBeenCalledWith({ time_zone: "America/Chicago" });
    // The server's answer has reached the profile entry the panel reads.
    await settle();
    expect(zoneField().value).toMatch(/Central Time/);

    expect(nameField().value).toBe("Typed Name");
  });

  it("clears the display name when Enter is pressed in an emptied field", async () => {
    const user = setupUser();
    render(<Panel />);
    await settle();
    expect(nameField().value).toBe("Stored Name");

    await user.clear(nameField());
    await user.keyboard("{Enter}");

    // The server clears the name on `null`; leaving the field out keeps it.
    expect(updateAccountProfile).toHaveBeenCalledWith({ preferred_name: null });
    // Once the answer arrives the field shows the name it carries, so the
    // field reads empty only when the answer has no name.
    await settle();
    expect(nameField().value).toBe("");
  });

  it("starts the server's refusal of a display name with a capital", async () => {
    updateAccountProfile.mockRejectedValue(
      new ApiError(403, "display name and time zone are fixed"),
    );
    const user = setupUser();
    render(<Panel />);
    await settle();

    await fill(user, nameField(), " Jr");
    await user.keyboard("{Enter}");
    await settle();

    expect(screen.getByText("Display name and time zone are fixed")).toBeInTheDocument();
    // The refused name stays in the field, to correct or leave again.
    expect(nameField().value).toBe("Stored Name Jr");
  });

  it("clears a refusal when the field is put back to the stored name", async () => {
    updateAccountProfile.mockRejectedValueOnce(new ApiError(400, "display name is too long"));
    const user = setupUser();
    render(<Panel />);
    await settle();

    await fill(user, nameField(), " Jr");
    await user.keyboard("{Enter}");
    await settle();
    expect(screen.getByText("Display name is too long")).toBeInTheDocument();

    await user.clear(nameField());
    await fill(user, nameField(), "Stored Name");
    await user.keyboard("{Enter}");
    await settle();
    expect(screen.queryByText("Display name is too long")).toBeNull();
  });

  it("keeps a name typed while the previous one is still being saved", async () => {
    let answer: (p: AccountProfile) => void = () => {};
    updateAccountProfile.mockImplementationOnce(
      () => new Promise<AccountProfile>((resolve) => (answer = resolve)),
    );
    const user = setupUser();
    render(<Panel />);
    await settle();

    await user.clear(nameField());
    await fill(user, nameField(), "Ann");
    await user.keyboard("{Enter}");
    await fill(user, nameField(), "e");
    answer({ ...stored, preferred_name: "Ann" });
    await settle();

    expect(nameField().value).toBe("Anne");
  });

  it("does not leave the field on an Enter that picks an input method's candidate", async () => {
    const user = setupUser();
    render(<Panel />);
    await settle();

    await fill(user, nameField(), " Jr");
    fireEvent.keyDown(nameField(), { key: "Enter", isComposing: true });
    await settle();

    expect(nameField()).toHaveFocus();
    expect(updateAccountProfile).not.toHaveBeenCalled();
  });

  it("sends nothing when the field is left with the stored name", async () => {
    const user = setupUser();
    render(<Panel />);
    await settle();

    await user.click(nameField());
    await user.keyboard("{Enter}");
    await fill(user, nameField(), " ");
    await user.click(zoneField());
    await settle();

    expect(updateAccountProfile).not.toHaveBeenCalled();
  });

  it("shows the Demo Account's name and zone as fixed and offers no address book load", async () => {
    getAccountProfile.mockImplementation(async () => ({
      ...stored,
      username: "demo",
      preferred_name: "Demo User",
      is_owner: false,
      is_demo: true,
    }));
    render(<Panel />);
    await settle();
    expect(nameField().value).toBe("Demo User");

    // The server refuses each of these for the Demo Account, so none is offered.
    expect(nameField().readOnly).toBe(true);
    expect(zoneField().disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "Choose a file" })).toBeNull();
    expect(screen.getByText(/display name and time zone are fixed/)).toBeTruthy();
  });
});
