/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../../lib/api";
import { settle, startFakeClock, stopFakeClock } from "../../test/fakeTimers";
import { Providers } from "../../test/providers";
import { MIB } from "../../test/units";
import { fill, setupUser } from "../../test/user";
import { ServerSettingsPanel } from "./ServerSettingsPanel";

const getServerSettings = vi.hoisted(() => vi.fn());
const updateServerSettings = vi.hoisted(() => vi.fn());

vi.mock("../../lib/authContext", () => ({
  useAuth: () => ({ accountId: 1 }),
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  getServerSettings: (...a: unknown[]) => getServerSettings(...a),
  updateServerSettings: (...a: unknown[]) => updateServerSettings(...a),
  getServerState: () => new Promise(() => {}),
  getDemoAccount: () => new Promise(() => {}),
}));

function renderPanel() {
  return render(
    <Providers>
      <ServerSettingsPanel />
    </Providers>,
  );
}

beforeEach(() => {
  getServerSettings.mockReset();
  getServerSettings.mockResolvedValue({
    public_registration: false,
    asset_max_bytes: 512 * MIB,
    journal: { mode: "wal", wal_error: null },
  });
  updateServerSettings.mockReset();
  startFakeClock();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("ServerSettingsPanel attachment size limit", () => {
  it("shows the limit the server holds, in megabytes", async () => {
    renderPanel();
    await settle();

    expect(screen.getByLabelText("Attachment size limit")).toHaveValue(512);
    // Nothing to save until the number differs from the one in force.
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("offers no save for a limit that is not a whole number of hundredths of a megabyte", async () => {
    // 0.333 MB, which the field shows rounded to 0.33.
    getServerSettings.mockResolvedValue({
      public_registration: false,
      asset_max_bytes: 349_176,
      journal: { mode: "wal", wal_error: null },
    });
    renderPanel();
    await settle();

    expect(screen.getByLabelText("Attachment size limit")).toHaveValue(0.33);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("saves the number the owner types as bytes, and leaves registration out of the request", async () => {
    updateServerSettings.mockResolvedValue({
      public_registration: false,
      asset_max_bytes: 100 * MIB,
      journal: { mode: "wal", wal_error: null },
    });
    renderPanel();
    await settle();
    const field = screen.getByLabelText("Attachment size limit");
    // The read the write starts never answers, so only the write's own answer
    // can show the new limit.
    getServerSettings.mockReturnValue(new Promise(() => {}));

    const user = setupUser();
    await user.clear(field);
    await fill(user, field, "100");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await settle();

    expect(updateServerSettings).toHaveBeenCalledWith({ asset_max_bytes: 100 * MIB });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(field).toHaveValue(100);
  });

  it("offers no save for a limit that is blank or not above zero", async () => {
    renderPanel();
    await settle();
    const field = screen.getByLabelText("Attachment size limit");

    const user = setupUser();
    await user.clear(field);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await fill(user, field, "0");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(updateServerSettings).not.toHaveBeenCalled();
  });

  it("shows the server's sentence when it refuses the limit, and keeps what was typed", async () => {
    updateServerSettings.mockRejectedValue(
      new ApiError(422, "asset_max_bytes must be at most 9223372036854775807"),
    );
    renderPanel();
    await settle();
    const field = screen.getByLabelText("Attachment size limit");

    const user = setupUser();
    await user.clear(field);
    await fill(user, field, "99999999999999");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent(
      "asset_max_bytes must be at most 9223372036854775807",
    );
    expect(field).toHaveValue(99999999999999);
  });
});
