/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeSaveFile = vi.fn();
const isTauri = vi.fn();

vi.mock("./desktop/fs", () => ({
  invokeSaveFile: (...args: unknown[]) => invokeSaveFile(...args),
}));
vi.mock("./tauriCheck", () => ({
  isTauri: () => isTauri(),
}));

import { nth } from "../test/nth";
import { saveFile } from "./saveFile";

describe("saveFile", () => {
  beforeEach(() => {
    invokeSaveFile.mockReset();
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
    expect(nth(created, 0).type).toBe("text/csv");
    expect(await nth(created, 0).text()).toBe("a,b\n");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:address-book");
    expect(invokeSaveFile).not.toHaveBeenCalled();
  });

  it("hands the desktop app the bytes and the name, with no path from the window", async () => {
    isTauri.mockReturnValue(true);
    invokeSaveFile.mockResolvedValue(true);

    expect(await saveFile("Café photo.jpg", new Blob([new Uint8Array([1, 2, 255])]))).toBe(true);

    expect(invokeSaveFile).toHaveBeenCalledTimes(1);
    const call = nth(invokeSaveFile.mock.calls, 0);
    expect(call).toHaveLength(2);
    const [bytes, fileName] = call;
    expect(bytes).toBeInstanceOf(Uint8Array);
    expect([...(bytes as Uint8Array)]).toEqual([1, 2, 255]);
    expect(fileName).toBe("Café photo.jpg");
  });

  it("reports false when the desktop app's dialog is closed without a choice", async () => {
    isTauri.mockReturnValue(true);
    invokeSaveFile.mockResolvedValue(false);

    expect(await saveFile("address-book.csv", new Blob(["a,b\n"]))).toBe(false);
  });
});
