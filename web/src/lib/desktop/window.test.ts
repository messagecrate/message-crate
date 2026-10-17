import { beforeEach, describe, expect, it, vi } from "vitest";
import { destroyWindow, onWindowCloseRequested } from "./window";

const onCloseRequested = vi.fn();
const destroy = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested, destroy }),
}));

describe("onWindowCloseRequested", () => {
  beforeEach(() => {
    onCloseRequested.mockReset();
  });

  it("hands the handler to the current window and resolves to its unlisten function", async () => {
    const unlisten = vi.fn();
    onCloseRequested.mockResolvedValue(unlisten);
    const handler = vi.fn();

    await expect(onWindowCloseRequested(handler)).resolves.toBe(unlisten);

    expect(onCloseRequested).toHaveBeenCalledExactlyOnceWith(handler);
  });
});

describe("destroyWindow", () => {
  beforeEach(() => {
    destroy.mockReset();
  });

  it("destroys the current window", async () => {
    destroy.mockResolvedValue(undefined);

    await destroyWindow();

    expect(destroy).toHaveBeenCalledTimes(1);
  });

  // A failure that reaches the caller shows the wrapper waits for destroy():
  // without the wait the rejection would be lost.
  it("passes on the window's refusal to close", async () => {
    destroy.mockRejectedValue(new Error("window already gone"));

    await expect(destroyWindow()).rejects.toThrow("window already gone");
  });
});
