/** @vitest-environment jsdom */

import { render, screen, waitFor } from "@testing-library/react";
import { Suspense } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { problemFromBody } from "../lib/api";
import { reloadPage } from "../lib/pageReload";
import { lazyScreen, ScreenLoadError } from "../lib/screenLoad";
import { setupUser } from "../test/user";
import ErrorBoundary from "./ErrorBoundary";

vi.mock("../lib/pageReload", () => ({ reloadPage: vi.fn() }));

/** A child that throws `error` while it renders. */
function Throws({ error }: { error: unknown }): never {
  throw error;
}

beforeEach(() => {
  sessionStorage.clear();
  // React reports every error a boundary catches on the console; the test
  // asserts what the person sees instead.
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.mocked(reloadPage).mockClear();
  vi.restoreAllMocks();
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

  it("reloads the page once for a screen that didn't load", () => {
    render(
      <ErrorBoundary scope="screen">
        <Throws
          error={new ScreenLoadError(new TypeError("Failed to fetch dynamically imported module"))}
        />
      </ErrorBoundary>,
    );

    expect(reloadPage).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reloads for a lazy screen whose download fails, and again after one that arrives", async () => {
    const Missing = lazyScreen<object>(() =>
      Promise.reject(new TypeError("Failed to fetch dynamically imported module")),
    );
    const first = render(
      <ErrorBoundary scope="screen">
        <Suspense fallback={null}>
          <Missing />
        </Suspense>
      </ErrorBoundary>,
    );
    await waitFor(() => expect(reloadPage).toHaveBeenCalledTimes(1));
    first.unmount();

    const Arrives = lazyScreen(async () => ({ default: () => <p>Settings</p> }));
    render(
      <Suspense fallback={null}>
        <Arrives />
      </Suspense>,
    );
    expect(await screen.findByText("Settings")).toBeInTheDocument();

    render(
      <ErrorBoundary scope="screen">
        <Throws error={new ScreenLoadError(new TypeError("Importing a module script failed."))} />
      </ErrorBoundary>,
    );
    expect(reloadPage).toHaveBeenCalledTimes(2);
  });

  it("says the screen didn't load when it still doesn't after the reload", () => {
    const failed = new ScreenLoadError(
      new TypeError("Failed to fetch dynamically imported module"),
    );
    const first = render(
      <ErrorBoundary scope="screen">
        <Throws error={failed} />
      </ErrorBoundary>,
    );
    first.unmount();
    vi.mocked(reloadPage).mockClear();

    render(
      <ErrorBoundary scope="screen">
        <Throws error={failed} />
      </ErrorBoundary>,
    );

    expect(reloadPage).not.toHaveBeenCalled();
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("This screen didn't load");
    expect(alert).toHaveTextContent("Failed to fetch dynamically imported module");
  });
});
