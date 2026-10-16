/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setBaseUrl } from "../lib/api";
import { DEFAULT_TAURI_SERVER_URL } from "../lib/authGuards";
import type { LocalServerStatus, LocalServerNotice as Notice } from "../lib/desktop/localServer";
import { deferred } from "../test/deferred";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { setupUser } from "../test/user";
import LocalServerNotice from "./LocalServerNotice";

const desktop = vi.hoisted(() => ({
  isTauri: true,
  handler: null as ((notice: Notice) => void) | null,
  unlisten: vi.fn(),
  status: vi.fn(),
  startLocalServer: vi.fn(),
}));

vi.mock("../lib/tauriCheck", () => ({ isTauri: () => desktop.isTauri }));

vi.mock("../lib/desktop/localServer", () => ({
  onLocalServerNotice: async (handler: (notice: Notice) => void) => {
    desktop.handler = handler;
    return desktop.unlisten;
  },
  invokeLocalServerStatus: () => desktop.status(),
}));

vi.mock("../lib/localServer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/localServer")>()),
  startLocalServer: () => desktop.startLocalServer(),
}));

const READY: LocalServerStatus = { status: "ready", started_by_app: true };
const FOUR_TIMES = "Message Crate stopped 4 times within 10 minutes.";

/** The desktop app sends `notice` to the window. */
function tell(notice: Notice) {
  const handler = desktop.handler;
  if (handler === null) throw new Error("the bar is not listening");
  act(() => handler(notice));
}

/** Render the notice, and let it start listening. */
async function renderBar() {
  const view = render(<LocalServerNotice />);
  await settle();
  return view;
}

beforeEach(() => {
  startFakeClock();
  setBaseUrl(DEFAULT_TAURI_SERVER_URL);
  desktop.isTauri = true;
  desktop.handler = null;
  desktop.unlisten.mockReset();
  desktop.status.mockReset();
  desktop.status.mockResolvedValue(READY);
  desktop.startLocalServer.mockReset();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
  setBaseUrl("");
});

describe("LocalServerNotice", () => {
  it("says the server stopped answering and is restarting, and clears itself once it answers", async () => {
    const { container } = await renderBar();
    expect(container).toBeEmptyDOMElement();

    tell({ state: "restarting", cause: "unanswered" });
    expect(screen.getByRole("status")).toHaveTextContent(
      "The server stopped answering. Restarting it…",
    );

    tell({ state: "answering" });
    expect(container).toBeEmptyDOMElement();
  });

  it("says the server stopped, not that it stopped answering, when it exited", async () => {
    await renderBar();
    tell({ state: "restarting", cause: "exited" });
    expect(screen.getByRole("status")).toHaveTextContent("The server stopped. Restarting it…");
  });

  it("stays when the restart fails, says why, and tries again", async () => {
    const { container } = await renderBar();
    tell({ state: "restarting", cause: "exited" });
    tell({ state: "failed", message: FOUR_TIMES, details: "panicked" });
    expect(screen.getByRole("alert")).toHaveTextContent(`${FOUR_TIMES}Try again`);

    const started = deferred<LocalServerStatus>();
    desktop.startLocalServer.mockReturnValue(started.promise);
    await setupUser().click(screen.getByRole("button", { name: "Try again" }));
    expect(desktop.startLocalServer).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Starting the server…");
    started.resolve({ status: "starting", first_time: false });
    await settle();
    expect(screen.getByRole("status")).toHaveTextContent("Starting the server…");

    tell({ state: "answering" });
    expect(container).toBeEmptyDOMElement();
  });

  it("says why when the app cannot be asked to start the server", async () => {
    await renderBar();
    tell({ state: "failed", message: "Message Crate took too long to start.", details: "" });

    desktop.startLocalServer.mockRejectedValue(new Error("The server program is missing"));
    await setupUser().click(screen.getByRole("button", { name: "Try again" }));
    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Message Crate could not be started.Try again",
    );
  });

  it("shows a failure the app reports when the window opens after the notice was sent", async () => {
    desktop.status.mockResolvedValue({
      status: "failed",
      reason: "start_failed",
      message: FOUR_TIMES,
      details: "",
    });
    await renderBar();
    expect(screen.getByRole("alert")).toHaveTextContent(FOUR_TIMES);
  });

  it("takes the app's word over the state it read when it opened", async () => {
    const status = deferred<LocalServerStatus>();
    desktop.status.mockReturnValue(status.promise);
    const { container } = await renderBar();
    tell({ state: "answering" });
    status.resolve({ status: "failed", reason: "start_failed", message: FOUR_TIMES, details: "" });
    await settle();
    expect(container).toBeEmptyDOMElement();
  });

  it("says nothing while the window uses a Message Crate at another address", async () => {
    setBaseUrl("http://192.168.1.20:8080");
    const { container } = await renderBar();
    tell({ state: "failed", message: FOUR_TIMES, details: "" });
    expect(container).toBeEmptyDOMElement();
  });

  it("stops listening when it goes", async () => {
    const { unmount } = await renderBar();
    unmount();
    expect(desktop.unlisten).toHaveBeenCalledTimes(1);
  });

  it("listens for nothing in the browser", async () => {
    desktop.isTauri = false;
    const { container } = await renderBar();
    expect(desktop.handler).toBeNull();
    expect(desktop.status).not.toHaveBeenCalled();
    expect(container).toBeEmptyDOMElement();
  });
});
