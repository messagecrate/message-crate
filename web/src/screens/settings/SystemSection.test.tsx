/** @vitest-environment jsdom */

import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setBaseUrl } from "../../lib/api";
import { APP_BUILD } from "../../lib/build";
import { getOpenToNetwork } from "../../lib/localServer";
import { readerLicenseUrl, readerSourceUrl } from "../../lib/thirdPartySoftware";
import { DOWNLOAD_POLL_MS } from "../../lib/useToolsStatus";
import {
  advanceTimersAndSettle,
  settle,
  startFakeClock,
  stopFakeClock,
} from "../../test/fakeTimers";
import { renderWithProviders as render } from "../../test/providers";
import { MIB } from "../../test/units";
import { fill, setupUser } from "../../test/user";
import { SystemSection } from "./SystemSection";

const tauriState = vi.hoisted(() => ({ isTauri: true }));
const toolsStatus = vi.hoisted(() => vi.fn());
/** The Staging Directory as the desktop process keeps it. */
const desktopStaging = vi.hoisted(() => ({
  root: "",
  defaultRoot: "/home/demo/message-crate",
  /** Set to make reading the Staging Directory fail with this text. */
  readError: null as string | null,
}));
const setStagingDirectory = vi.hoisted(() => vi.fn());
const invokeOpenDataDirectory = vi.hoisted(() => vi.fn());

const startLocalServer = vi.hoisted(() => vi.fn());
const invokeSetOpenToNetwork = vi.hoisted(() => vi.fn());

vi.mock("../../lib/localServer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/localServer")>()),
  startLocalServer: () => startLocalServer(),
}));

vi.mock("../../lib/tauriCheck", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("../../lib/tauri", async (importOriginal) => ({
  toolsDownloading: (await importOriginal<typeof import("../../lib/tauri")>()).toolsDownloading,
  invokeToolsStatus: () => toolsStatus(),
  invokeStagingDirectory: async () => {
    if (desktopStaging.readError !== null) throw desktopStaging.readError;
    return {
      root: desktopStaging.root || desktopStaging.defaultRoot,
      defaultRoot: desktopStaging.defaultRoot,
    };
  },
  invokeSetStagingDirectory: (root: string) => setStagingDirectory(root),
  invokeExportDirectory: async () => "/home/demo/.local/share/app.messagecrate.desktop/exports",
  invokeOpenDataDirectory: () => invokeOpenDataDirectory(),
  invokeSetOpenToNetwork: (on: boolean) => invokeSetOpenToNetwork(on),
}));

afterEach(() => {
  cleanup();
  setBaseUrl("");
  stopFakeClock();
});

beforeEach(() => {
  startFakeClock();
  localStorage.clear();
  tauriState.isTauri = true;
  desktopStaging.root = "";
  desktopStaging.readError = null;
  setStagingDirectory.mockReset();
  setStagingDirectory.mockImplementation(async (root: string) => {
    // The desktop process refuses a relative directory, as resolve_staging_root does.
    if (root !== "" && !root.startsWith("/")) throw "The staging directory must be a full path.";
    desktopStaging.root = root === desktopStaging.defaultRoot ? "" : root;
    return {
      root: desktopStaging.root || desktopStaging.defaultRoot,
      defaultRoot: desktopStaging.defaultRoot,
    };
  });
  toolsStatus.mockReset();
  toolsStatus.mockResolvedValue({
    toolsDir: "/home/demo/message-crate/tools",
    checking: false,
    ffmpeg: { state: "found", path: "/usr/bin/ffmpeg" },
    ffprobe: { state: "found", path: "/usr/bin/ffprobe" },
    wtsexporter: { state: "found", path: "/home/demo/message-crate/tools/wtsexporter" },
  });
});

describe("SystemSection", () => {
  it("shows this app's version in the browser and in the desktop app", async () => {
    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.getByText("Version").nextElementSibling).toHaveTextContent(APP_BUILD);
    cleanup();

    tauriState.isTauri = true;
    render(<SystemSection />);
    await settle();
    expect(screen.getByText("Version").nextElementSibling).toHaveTextContent(APP_BUILD);
  });

  it("names the Apple Messages reader and its GPL license in the desktop app only", async () => {
    tauriState.isTauri = true;
    render(<SystemSection />);
    await settle();
    expect(screen.getByText("Third-party software")).toBeTruthy();
    expect(screen.getByText(/Apple Messages reader/)).toHaveTextContent("imessage-reader");
    expect(screen.getByText(/GNU General Public License/)).toBeTruthy();
    const source = screen.getByRole("link", { name: "Source" });
    expect(source.getAttribute("href")).toBe(readerSourceUrl(APP_BUILD));
    const license = screen.getByRole("link", { name: "License" });
    expect(license.getAttribute("href")).toBe(readerLicenseUrl(APP_BUILD));
    cleanup();

    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.queryByText("Third-party software")).toBeNull();
  });

  it("opens the data directory of the app's own Message Crate", async () => {
    invokeOpenDataDirectory.mockResolvedValue(undefined);
    render(<SystemSection />);
    await settle();
    const user = setupUser();
    await user.click(screen.getByRole("button", { name: "Open data directory" }));
    await settle();
    expect(invokeOpenDataDirectory).toHaveBeenCalledTimes(1);
  });

  it("says why the data directory could not be opened", async () => {
    invokeOpenDataDirectory.mockRejectedValue(new Error("Could not open /data"));
    render(<SystemSection />);
    await settle();
    const user = setupUser();
    await user.click(screen.getByRole("button", { name: "Open data directory" }));
    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not open /data");
  });

  it("keeps the app's own Message Crate closed to the network until asked", async () => {
    setBaseUrl("http://127.0.0.1:8080");
    startLocalServer.mockReset();
    invokeSetOpenToNetwork.mockReset();
    invokeSetOpenToNetwork.mockResolvedValue({ status: "starting", first_time: false });
    render(<SystemSection />);
    await settle();
    const box = screen.getByRole("checkbox", {
      name: /Let other devices on this network connect/,
    });
    expect(box).not.toBeChecked();
    // The warning is there before the choice, not after it.
    expect(screen.getByText(/plain HTTP/)).toBeInTheDocument();

    const user = setupUser();
    await user.click(box);
    await settle();

    expect(getOpenToNetwork()).toBe(true);
    expect(invokeSetOpenToNetwork).toHaveBeenLastCalledWith(true);

    await user.click(box);
    await settle();
    expect(getOpenToNetwork()).toBe(false);
    expect(invokeSetOpenToNetwork).toHaveBeenLastCalledWith(false);
    // The setting restarts a server the app runs; it never starts one.
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("changes no server while the app uses another Message Crate", async () => {
    setBaseUrl("https://crate.example");
    startLocalServer.mockReset();
    invokeSetOpenToNetwork.mockReset();
    render(<SystemSection />);
    await settle();

    const user = setupUser();
    await user.click(
      screen.getByRole("checkbox", { name: /Let other devices on this network connect/ }),
    );
    await settle();

    expect(getOpenToNetwork()).toBe(true);
    expect(invokeSetOpenToNetwork).not.toHaveBeenCalled();
    expect(startLocalServer).not.toHaveBeenCalled();
  });

  it("says the setting does not change a Message Crate the app did not start", async () => {
    setBaseUrl("http://127.0.0.1:8080");
    invokeSetOpenToNetwork.mockReset();
    invokeSetOpenToNetwork.mockResolvedValue({ status: "ready", started_by_app: false });
    render(<SystemSection />);
    await settle();
    const user = setupUser();
    await user.click(
      screen.getByRole("checkbox", { name: /Let other devices on this network connect/ }),
    );
    await settle();
    expect(screen.getByRole("status")).toHaveTextContent("This app did not start");
  });

  it("offers no data directory in the browser", () => {
    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.queryByRole("button", { name: "Open data directory" })).toBeNull();
  });

  it("shows the desktop-only stub when not in Tauri", () => {
    tauriState.isTauri = false;
    render(<SystemSection />);
    expect(screen.getByText(/available in the desktop app/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  });

  it("has no Save button and labels the path fields", async () => {
    render(<SystemSection />);
    await settle();
    expect(screen.getByLabelText("Staging directory")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Saving…" })).toBeNull();
  });

  it("names the Export Directory, where each Export and Convert gets a directory", async () => {
    render(<SystemSection />);
    await settle();
    expect(
      screen.getByRole("button", {
        name: "/home/demo/.local/share/app.messagecrate.desktop/exports",
      }),
    ).toBeTruthy();
    expect(screen.getByText(/Each Export and Convert gets a directory of its own/)).toBeTruthy();
  });

  it("stores the staging directory in the desktop process on change", async () => {
    const user = setupUser();
    render(<SystemSection />);
    await settle();
    const stagingInput = screen.getByDisplayValue("/home/demo/message-crate");

    await user.clear(stagingInput);
    await fill(user, stagingInput, "/tmp/my-staging");
    await settle();

    expect(desktopStaging.root).toBe("/tmp/my-staging");
    expect(setStagingDirectory).toHaveBeenLastCalledWith("/tmp/my-staging");
  });

  it("says why the desktop process refused a staging directory", async () => {
    const user = setupUser();
    render(<SystemSection />);
    await settle();
    const stagingInput = screen.getByDisplayValue("/home/demo/message-crate");
    setStagingDirectory.mockRejectedValue("Could not save staging.json: disk full");

    await user.type(stagingInput, "/x");
    await settle();

    expect(screen.getByText(/disk full/)).toBeInTheDocument();
  });

  it("shows a directory accepted after a refusal when the field is left before the answer", async () => {
    const user = setupUser();
    render(<SystemSection />);
    await settle();
    const stagingInput = screen.getByDisplayValue("/home/demo/message-crate");
    await user.type(stagingInput, "x", {
      initialSelectionStart: 0,
      initialSelectionEnd: "/home/demo/message-crate".length,
    });
    await settle();
    expect(screen.getByText(/Not saved/)).toBeInTheDocument();

    // The pasted directory replaces the refused one in one change, so the
    // refusal is still shown when the field is left.
    (stagingInput as HTMLInputElement).select();
    // The desktop process accepts the pasted directory, but answers late.
    let answer!: () => void;
    setStagingDirectory.mockImplementationOnce(
      (root: string) =>
        new Promise((resolve) => {
          answer = () => {
            desktopStaging.root = root;
            resolve({ root, defaultRoot: desktopStaging.defaultRoot });
          };
        }),
    );
    await user.paste("/data/mc");
    await user.tab();
    answer();
    await settle();

    expect(stagingInput).toHaveValue("/data/mc");
    expect(screen.queryByText(/Not saved/)).toBeNull();
  });

  it("says why a relative staging directory is not saved, and shows the one in use on blur", async () => {
    const user = setupUser();
    desktopStaging.root = "/srv/staging";
    render(<SystemSection />);
    await settle();
    const stagingInput = screen.getByDisplayValue("/srv/staging");

    // Typed over the whole path: an empty field would clear the setting on its own.
    await user.type(stagingInput, "staging", {
      initialSelectionStart: 0,
      initialSelectionEnd: "/srv/staging".length,
    });

    await settle();
    expect(stagingInput).toHaveValue("staging");
    expect(
      screen.getByText("Not saved. The staging directory must be a full path."),
    ).toBeInTheDocument();
    expect(desktopStaging.root).toBe("/srv/staging");

    await user.tab();
    await settle();

    expect(stagingInput).toHaveValue("/srv/staging");
    expect(screen.queryByText(/must be a full path/)).toBeNull();
  });

  it("says why the staging directory could not be read, and reads it again when the field is left", async () => {
    const user = setupUser();
    desktopStaging.readError = "Could not read staging.json: permission denied";
    desktopStaging.root = "/srv/staging";
    render(<SystemSection />);
    await settle();

    expect(screen.getByText(/permission denied/)).toBeInTheDocument();
    const stagingInput = screen.getByLabelText("Staging directory");
    expect(stagingInput).toHaveValue("");

    desktopStaging.readError = null;
    await user.click(stagingInput);
    await user.tab();
    await settle();

    expect(stagingInput).toHaveValue("/srv/staging");
    expect(screen.queryByText(/permission denied/)).toBeNull();
  });

  it("shows where ffmpeg, ffprobe and wtsexporter were found, and the Tools Directory", async () => {
    render(<SystemSection />);
    await settle();
    expect(screen.getByLabelText(/Found ffmpeg/i)).toBeTruthy();
    expect(screen.getByLabelText(/Found ffprobe/i)).toBeTruthy();
    expect(screen.getByLabelText(/Found wtsexporter/i)).toBeTruthy();
    expect(screen.getByText("/usr/bin/ffmpeg")).toBeTruthy();
    expect(screen.getByText("/usr/bin/ffprobe")).toBeTruthy();
    expect(screen.getByText("/home/demo/message-crate/tools/wtsexporter")).toBeTruthy();
    expect(screen.getByText("Tools directory").nextElementSibling).toHaveTextContent(
      "/home/demo/message-crate/tools",
    );
  });

  it("says which program is missing", async () => {
    toolsStatus.mockResolvedValue({
      toolsDir: "/home/demo/message-crate/tools",
      checking: false,
      ffmpeg: { state: "missing" },
      ffprobe: { state: "found", path: "/usr/bin/ffprobe" },
      wtsexporter: { state: "missing" },
    });
    render(<SystemSection />);
    await settle();
    expect(
      screen.getByLabelText(
        "ffmpeg not found. Put it beside ffprobe, or put both in the Tools Directory, /home/demo/message-crate/tools.",
      ),
    ).toBeTruthy();
    expect(screen.getByLabelText(/Found ffprobe/i)).toBeTruthy();
    expect(
      screen.getByLabelText(
        "wtsexporter not found. Put it in the Tools Directory, /home/demo/message-crate/tools.",
      ),
    ).toBeTruthy();
  });

  it("says to put ffmpeg and ffprobe in the Tools Directory when neither is found", async () => {
    toolsStatus.mockResolvedValue({
      toolsDir: "/home/demo/message-crate/tools",
      checking: false,
      ffmpeg: { state: "missing" },
      ffprobe: { state: "missing" },
      wtsexporter: { state: "missing" },
    });
    render(<SystemSection />);
    await settle();
    expect(
      screen.getByLabelText(
        "ffmpeg not found. Put it and ffprobe in the Tools Directory, /home/demo/message-crate/tools.",
      ),
    ).toBeTruthy();
    expect(
      screen.getByLabelText(
        "ffprobe not found. Put it and ffmpeg in the Tools Directory, /home/demo/message-crate/tools.",
      ),
    ).toBeTruthy();
  });

  it("says why ffmpeg and ffprobe found in two places are not used", async () => {
    const reason =
      "ffmpeg is on PATH at /usr/bin/ffmpeg and ffprobe is in the Tools Directory at /home/demo/message-crate/tools/ffprobe.";
    toolsStatus.mockResolvedValue({
      toolsDir: "/home/demo/message-crate/tools",
      checking: false,
      ffmpeg: { state: "unusable", reason },
      ffprobe: { state: "unusable", reason },
      wtsexporter: { state: "missing" },
    });
    render(<SystemSection />);
    await settle();
    expect(screen.getByLabelText(`ffmpeg not used. ${reason}`)).toBeTruthy();
    expect(screen.getByLabelText(`ffprobe not used. ${reason}`)).toBeTruthy();
  });

  it("has nothing to type for the media tools: the ffmpeg directory field is gone (#1053)", async () => {
    render(<SystemSection />);
    await settle();
    expect(screen.getByLabelText(/Found ffmpeg/i)).toBeTruthy();
    expect(screen.queryByLabelText("ffmpeg directory")).toBeNull();
    const media = screen.getByRole("list", { name: "Media tools" }).parentElement;
    expect(media?.querySelector("input")).toBeNull();
  });

  it("asks again while a check runs, so a program it hasn't reached yet shows once it arrives", async () => {
    toolsStatus.mockResolvedValueOnce({
      toolsDir: "/home/demo/message-crate/tools",
      checking: true,
      ffmpeg: { state: "found", path: "/usr/bin/ffmpeg" },
      ffprobe: { state: "found", path: "/usr/bin/ffprobe" },
      wtsexporter: { state: "missing" },
    });
    render(<SystemSection />);
    await settle();
    expect(screen.getByLabelText(/wtsexporter not found/)).toBeTruthy();
    // The next answer, at the next poll, has wtsexporter in place.
    await advanceTimersAndSettle(DOWNLOAD_POLL_MS - 1);
    expect(screen.getByLabelText(/wtsexporter not found/)).toBeTruthy();
    await advanceTimersAndSettle(1);
    expect(screen.getByLabelText(/Found wtsexporter/i)).toBeTruthy();
  });

  it("shows a download's progress, and asks again until it ends", async () => {
    toolsStatus.mockResolvedValueOnce({
      toolsDir: "/home/demo/message-crate/tools",
      checking: false,
      ffmpeg: { state: "downloading", received: 12 * MIB, total: 29 * MIB },
      ffprobe: { state: "downloading", received: 0, total: null },
      wtsexporter: { state: "found", path: "/home/demo/message-crate/tools/wtsexporter" },
    });
    render(<SystemSection />);
    await settle();
    expect(screen.getByLabelText("Downloading ffmpeg: 12 MB of 29 MB (41%).")).toBeTruthy();
    expect(screen.getByLabelText("Downloading ffprobe: 0 B so far.")).toBeTruthy();
    // The next answer, at the next poll, has both in place.
    await advanceTimersAndSettle(DOWNLOAD_POLL_MS - 1);
    expect(screen.getByLabelText("Downloading ffmpeg: 12 MB of 29 MB (41%).")).toBeTruthy();
    await advanceTimersAndSettle(1);
    expect(screen.getByLabelText(/Found ffmpeg/i)).toBeTruthy();
    expect(screen.getByLabelText(/Found ffprobe/i)).toBeTruthy();
  });

  it("says a program the app has no download for goes in the Tools Directory by hand", async () => {
    toolsStatus.mockResolvedValue({
      toolsDir: "/home/demo/message-crate/tools",
      checking: false,
      ffmpeg: { state: "found", path: "/usr/bin/ffmpeg" },
      ffprobe: { state: "found", path: "/usr/bin/ffprobe" },
      wtsexporter: { state: "unavailable" },
    });
    render(<SystemSection />);
    await settle();
    expect(
      screen.getByLabelText(
        "wtsexporter not found, and the app has no download of it for this computer. Put a copy in the Tools Directory, /home/demo/message-crate/tools.",
      ),
    ).toBeTruthy();
  });

  it("says why a download failed", async () => {
    const reason = "No connection to the download's server: error sending request.";
    toolsStatus.mockResolvedValue({
      toolsDir: "/home/demo/message-crate/tools",
      checking: false,
      ffmpeg: { state: "found", path: "/usr/bin/ffmpeg" },
      ffprobe: { state: "found", path: "/usr/bin/ffprobe" },
      wtsexporter: { state: "downloadFailed", reason },
    });
    render(<SystemSection />);
    await settle();
    expect(
      screen.getByLabelText(
        `wtsexporter download failed. ${reason} It is tried again the next time the app starts, or with Try again on the Import form. See "Import can't find wtsexporter" in Troubleshooting at messagecrate.app.`,
      ),
    ).toBeTruthy();
    expect(
      screen.getByRole("link", { name: "Import can't find wtsexporter" }).getAttribute("href"),
    ).toBe(
      "https://messagecrate.app/docs/user/features/owner/troubleshooting/#import-cant-find-wtsexporter",
    );
  });

  it("says so when the desktop process cannot be asked", async () => {
    toolsStatus.mockRejectedValue(new Error("no desktop process"));
    render(<SystemSection />);
    await settle();
    expect(screen.getByText(/Could not look for the media tools/)).toHaveTextContent(
      "no desktop process",
    );
  });
});
