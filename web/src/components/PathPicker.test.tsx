/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { setupUser } from "../test/user";
import PathPicker from "./PathPicker";

const desktop = vi.hoisted(() => ({ pickPath: vi.fn() }));
vi.mock("../lib/desktop/fs", () => ({ pickPath: desktop.pickPath }));

beforeEach(() => {
  startFakeClock();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  stopFakeClock();
});

describe("PathPicker", () => {
  it("says the file dialog could not be opened when the dialog plugin rejects", async () => {
    // A Tauri command rejects with the plugin's own string, not an Error.
    desktop.pickPath.mockRejectedValue("dialog unavailable");
    const user = setupUser();
    const onChange = vi.fn();
    render(<PathPicker value="" onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: "Browse" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent(
      "The file dialog could not be opened: dialog unavailable",
    );
    expect(onChange).not.toHaveBeenCalled();
  });

  it("clears the message once a later Browse opens the dialog", async () => {
    desktop.pickPath.mockRejectedValueOnce(new Error("dialog unavailable"));
    desktop.pickPath.mockResolvedValueOnce("/backups/chat.db");
    const user = setupUser();
    const onChange = vi.fn();
    render(<PathPicker value="" onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: "Browse" }));
    await settle();
    expect(screen.getByRole("alert")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Browse" }));
    await settle();
    expect(onChange).toHaveBeenCalledWith("/backups/chat.db");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
