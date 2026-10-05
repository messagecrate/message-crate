import { beforeEach, describe, expect, it, vi } from "vitest";
import { attachment } from "../test/apiShapes";
import { downloadAttachment } from "./downloadAttachment";
import { saveDownload, saveFile } from "./saveFile";
import { createMediaLink, fetchAsset } from "./serverApi";
import { isTauri } from "./tauri-check";

vi.mock("./serverApi", () => ({ createMediaLink: vi.fn(), fetchAsset: vi.fn() }));
vi.mock("./saveFile", () => ({ saveDownload: vi.fn(), saveFile: vi.fn() }));
vi.mock("./tauri-check", () => ({ isTauri: vi.fn() }));

const VIDEO = attachment({
  original_name: "Clip.mov",
  mime_type: "video/quicktime",
  sha256: "abc",
});

const LINK = {
  url: "http://127.0.0.1:8080/v1/assets/abc?media_link=1.2.sig",
  preview_url: "http://127.0.0.1:8080/v1/assets/abc/preview?media_link=1.2.sig",
  thumbnail_url: "http://127.0.0.1:8080/v1/assets/abc/thumbnail?media_link=1.2.sig",
  expires_at: "2026-10-05T13:00:00Z",
};

describe("downloadAttachment", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  // A video of hundreds of megabytes fetched into the window, then copied
  // into the request to the app, was held in memory two and three times
  // over (#1739). The app downloads it itself from a Media Link instead.
  it("in the desktop app, has the app download the original from a Media Link, never fetching it into the window", async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(createMediaLink).mockResolvedValue(LINK);
    vi.mocked(saveDownload).mockResolvedValue(true);

    expect(await downloadAttachment(VIDEO)).toBe(true);

    expect(createMediaLink).toHaveBeenCalledExactlyOnceWith("abc");
    expect(saveDownload).toHaveBeenCalledExactlyOnceWith("Clip.mov", LINK.url);
    expect(fetchAsset).not.toHaveBeenCalled();
    expect(saveFile).not.toHaveBeenCalled();
  });

  it("in the desktop app, reports false when the Save dialog is closed without a choice", async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(createMediaLink).mockResolvedValue(LINK);
    vi.mocked(saveDownload).mockResolvedValue(false);

    expect(await downloadAttachment(VIDEO)).toBe(false);
  });

  it("in a browser, fetches the original and hands it to the browser's downloads", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    const original = new Blob(["mov bytes"]);
    vi.mocked(fetchAsset).mockResolvedValue(original);
    vi.mocked(saveFile).mockResolvedValue(true);

    expect(await downloadAttachment(VIDEO)).toBe(true);

    expect(fetchAsset).toHaveBeenCalledExactlyOnceWith("abc", { version: "original" });
    expect(saveFile).toHaveBeenCalledExactlyOnceWith("Clip.mov", original);
    expect(createMediaLink).not.toHaveBeenCalled();
  });

  it("refuses an attachment with no stored file", async () => {
    await expect(downloadAttachment(attachment({ original_name: "gone.jpg" }))).rejects.toThrow(
      "This attachment has no stored file.",
    );
  });
});
