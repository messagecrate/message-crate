import { beforeEach, describe, expect, it, vi } from "vitest";
import { invokeSaveDownload } from "./fs";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

describe("invokeSaveDownload", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  // Tauri matches arguments by name: a key the command does not take reaches
  // it as missing, and the download fails before the Save dialog shows.
  it("hands the desktop app the link and the name under the command's argument names, never the file's bytes", async () => {
    invoke.mockResolvedValue(true);
    const url = "http://127.0.0.1:8080/v1/assets/abc?media_link=1.2.sig";

    await expect(invokeSaveDownload(url, "Café clip.mov")).resolves.toBe(true);

    expect(invoke).toHaveBeenCalledExactlyOnceWith("save_download", {
      url,
      fileName: "Café clip.mov",
    });
  });
});
