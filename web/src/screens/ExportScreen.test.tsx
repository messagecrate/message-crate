/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { currentDesktopJob } from "../lib/desktopJob";
import { fill, setupUser } from "../test/user";
import ExportScreen from "./ExportScreen";
import { ConvertSection } from "./settings/ConvertSection";

const invokePull = vi.hoisted(() => vi.fn());
const invokeFormat = vi.hoisted(() => vi.fn());
const invokeFinishExportDir = vi.hoisted(() => vi.fn());
const invokeDiscardExportDir = vi.hoisted(() => vi.fn());
const invokeCancel = vi.hoisted(() => vi.fn());
const invokeCreateExportDir = vi.hoisted(() => vi.fn());
const awaitTauriJob = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => true,
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    EXPORT_FORMATS: actual.EXPORT_FORMATS,
    invokePull: (...args: unknown[]) => invokePull(...args),
    invokeFormat: (...args: unknown[]) => invokeFormat(...args),
    invokeCreateExportDir: (...args: unknown[]) => invokeCreateExportDir(...args),
    invokeFinishExportDir: (...args: unknown[]) => invokeFinishExportDir(...args),
    invokeDiscardExportDir: (...args: unknown[]) => invokeDiscardExportDir(...args),
    invokeCancel: (...args: unknown[]) => invokeCancel(...args),
    // The job's name comes first; the mocks below take what follows it.
    awaitTauriJob: (_job: string, ...args: unknown[]) => awaitTauriJob(...args),
    onExtractEvents: vi.fn(async () => () => {}),
  };
});

vi.mock("../lib/api", () => ({
  getBaseUrl: () => "http://127.0.0.1:8080",
}));

vi.mock("../lib/auth", () => ({
  useAuth: () => ({ token: "test-token" }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

/** The directory the desktop makes for the export in the Export Directory. */
const EXPORT_DIR = {
  dir: "/home/demo/.local/share/app.messagecrate.desktop/exports/export-2026-10-04-1430-csv",
  pulled:
    "/home/demo/.local/share/app.messagecrate.desktop/exports/export-2026-10-04-1430-csv/.pulled",
  converting:
    "/home/demo/.local/share/app.messagecrate.desktop/exports/export-2026-10-04-1430-csv/.converting",
};

beforeEach(() => {
  invokeCreateExportDir.mockResolvedValue(EXPORT_DIR);
  invokeFinishExportDir.mockResolvedValue(EXPORT_DIR.dir);
  invokeDiscardExportDir.mockResolvedValue(undefined);
  // The hook's `run` goes through awaitTauriJob: call the invoke and resolve.
  awaitTauriJob.mockImplementation(async (invokeFn: () => Promise<void>) => {
    await invokeFn();
    return { summary: "done" };
  });
});

/**
 * The screen at `/export`, or at `/export?q=` when `query` is given, the way
 * LeftPanel opens it from the conversation list: with `list=conversations`.
 */
function renderScreen(query?: string) {
  const path =
    query === undefined ? "/export" : `/export?q=${encodeURIComponent(query)}&list=conversations`;
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ExportScreen />
    </MemoryRouter>,
  );
}

/** Fill the save directory and press Export. */
async function exportTo(directory: string) {
  const user = setupUser();
  renderScreen();
  await fill(user, screen.getByPlaceholderText("The Export Directory"), directory);
  await user.click(screen.getByRole("button", { name: "Export" }));
  return user;
}

/** Pick a format from the Format select, then press Export. */
async function exportAs(directory: string, formatLabel: string) {
  const user = setupUser();
  renderScreen();
  await fill(user, screen.getByPlaceholderText("The Export Directory"), directory);
  await user.click(screen.getByRole("button", { name: /Format/ }));
  await user.click(await screen.findByRole("option", { name: formatLabel }));
  await user.click(screen.getByRole("button", { name: "Export" }));
  return user;
}

describe("ExportScreen", () => {
  it("pulls straight into the chosen directory for JSON Lines", async () => {
    await exportTo("/home/demo/out");

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    // Everything is the scope the screen opens in without a query, and it
    // sends a blank query, which message-crate-pull reads as the whole account.
    expect(invokePull.mock.calls[0][0]).toMatchObject({ out_dir: "/home/demo/out", query: "" });
    // JSONL is what pull already writes, so there is nothing to convert. The
    // export's own directory is finished, which deletes it when it is empty.
    expect(invokeFormat).not.toHaveBeenCalled();
    await waitFor(() => expect(invokeFinishExportDir).toHaveBeenCalledWith(EXPORT_DIR.dir));
    expect(invokeDiscardExportDir).not.toHaveBeenCalled();
  });

  it("pulls JSON Lines into its own directory in the Export Directory when no directory is chosen", async () => {
    const user = setupUser();
    renderScreen();
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    expect(invokeCreateExportDir).toHaveBeenCalledWith("export", "jsonl", "");
    expect(invokePull.mock.calls[0][0]).toMatchObject({ out_dir: EXPORT_DIR.dir });
    await waitFor(() => expect(invokeFinishExportDir).toHaveBeenCalledWith(EXPORT_DIR.dir));
    expect(await screen.findByText(/Export complete/)).toHaveTextContent(
      `Export complete. JSON Lines (.jsonl) saved to ${EXPORT_DIR.dir}.`,
    );
  });

  it("converts into the export's own directory when no directory is chosen", async () => {
    const user = setupUser();
    renderScreen();
    await user.click(screen.getByRole("button", { name: /Format/ }));
    await user.click(await screen.findByRole("option", { name: "CSV (.csv)" }));
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokeCreateExportDir).toHaveBeenCalledWith("export", "csv", "");
    expect(invokePull.mock.calls[0][0]).toMatchObject({ out_dir: EXPORT_DIR.pulled });
    // The conversion may not write into the directory that holds its input,
    // so it writes beside it and the finish moves the result up.
    expect(invokeFormat.mock.calls[0][0]).toMatchObject({
      input_dir: EXPORT_DIR.pulled,
      output_dir: EXPORT_DIR.converting,
    });
    await waitFor(() => expect(invokeFinishExportDir).toHaveBeenCalledWith(EXPORT_DIR.dir));
    expect(await screen.findByText(/Export complete/)).toHaveTextContent(
      `Export complete. CSV (.csv) saved to ${EXPORT_DIR.dir}.`,
    );
  });

  it("pulls into the export's own directory and converts into the chosen directory for CSV", async () => {
    const pulled = EXPORT_DIR.pulled;
    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokePull.mock.calls[0][0]).toMatchObject({ out_dir: pulled });
    expect(invokeFormat.mock.calls[0][0]).toEqual({
      input_dir: pulled,
      output_dir: "/home/demo/out",
      output_format: "csv",
      started_from: "export",
      run_started_ms: expect.any(Number),
    });
  });

  it("hands the conversion the time the Export Run started, before the pull", async () => {
    let pulled = 0;
    invokePull.mockImplementation(async () => {
      pulled = Date.now();
    });
    const before = Date.now();
    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    const started = invokeFormat.mock.calls[0][0].run_started_ms as number;
    expect(started).toBeGreaterThanOrEqual(before);
    expect(started).toBeLessThanOrEqual(pulled);
  });

  it("starts nothing when the desktop refuses Save to for holding the Export Directory", async () => {
    invokeCreateExportDir.mockRejectedValueOnce(
      new Error("/home/demo holds the Export Directory, where the export works."),
    );
    await exportAs("/home/demo", "CSV (.csv)");

    expect(
      await screen.findByText("/home/demo holds the Export Directory, where the export works."),
    ).toBeTruthy();
    expect(invokeCreateExportDir).toHaveBeenCalledWith("export", "csv", "/home/demo");
    expect(invokePull).not.toHaveBeenCalled();
    expect(invokeDiscardExportDir).not.toHaveBeenCalled();
  });

  it("finishes the export's own directory once the conversion ends", async () => {
    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeFinishExportDir).toHaveBeenCalledWith(EXPORT_DIR.dir));
    expect(invokeDiscardExportDir).not.toHaveBeenCalled();
  });

  it("deletes the export's own directory when the conversion fails", async () => {
    // Otherwise a failed export silently leaves a whole copy of the conversations on
    // disk, in a directory the person never chose and will not think to look in.
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await invokeFn();
      return { summary: "pulled" };
    });
    awaitTauriJob.mockImplementationOnce(async () => {
      throw new Error("unsupported output format");
    });

    await exportAs("/home/demo/out", "CSV (.csv)");

    await waitFor(() => expect(invokeDiscardExportDir).toHaveBeenCalledWith(EXPORT_DIR.dir));
    expect(invokeFinishExportDir).not.toHaveBeenCalled();
    expect(await screen.findByText("unsupported output format")).toBeTruthy();
  });

  it("does not start the conversion when Cancel is pressed after the pull finished", async () => {
    // A Cancel sent while no job runs stops nothing, and invokeFormat starts
    // its job with a cancel flag of its own, so the screen must not start it.
    let releaseFormat: () => void = () => {};
    const formatHeld = new Promise<void>((resolve) => {
      releaseFormat = resolve;
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await invokeFn();
      return { summary: "pulled" };
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await formatHeld;
      await invokeFn();
      return { summary: "converted" };
    });

    const user = await exportAs("/home/demo/out", "CSV (.csv)");
    await waitFor(() => expect(awaitTauriJob).toHaveBeenCalledTimes(2));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    releaseFormat();

    await waitFor(() => expect(invokeDiscardExportDir).toHaveBeenCalledWith(EXPORT_DIR.dir));
    expect(invokeFormat).not.toHaveBeenCalled();
  });

  it("keeps Convert off between the pull and the format step, and lets it start once the export ends", async () => {
    // Each job holds the desktop only while it runs; between the pull and the
    // format step the desktop has nothing running, so a Convert started there
    // would make it refuse the format step (#1407).
    let releaseFormat: () => void = () => {};
    const formatHeld = new Promise<void>((resolve) => {
      releaseFormat = resolve;
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await invokeFn();
      return { summary: "pulled" };
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await formatHeld;
      await invokeFn();
      return { summary: "converted" };
    });

    const user = setupUser();
    render(
      <MemoryRouter initialEntries={["/export"]}>
        <ExportScreen />
        <ConvertSection />
      </MemoryRouter>,
    );
    await fill(user, screen.getByLabelText("Input directory"), "/home/demo/export-json");
    await fill(user, screen.getByLabelText("Output directory"), "/home/demo/export-csv");
    const convert = screen.getByRole("button", { name: "Convert" });
    expect(convert).toBeEnabled();

    await fill(user, screen.getByLabelText("Save to"), "/home/demo/out");
    // The Export screen's Format comes before Convert's Output format.
    const [exportFormat] = screen.getAllByRole("button", { name: /Format/ });
    if (!exportFormat) throw new Error("no Format select");
    await user.click(exportFormat);
    await user.click(await screen.findByRole("option", { name: "CSV (.csv)" }));
    await user.click(screen.getByRole("button", { name: "Export" }));

    // The pull has ended and the format step has not started.
    await waitFor(() => expect(awaitTauriJob).toHaveBeenCalledTimes(2));
    expect(invokePull).toHaveBeenCalledTimes(1);
    expect(invokeFormat).not.toHaveBeenCalled();
    expect(currentDesktopJob()).toBe("Export");
    expect(convert).toBeDisabled();

    releaseFormat();
    await waitFor(() => expect(invokeFinishExportDir).toHaveBeenCalled());
    await waitFor(() => expect(convert).toBeEnabled());
    expect(currentDesktopJob()).toBeNull();
  });

  it("lets the desktop job go when the export fails", async () => {
    awaitTauriJob.mockImplementationOnce(async () => {
      throw new Error("server unreachable");
    });

    await exportAs("/home/demo/out", "CSV (.csv)");

    expect(await screen.findByText("server unreachable")).toBeTruthy();
    expect(currentDesktopJob()).toBeNull();
  });

  it("ignores a second Export while one is already under way", async () => {
    // The desktop backend runs one job at a time (src-tauri/src/commands/jobs.rs),
    // and between the pull and the conversion it has nothing running to refuse.
    let releasePull: () => void = () => {};
    const pullStarted = new Promise<void>((resolve) => {
      releasePull = resolve;
    });
    invokeCreateExportDir.mockImplementation(async () => {
      await pullStarted;
      return EXPORT_DIR;
    });

    const user = setupUser();
    renderScreen();
    await fill(user, screen.getByPlaceholderText("The Export Directory"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: /Format/ }));
    await user.click(await screen.findByRole("option", { name: "CSV (.csv)" }));

    const exportButton = screen.getByRole("button", { name: "Export" });
    await user.click(exportButton);
    // Still making the export's directory: the button must already be inert.
    // `fireEvent`, not `user.click`: on a disabled button user-event sends the
    // pointer events and not the click, and React Aria then clicks the button
    // itself 80 ms later. On a busy machine that lands after the first export
    // has ended and the button is live again, and starts a second one.
    fireEvent.click(exportButton);
    releasePull();

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokePull).toHaveBeenCalledTimes(1);
    expect(invokeCreateExportDir).toHaveBeenCalledTimes(1);
  });

  it("opens in Everything with no query box, and offers the box under Search", async () => {
    const user = setupUser();
    renderScreen();
    expect(screen.getByRole("button", { name: /Scope/ })).toHaveTextContent("Everything");
    expect(screen.queryByRole("textbox", { name: "Search" })).toBeNull();

    await user.click(screen.getByRole("button", { name: /Scope/ }));
    await user.click(await screen.findByRole("option", { name: "Search" }));
    expect(screen.getByRole("textbox", { name: "Search" })).toBeTruthy();
  });

  it("sends the query typed under Search", async () => {
    const user = setupUser();
    renderScreen();
    await fill(user, screen.getByPlaceholderText("The Export Directory"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: /Scope/ }));
    await user.click(await screen.findByRole("option", { name: "Search" }));
    await fill(user, screen.getByRole("textbox", { name: "Search" }), " in:#19,#22 ");
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    // A search typed here, with no hand-off, is for the Messages list.
    expect(screen.getByRole("button", { name: /Search in/ })).toHaveTextContent("Messages");
    expect(invokePull.mock.calls[0][0]).toMatchObject({ query: "in:#19,#22", list: "messages" });
  });

  it("opens in Search with the query it was given, and sends it", async () => {
    // LeftPanel hands over the conversation list's query as `?q=`, so the
    // person sees what "the current view" means before exporting it.
    const user = setupUser();
    renderScreen("messages:>100 tag:Work");
    expect(screen.getByRole("button", { name: /Scope/ })).toHaveTextContent("Search");
    expect(screen.getByRole("textbox", { name: "Search" })).toHaveValue("messages:>100 tag:Work");
    // The query came from the Conversations list, and the screen says the
    // file will hold whole conversations.
    expect(screen.getByRole("button", { name: /Search in/ })).toHaveTextContent("Conversations");
    expect(
      screen.getByText(/holds every message of each conversation this search finds/),
    ).toBeTruthy();

    await fill(user, screen.getByPlaceholderText("The Export Directory"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    // Sent as a Messages query, the server would refuse `messages:` (#959).
    expect(invokePull.mock.calls[0][0]).toMatchObject({
      query: "messages:>100 tag:Work",
      list: "conversations",
    });
  });

  it("exports only the matching messages once the search is switched to Messages", async () => {
    const user = setupUser();
    renderScreen("tag:Work");
    await user.click(screen.getByRole("button", { name: /Search in/ }));
    await user.click(await screen.findByRole("option", { name: "Messages" }));
    expect(screen.getByText(/holds only the messages this search finds/)).toBeTruthy();

    await fill(user, screen.getByPlaceholderText("The Export Directory"), "/home/demo/out");
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokePull).toHaveBeenCalledTimes(1));
    expect(invokePull.mock.calls[0][0]).toMatchObject({ query: "tag:Work", list: "messages" });
  });

  it("will not export a Search scope with a blank query", async () => {
    // message-crate-pull reads a blank query as the whole account, which is not what
    // someone who chose Search and left the box empty asked for.
    const user = setupUser();
    renderScreen("from:me");
    await fill(user, screen.getByPlaceholderText("The Export Directory"), "/home/demo/out");
    await user.clear(screen.getByRole("textbox", { name: "Search" }));
    expect(screen.getByRole("button", { name: "Export" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Scope/ }));
    await user.click(await screen.findByRole("option", { name: "Everything" }));
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
  });

  it("reports the failure rather than claiming the export finished", async () => {
    awaitTauriJob.mockImplementation(async () => {
      throw new Error("session token is required");
    });

    await exportTo("/home/demo/out");

    expect(await screen.findByText("session token is required")).toBeTruthy();
    expect(screen.queryByText(/Export complete/)).toBeNull();
  });

  it("names the directory and format the export wrote to after the form changes", async () => {
    const user = await exportTo("/a");
    await screen.findByText(/Export complete/);
    const field = screen.getByPlaceholderText("The Export Directory");
    await user.clear(field);
    await fill(user, field, "/b");
    await user.click(screen.getByRole("button", { name: /Format/ }));
    await user.click(await screen.findByRole("option", { name: "CSV (.csv)" }));

    expect(screen.getByText(/Export complete/)).toHaveTextContent(
      "Export complete. JSON Lines (.jsonl) saved to /a.",
    );
  });

  it("locks the directory field while an export runs", async () => {
    let releasePull: () => void = () => {};
    const pullHeld = new Promise<void>((resolve) => {
      releasePull = resolve;
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await pullHeld;
      await invokeFn();
      return { summary: "pulled" };
    });

    await exportTo("/a");
    expect(screen.getByPlaceholderText("The Export Directory")).toBeDisabled();
    releasePull();
    await screen.findByText(/Export complete/);
    expect(screen.getByPlaceholderText("The Export Directory")).toBeEnabled();
  });

  it("clears the last export's message as soon as the next export starts", async () => {
    // The export's directory is made before the job starts; the earlier
    // message must not stay up through that wait.
    const user = await exportTo("/a");
    await screen.findByText(/Export complete/);
    invokeCreateExportDir.mockImplementation(() => new Promise<never>(() => {}));
    await user.click(screen.getByRole("button", { name: /Format/ }));
    await user.click(await screen.findByRole("option", { name: "CSV (.csv)" }));
    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => expect(invokeCreateExportDir).toHaveBeenCalledTimes(2));
    expect(screen.queryByText(/Export complete/)).toBeNull();
  });
});
