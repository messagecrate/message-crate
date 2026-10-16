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

  it("destroys the current window and waits for it", async () => {
    let destroyed = false;
    destroy.mockImplementation(async () => {
      await Promise.resolve();
      destroyed = true;
    });

    await destroyWindow();

    expect(destroy).toHaveBeenCalledTimes(1);
    expect(destroyed).toBe(true);
  });
});
