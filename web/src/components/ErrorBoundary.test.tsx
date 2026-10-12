/** @vitest-environment jsdom */

import { render, screen } from "@testing-library/react";
import { Suspense } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { problemFromBody } from "../lib/api";
import { ChunkLoadError, lazyChunk } from "../lib/chunkLoad";
import { pageAnswers, reloadPage } from "../lib/pageReload";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { setupUser } from "../test/user";
import ErrorBoundary from "./ErrorBoundary";

vi.mock("../lib/pageReload", () => ({ reloadPage: vi.fn(), pageAnswers: vi.fn() }));

/** A child that throws `error` while it renders. */
function Throws({ error }: { error: unknown }): never {
  throw error;
}

/** What `lazyChunk` rejects with when the download of `chunk` fails. */
function chunkFailed(chunk: string): ChunkLoadError {
  return new ChunkLoadError(chunk, new TypeError("Failed to fetch dynamically imported module"));
}

beforeEach(() => {
  startFakeClock();
  sessionStorage.clear();
  // The page this app came from answers, unless a test says otherwise.
  vi.mocked(pageAnswers).mockResolvedValue(true);
  // React reports every error a boundary catches on the console; the test
  // asserts what the person sees instead.
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.mocked(reloadPage).mockClear();
  vi.restoreAllMocks();
  stopFakeClock();
});

describe("ErrorBoundary", () => {
  it("shows what went wrong and a Reload button in place of a screen that threw", async () => {
    render(
      <ErrorBoundary scope="screen">
        <Throws error={new Error("cannot read properties of undefined")} />
      </ErrorBoundary>,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong on this screen");
    expect(alert).toHaveTextContent("Cannot read properties of undefined");

    await setupUser().click(screen.getByRole("button", { name: "Reload" }));
    expect(reloadPage).toHaveBeenCalledTimes(1);
  });

  it("shows the request id of a server failure a screen threw", () => {
    const failure = problemFromBody(
      500,
      JSON.stringify({
        type: "about:blank",
        title: "Internal Server Error",
        status: 500,
        detail: "internal server error",
        request_id: "4f3c2a1b-0000-4000-8000-000000000001",
      }),
    );
    render(
      <ErrorBoundary scope="page">
        <Throws error={failure} />
      </ErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Internal server error (request 4f3c2a1b-0000-4000-8000-000000000001)",
    );
  });

  it("shows the screen again once the person goes somewhere else", () => {
    const { rerender } = render(
      <ErrorBoundary scope="screen" resetKey="/settings">
        <Throws error={new Error("broken")} />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();

    rerender(
      <ErrorBoundary scope="screen" resetKey="/contacts">
        <p>Contacts</p>
      </ErrorBoundary>,
    );

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText("Contacts")).toBeInTheDocument();
  });

  it("reloads the page once for a screen that didn't load", async () => {
    render(
      <ErrorBoundary scope="screen">
        <Throws error={chunkFailed("SettingsScreen")} />
      </ErrorBoundary>,
    );

    await settle();
    expect(reloadPage).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("keeps the app and says why when the page itself no longer answers", async () => {
    vi.mocked(pageAnswers).mockResolvedValue(false);
    render(
      <ErrorBoundary scope="screen">
        <Throws error={chunkFailed("SettingsScreen")} />
      </ErrorBoundary>,
    );

    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent("This screen didn't load");
    expect(reloadPage).not.toHaveBeenCalled();
  });

  it("says a panel failed, in the panel's place", () => {
    render(
      <ErrorBoundary scope="panel">
        <Throws error={new Error("broken")} />
      </ErrorBoundary>,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong in this panel");
    expect(alert).toHaveTextContent("Reload to try again.");
  });

  it("reloads for a lazy chunk whose download fails, and again once it has arrived", async () => {
    const Missing = lazyChunk<object>("SettingsScreen", () =>
      Promise.reject(new TypeError("Failed to fetch dynamically imported module")),
    );
    const first = render(
      <ErrorBoundary scope="screen">
        <Suspense fallback={null}>
          <Missing />
        </Suspense>
      </ErrorBoundary>,
    );
    await settle();
    expect(reloadPage).toHaveBeenCalledTimes(1);
    first.unmount();

    const Arrives = lazyChunk("SettingsScreen", async () => ({
      default: () => <p>Settings</p>,
    }));
    render(
      <Suspense fallback={null}>
        <Arrives />
      </Suspense>,
    );
    await settle();
    expect(screen.getByText("Settings")).toBeInTheDocument();

    render(
      <ErrorBoundary scope="screen">
        <Throws error={chunkFailed("SettingsScreen")} />
      </ErrorBoundary>,
    );
    await settle();
    expect(reloadPage).toHaveBeenCalledTimes(2);
  });

  it("does not reload again for a chunk that failed, when another chunk loads in between", async () => {
    const first = render(
      <ErrorBoundary scope="panel">
        <Throws error={chunkFailed("AdvancedSearchForm")} />
      </ErrorBoundary>,
    );
    await settle();
    expect(reloadPage).toHaveBeenCalledTimes(1);
    first.unmount();

    const Settings = lazyChunk("SettingsScreen", async () => ({
      default: () => <p>Settings</p>,
    }));
    render(
      <Suspense fallback={null}>
        <Settings />
      </Suspense>,
    );
    await settle();
    expect(screen.getByText("Settings")).toBeInTheDocument();

    render(
      <ErrorBoundary scope="panel">
        <Throws error={chunkFailed("AdvancedSearchForm")} />
      </ErrorBoundary>,
    );
    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent("This panel didn't load");
    expect(reloadPage).toHaveBeenCalledTimes(1);
  });

  it("says the screen didn't load when it still doesn't after the reload", async () => {
    const first = render(
      <ErrorBoundary scope="screen">
        <Throws error={chunkFailed("TrashScreen")} />
      </ErrorBoundary>,
    );
    await settle();
    expect(reloadPage).toHaveBeenCalledTimes(1);
    first.unmount();
    vi.mocked(reloadPage).mockClear();

    render(
      <ErrorBoundary scope="screen">
        <Throws error={chunkFailed("TrashScreen")} />
      </ErrorBoundary>,
    );

    await settle();
    const alert = screen.getByRole("alert");
    expect(reloadPage).not.toHaveBeenCalled();
    expect(alert).toHaveTextContent("This screen didn't load");
    expect(alert).toHaveTextContent("Failed to fetch dynamically imported module");
  });
});

describe("pageAnswers", () => {
  it("counts a page that never answers as one that doesn't", async () => {
    const { pageAnswers: realPageAnswers, PAGE_ANSWER_TIMEOUT_MS } =
      await vi.importActual<typeof import("../lib/pageReload")>("../lib/pageReload");
    vi.stubGlobal(
      "fetch",
      vi.fn(
        (_url: string, init: RequestInit) =>
          new Promise((_resolve, reject) => {
            init.signal?.addEventListener("abort", () => reject(init.signal?.reason));
          }),
      ),
    );

    const answer = realPageAnswers();
    await vi.advanceTimersByTimeAsync(PAGE_ANSWER_TIMEOUT_MS);

    expect(await answer).toBe(false);
  });
});
