import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeOpenPath = vi.fn();
const isTauri = vi.fn();

vi.mock("./desktop/fs", () => ({
  invokeOpenPath: (...args: unknown[]) => invokeOpenPath(...args),
}));

vi.mock("./tauriCheck", () => ({
  isTauri: () => isTauri(),
}));

describe("openPathInExplorer", () => {
  beforeEach(() => {
    invokeOpenPath.mockReset();
    isTauri.mockReset();
    isTauri.mockReturnValue(true);
    invokeOpenPath.mockResolvedValue(undefined);
  });

  it("asks the desktop app to open the trimmed path alone", async () => {
    const { openPathInExplorer } = await import("./openPath");
    await openPathInExplorer("  /home/sam/message-crate/staging ");
    expect(invokeOpenPath).toHaveBeenCalledExactlyOnceWith("/home/sam/message-crate/staging");
  });

  it("no-ops on blank paths", async () => {
    const { openPathInExplorer } = await import("./openPath");
    await openPathInExplorer("   ");
    expect(invokeOpenPath).not.toHaveBeenCalled();
  });

  it("rejects when not running in Tauri", async () => {
    isTauri.mockReturnValue(false);
    const { openPathInExplorer } = await import("./openPath");
    await expect(openPathInExplorer("/home/sam/message-crate/staging")).rejects.toThrow(
      "desktop app",
    );
    expect(invokeOpenPath).not.toHaveBeenCalled();
  });
});
