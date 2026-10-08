/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { saveFile } from "../../lib/saveFile";
import type { LogLevel, LogLine, LogLinesPage } from "../../lib/serverApi";
import { installIntersectionObserver, scrollNear } from "../../test/intersectionObserver";
import { Providers } from "../../test/providers";
import { fill, setupUser } from "../../test/user";
import LogViewer, { LOG_PAGE_SIZE } from "./LogViewer";
import type { LinesRequest, LogSource } from "./logSource";

vi.mock("../../lib/auth", () => ({ useAuth: () => ({ accountId: 1 }) }));
vi.mock("../../lib/saveFile", () => ({ saveFile: vi.fn() }));

const saveMock = vi.mocked(saveFile);

const SEVERITY: Record<LogLevel, number> = { error: 0, warn: 1, info: 2, debug: 3, trace: 4 };

/**
 * A log of `count` lines, oldest first: every tenth an error, every fifth
 * otherwise a warning, the rest info. Line `n` says "line n".
 */
function aLog(count: number): LogLine[] {
  return Array.from({ length: count }, (_, i) => {
    const n = i + 1;
    const level: LogLevel = n % 10 === 0 ? "error" : n % 5 === 0 ? "warn" : "info";
    return { id: n * 100, time: "2026-10-08T12:00:00.000000Z", level, text: `line ${n}` };
  });
}

/** A source over `lines` that answers as the server does: newest first, filtered, paged. */
function sourceOver(lines: LogLine[]) {
  const readLines = vi.fn(async (request: LinesRequest): Promise<LogLinesPage> => {
    const matching = lines
      .filter((line) => request.after === undefined || line.id < request.after)
      .filter((line) => !request.level || SEVERITY[line.level] <= SEVERITY[request.level])
      .filter((line) => !request.text || line.text.includes(request.text))
      .reverse();
    return {
      items: matching.slice(0, request.limit),
      limit: request.limit,
      has_more: matching.length > request.limit,
    };
  });
  const source: LogSource = { key: ["test-log"], readLines: (request) => readLines(request) };
  return { source, readLines };
}

function shownTexts(): string[] {
  const list = screen.getByRole("list", { name: "Log lines" });
  return within(list)
    .getAllByRole("listitem")
    .map((item) => item.lastElementChild?.textContent ?? "");
}

function renderViewer(source: LogSource, read = vi.fn(async () => "the whole log\n")) {
  render(
    <Providers>
      <LogViewer source={source} downloads={[{ name: "import-sms-261006.log", read }]} />
    </Providers>,
  );
  return read;
}

async function pickLevel(user: ReturnType<typeof setupUser>, label: string) {
  await user.click(screen.getByRole("button", { name: /Show/ }));
  await user.click(screen.getByRole("option", { name: label }));
}

describe("LogViewer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    installIntersectionObserver();
    saveMock.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
  });

  it("opens at warnings and up, newest first", async () => {
    const { source, readLines } = sourceOver(aLog(20));
    renderViewer(source);

    await screen.findByRole("list", { name: "Log lines" });
    expect(readLines).toHaveBeenCalledWith({
      level: "warn",
      text: undefined,
      after: undefined,
      limit: LOG_PAGE_SIZE,
    });
    expect(shownTexts()).toEqual(["line 20", "line 15", "line 10", "line 5"]);
  });

  it("shows errors alone, or every line, as the level filter says", async () => {
    const user = setupUser();
    const { source, readLines } = sourceOver(aLog(20));
    renderViewer(source);
    await screen.findByRole("list", { name: "Log lines" });

    await pickLevel(user, "Errors");
    await waitFor(() => expect(shownTexts()).toEqual(["line 20", "line 10"]));
    expect(readLines).toHaveBeenLastCalledWith(expect.objectContaining({ level: "error" }));

    await pickLevel(user, "Everything");
    await waitFor(() => expect(shownTexts()).toHaveLength(20));
    expect(readLines).toHaveBeenLastCalledWith(expect.objectContaining({ level: undefined }));
    expect(shownTexts()[0]).toBe("line 20");

    await pickLevel(user, "Warnings and up");
    await waitFor(() => expect(shownTexts()).toEqual(["line 20", "line 15", "line 10", "line 5"]));
  });

  it("searches as the words are typed", async () => {
    const user = setupUser();
    const { source, readLines } = sourceOver(aLog(20));
    renderViewer(source);
    await screen.findByRole("list", { name: "Log lines" });

    await fill(user, screen.getByRole("textbox", { name: "Search the log" }), "line 1");
    await waitFor(() => expect(shownTexts()).toEqual(["line 15", "line 10"]));
    expect(readLines).toHaveBeenLastCalledWith(expect.objectContaining({ text: "line 1" }));

    await fill(user, screen.getByRole("textbox", { name: "Search the log" }), "no such line");
    expect(await screen.findByText("No line matches the search.")).toBeInTheDocument();
  });

  it("downloads the log as it is, under its own name", async () => {
    const user = setupUser();
    const { source } = sourceOver(aLog(5));
    const read = renderViewer(source);
    await screen.findByRole("list", { name: "Log lines" });

    await user.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    expect(read).toHaveBeenCalledTimes(1);
    const [name, file] = saveMock.mock.calls[0];
    expect(name).toBe("import-sms-261006.log");
    expect(await file.text()).toBe("the whole log\n");
  });

  it("reads older lines when the list scrolls near its end", async () => {
    const user = setupUser();
    const { source, readLines } = sourceOver(aLog(LOG_PAGE_SIZE + 50));
    renderViewer(source);
    await screen.findByRole("list", { name: "Log lines" });
    await pickLevel(user, "Everything");
    await waitFor(() => expect(shownTexts()).toHaveLength(LOG_PAGE_SIZE));
    expect(shownTexts().at(-1)).toBe("line 51");

    scrollNear(screen.getByRole("list", { name: "Log lines" }).parentElement as HTMLElement);

    await waitFor(() => expect(shownTexts()).toHaveLength(LOG_PAGE_SIZE + 50));
    expect(readLines).toHaveBeenLastCalledWith(expect.objectContaining({ after: 5100 }));
    expect(shownTexts().at(-1)).toBe("line 1");
  });
});
