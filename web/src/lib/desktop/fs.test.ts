import { beforeEach, describe, expect, it, vi } from "vitest";
import { nth } from "../../test/nth";
import { invokeOpenPath, invokeSaveDownload, invokeSaveFile, pickPath } from "./fs";

const invoke = vi.fn();
const openDialog = vi.fn();
const saveDialog = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openDialog(...args),
  save: (...args: unknown[]) => saveDialog(...args),
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

describe("invokeOpenPath", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  // The desktop process checks the path against the run directories it made;
  // a root from the window would be the setting as it is now.
  it("hands the desktop app the path alone", async () => {
    invoke.mockResolvedValue(undefined);

    await invokeOpenPath("/home/sam/message-crate/staging");

    expect(invoke).toHaveBeenCalledExactlyOnceWith("open_path", {
      path: "/home/sam/message-crate/staging",
    });
  });
});

describe("invokeSaveFile", () => {
  beforeEach(() => {
    invoke.mockReset();
    saveDialog.mockReset();
  });

  it("sends the bytes as the raw body and the name as a header, with no path", async () => {
    invoke.mockResolvedValue(true);

    await expect(invokeSaveFile(new Uint8Array([1, 2, 255]), "Café photo.jpg")).resolves.toBe(true);

    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, body, options] = nth(invoke.mock.calls, 0);
    expect(command).toBe("save_file");
    // The bytes as they are, not a JSON array, and the name as a header.
    expect(body).toBeInstanceOf(Uint8Array);
    expect([...(body as Uint8Array)]).toEqual([1, 2, 255]);
    expect(options).toEqual({ headers: { "file-name": '"Caf\\u00e9 photo.jpg"' } });
    // The desktop app shows the Save dialog itself; the window opens none.
    expect(saveDialog).not.toHaveBeenCalled();
  });

  it("escapes a name outside ASCII, an emoji's two halves included, so it fits in a header", async () => {
    invoke.mockResolvedValue(true);

    await invokeSaveFile(new Uint8Array([120]), "😀.png");

    expect(nth(invoke.mock.calls, 0)[2]).toEqual({
      headers: { "file-name": '"\\ud83d\\ude00.png"' },
    });
  });

  it("reports false when the dialog is closed without a choice", async () => {
    invoke.mockResolvedValue(false);

    await expect(invokeSaveFile(new Uint8Array([1]), "address-book.csv")).resolves.toBe(false);
  });
});

describe("pickPath", () => {
  beforeEach(() => {
    openDialog.mockReset();
  });

  it("asks the dialog for one directory when directory is set", async () => {
    openDialog.mockResolvedValue("/backups");

    await expect(pickPath({ directory: true })).resolves.toBe("/backups");

    expect(openDialog).toHaveBeenCalledExactlyOnceWith({ directory: true, multiple: false });
  });

  it("asks the dialog for one file with the given filters", async () => {
    openDialog.mockResolvedValue("/backups/chat.db");
    const filters = [{ name: "Messages database", extensions: ["db"] }];

    await expect(pickPath({ filters })).resolves.toBe("/backups/chat.db");

    expect(openDialog).toHaveBeenCalledExactlyOnceWith({ multiple: false, filters });
  });

  it("resolves null when the dialog is closed without a choice", async () => {
    openDialog.mockResolvedValue(null);

    await expect(pickPath({})).resolves.toBeNull();
  });

  it("resolves null when the dialog answers with anything but one path", async () => {
    openDialog.mockResolvedValue(["/a", "/b"]);

    await expect(pickPath({})).resolves.toBeNull();
  });

  it("passes the dialog's rejection on", async () => {
    openDialog.mockRejectedValue("dialog unavailable");

    await expect(pickPath({})).rejects.toBe("dialog unavailable");
  });
});
