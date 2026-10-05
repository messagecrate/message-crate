/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const save = vi.fn();
const isTauri = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: (...args: unknown[]) => save(...args),
}));
vi.mock("./tauri-check", () => ({
  isTauri: () => isTauri(),
}));

import { saveDownload, saveFile } from "./saveFile";

describe("saveFile", () => {
  beforeEach(() => {
    invoke.mockReset();
    save.mockReset();
    isTauri.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("downloads the text under the given name in a browser", async () => {
    isTauri.mockReturnValue(false);
    const created: Blob[] = [];
    URL.createObjectURL = vi.fn((blob: Blob) => {
      created.push(blob);
      return "blob:address-book";
    });
    URL.revokeObjectURL = vi.fn();
    const clicked: { href: string; download: string }[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push({ href: this.href, download: this.download });
    });

    expect(await saveFile("address-book.csv", new Blob(["a,b\n"], { type: "text/csv" }))).toBe(
      true,
    );

    expect(clicked).toEqual([{ href: "blob:address-book", download: "address-book.csv" }]);
    expect(created[0].type).toBe("text/csv");
    expect(await created[0].text()).toBe("a,b\n");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:address-book");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("has the desktop app ask where to save and write the file, with no path from the window", async () => {
    isTauri.mockReturnValue(true);
    invoke.mockResolvedValue(true);

    expect(await saveFile("Café photo.jpg", new Blob([new Uint8Array([1, 2, 255])]))).toBe(true);

    expect(save).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, body, options] = invoke.mock.calls[0];
    expect(command).toBe("save_file");
    // The bytes as they are, not a JSON array, and the name as a header.
    expect(body).toBeInstanceOf(Uint8Array);
    expect([...(body as Uint8Array)]).toEqual([1, 2, 255]);
    expect(options).toEqual({ headers: { "file-name": '"Caf\\u00e9 photo.jpg"' } });
  });

  it("escapes a name outside ASCII, an emoji's two halves included, so it fits in a header", async () => {
    isTauri.mockReturnValue(true);
    invoke.mockResolvedValue(true);

    await saveFile("😀.png", new Blob(["x"]));

    expect(invoke.mock.calls[0][2]).toEqual({ headers: { "file-name": '"\\ud83d\\ude00.png"' } });
  });

  it("reports false when the desktop app's dialog is closed without a choice", async () => {
    isTauri.mockReturnValue(true);
    invoke.mockResolvedValue(false);

    expect(await saveFile("address-book.csv", new Blob(["a,b\n"]))).toBe(false);
  });
});

describe("saveDownload", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("hands the desktop app the link and the name, never the file's bytes", async () => {
    invoke.mockResolvedValue(true);
    const url = "http://127.0.0.1:8080/v1/assets/abc?media_link=1.2.sig";

    expect(await saveDownload("Café clip.mov", url)).toBe(true);

    expect(invoke).toHaveBeenCalledExactlyOnceWith("save_download", {
      url,
      fileName: "Café clip.mov",
    });
  });

  it("reports false when the desktop app's dialog is closed without a choice", async () => {
    invoke.mockResolvedValue(false);

    expect(await saveDownload("clip.mov", "http://server/v1/assets/abc?media_link=l")).toBe(false);
  });
});
