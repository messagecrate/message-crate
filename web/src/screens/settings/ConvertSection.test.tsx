/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { holdDesktopJob } from "../../lib/desktopJob";
import { fill, setupUser } from "../../test/user";
import { ConvertSection } from "./ConvertSection";

const tauriState = vi.hoisted(() => ({ isTauri: true }));
const invokeFormat = vi.hoisted(() => vi.fn());
const invokeCancel = vi.hoisted(() => vi.fn());
const awaitTauriJob = vi.hoisted(() => vi.fn());
const invokeCreateExportDir = vi.hoisted(() => vi.fn());
const invokeFinishExportDir = vi.hoisted(() => vi.fn());
const invokeDiscardExportDir = vi.hoisted(() => vi.fn());

/** The directory the desktop makes for a Convert in the Export Directory. */
const CONVERT_DIR =
  "/home/demo/.local/share/app.messagecrate.desktop/exports/convert-2026-10-04-1430-jsonl";

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => tauriState.isTauri,
}));

vi.mock("../../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/tauri")>();
  return {
    EXPORT_FORMATS: actual.EXPORT_FORMATS,
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

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  tauriState.isTauri = true;
  invokeCreateExportDir.mockResolvedValue({
    dir: CONVERT_DIR,
    pulled: `${CONVERT_DIR}/.pulled`,
    converting: `${CONVERT_DIR}/.converting`,
  });
  invokeFinishExportDir.mockResolvedValue(CONVERT_DIR);
  invokeDiscardExportDir.mockResolvedValue(undefined);
  // The hook's `run` goes through awaitTauriJob: call the invoke and resolve.
  awaitTauriJob.mockImplementation(async (invokeFn: () => Promise<void>) => {
    await invokeFn();
    return { summary: "Format conversion complete." };
  });
});

const convertButton = () => screen.getByRole("button", { name: "Convert" });

/** Fill both directories; the format stays at its default unless `formatLabel` is given. */
async function fillDirectories(input: string, output: string, formatLabel?: string) {
  const user = setupUser();
  render(<ConvertSection />);
  await fill(user, screen.getByLabelText("Input directory"), input);
  await fill(user, screen.getByLabelText("Output directory"), output);
  if (formatLabel) {
    await user.click(screen.getByRole("button", { name: /Output format/ }));
    await user.click(await screen.findByRole("option", { name: formatLabel }));
  }
  return user;
}

describe("ConvertSection", () => {
  it("does not start while another desktop job runs, and names that job", async () => {
    const release = holdDesktopJob("Export");
    try {
      await fillDirectories("/home/demo/export-json", "/home/demo/export-csv");
      expect(convertButton()).toBeDisabled();
      expect(screen.getByRole("status").textContent).toBe(
        "An Export is running. Convert can start once it ends.",
      );
    } finally {
      release();
    }
    await waitFor(() => expect(convertButton()).toBeEnabled());
  });

  it("shows the desktop-only stub when not in Tauri", () => {
    tauriState.isTauri = false;
    render(<ConvertSection />);
    expect(screen.getByText(/available in the desktop app/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Convert" })).toBeNull();
  });

  it("keeps Convert disabled until the input directory is filled", async () => {
    const user = setupUser();
    render(<ConvertSection />);
    expect(convertButton()).toBeDisabled();

    await fill(user, screen.getByLabelText("Input directory"), "/home/demo/export-json");
    expect(convertButton()).toBeEnabled();
  });

  it("converts into its own directory in the Export Directory when no output directory is chosen", async () => {
    const user = setupUser();
    render(<ConvertSection />);
    await fill(user, screen.getByLabelText("Input directory"), "/home/demo/export-json");
    await user.click(convertButton());

    await waitFor(() => expect(invokeFinishExportDir).toHaveBeenCalledWith(CONVERT_DIR));
    expect(invokeCreateExportDir).toHaveBeenCalledWith("convert", "jsonl", "");
    expect(invokeFormat.mock.calls[0][0]).toMatchObject({
      input_dir: "/home/demo/export-json",
      output_dir: CONVERT_DIR,
    });
    expect(await screen.findByText(/Conversion complete/)).toHaveTextContent(
      `Conversion complete. JSON Lines (.jsonl) written to ${CONVERT_DIR}.`,
    );
  });

  it("deletes its own directory when the conversion fails", async () => {
    awaitTauriJob.mockImplementation(async () => {
      throw new Error("no conversation files in /home/demo/empty");
    });
    const user = setupUser();
    render(<ConvertSection />);
    await fill(user, screen.getByLabelText("Input directory"), "/home/demo/empty");
    await user.click(convertButton());

    await waitFor(() => expect(invokeDiscardExportDir).toHaveBeenCalledWith(CONVERT_DIR));
    expect(invokeFinishExportDir).not.toHaveBeenCalled();
  });

  it("makes no directory of its own when an output directory is chosen", async () => {
    const user = await fillDirectories("/home/demo/export-json", "/home/demo/export-csv");
    await user.click(convertButton());

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokeFormat.mock.calls[0][0]).toMatchObject({ output_dir: "/home/demo/export-csv" });
    expect(invokeCreateExportDir).not.toHaveBeenCalled();
  });

  it("refuses the same directory for input and output and says so", async () => {
    // message-reexport would reject this anyway, but only after the job had
    // started; the screen states the rule before the button is live.
    const user = await fillDirectories("/home/demo/export", "/home/demo/export/");

    expect(screen.getByRole("alert")).toHaveTextContent(/two directories must differ/);
    expect(convertButton()).toBeDisabled();
    await user.click(convertButton()).catch(() => {});
    expect(invokeFormat).not.toHaveBeenCalled();
  });

  it("clears the directory message once the output directory changes", async () => {
    const user = await fillDirectories("/home/demo/export", "/home/demo/export");
    expect(screen.getByRole("alert")).toBeTruthy();

    await user.type(screen.getByLabelText("Output directory"), "-csv");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(convertButton()).toBeEnabled();
  });

  it("runs the format command with both directories and the chosen output format", async () => {
    const user = await fillDirectories(
      "/home/demo/export-json",
      "/home/demo/export-csv",
      "CSV (.csv)",
    );
    await user.click(convertButton());

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokeFormat.mock.calls[0][0]).toEqual({
      input_dir: "/home/demo/export-json",
      output_dir: "/home/demo/export-csv",
      output_format: "csv",
      started_from: "convert",
    });
    expect(await screen.findByText(/Conversion complete\. CSV \(\.csv\) written to/)).toBeTruthy();
  });

  it("defaults the output format to JSON Lines", async () => {
    const user = await fillDirectories("/home/demo/export-xml", "/home/demo/export-jsonl");
    await user.click(convertButton());

    await waitFor(() => expect(invokeFormat).toHaveBeenCalledTimes(1));
    expect(invokeFormat.mock.calls[0][0]).toMatchObject({ output_format: "jsonl" });
  });

  it("reports the failure rather than claiming the conversion finished", async () => {
    awaitTauriJob.mockImplementation(async () => {
      throw new Error("input and output directories must be different");
    });
    const user = await fillDirectories("/home/demo/link-to-export", "/home/demo/export");
    await user.click(convertButton());

    expect(await screen.findByText("input and output directories must be different")).toBeTruthy();
    expect(screen.queryByText(/Conversion complete/)).toBeNull();
  });

  it("names the directory and format the conversion wrote to after the form changes", async () => {
    const user = await fillDirectories("/home/demo/export-json", "/a", "CSV (.csv)");
    await user.click(convertButton());
    await screen.findByText(/Conversion complete/);

    const field = screen.getByLabelText("Output directory");
    await user.clear(field);
    await fill(user, field, "/b");
    await user.click(screen.getByRole("button", { name: /Output format/ }));
    await user.click(await screen.findByRole("option", { name: "JSON Lines (.jsonl)" }));

    expect(screen.getByText(/Conversion complete/)).toHaveTextContent(
      "Conversion complete. CSV (.csv) written to /a.",
    );
  });

  it("locks both directory fields while a conversion runs", async () => {
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    awaitTauriJob.mockImplementationOnce(async (invokeFn: () => Promise<void>) => {
      await held;
      await invokeFn();
      return { summary: "Format conversion complete." };
    });

    const user = await fillDirectories("/home/demo/export-json", "/a");
    await user.click(convertButton());
    expect(screen.getByLabelText("Input directory")).toBeDisabled();
    expect(screen.getByLabelText("Output directory")).toBeDisabled();
    release();
    await screen.findByText(/Conversion complete/);
    expect(screen.getByLabelText("Output directory")).toBeEnabled();
  });
});
