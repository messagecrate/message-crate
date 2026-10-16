/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LocalServerNotice } from "../lib/desktop/localServer";
import { deferred } from "../test/deferred";
import { settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { setupUser } from "../test/user";
import NoticeBar from "./NoticeBar";

const desktop = vi.hoisted(() => ({
  isTauri: true,
  handler: null as ((notice: LocalServerNotice) => void) | null,
  unlisten: vi.fn(),
  startLocalServer: vi.fn(),
}));

vi.mock("../lib/tauriCheck", () => ({ isTauri: () => desktop.isTauri }));

vi.mock("../lib/desktop/localServer", () => ({
  onLocalServerNotice: async (handler: (notice: LocalServerNotice) => void) => {
    desktop.handler = handler;
    return desktop.unlisten;
  },
}));

vi.mock("../lib/localServer", () => ({
  startLocalServer: () => desktop.startLocalServer(),
}));

/** The desktop app sends `notice` to the window. */
function tell(notice: LocalServerNotice) {
  const handler = desktop.handler;
  if (handler === null) throw new Error("the bar is not listening");
  act(() => handler(notice));
}

beforeEach(() => {
  startFakeClock();
  desktop.isTauri = true;
  desktop.handler = null;
  desktop.unlisten.mockReset();
  desktop.startLocalServer.mockReset();
});

afterEach(() => {
  cleanup();
  stopFakeClock();
});

describe("NoticeBar", () => {
  it("says the server is being restarted and clears itself once it answers", async () => {
    const { container } = render(<NoticeBar />);
    await settle();
    expect(container).toBeEmptyDOMElement();

    tell({ state: "restarting" });
    expect(screen.getByRole("status")).toHaveTextContent(
      "The server stopped answering. Restarting it…",
    );

    tell({ state: "answering" });
    expect(container).toBeEmptyDOMElement();
  });

  it("stays when the restart fails, says why, and tries again", async () => {
    const { container } = render(<NoticeBar />);
    await settle();
    tell({ state: "restarting" });
    tell({
      state: "failed",
      message: "Message Crate stopped 4 times within 10 minutes.",
      details: "panicked",
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The server stopped answering and could not be started again. Message Crate stopped 4 times within 10 minutes.",
    );

    const started = deferred<{ status: "starting"; first_time: boolean }>();
    desktop.startLocalServer.mockReturnValue(started.promise);
    await setupUser().click(screen.getByRole("button", { name: "Try again" }));
    expect(desktop.startLocalServer).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Restarting it…");
    started.resolve({ status: "starting", first_time: false });
    await settle();
    expect(screen.getByRole("status")).toHaveTextContent("Restarting it…");

    tell({ state: "answering" });
    expect(container).toBeEmptyDOMElement();
  });

  it("says why when the app cannot be asked to start the server", async () => {
    render(<NoticeBar />);
    await settle();
    tell({ state: "failed", message: "Message Crate took too long to start.", details: "" });

    desktop.startLocalServer.mockRejectedValue(new Error("The server program is missing"));
    await setupUser().click(screen.getByRole("button", { name: "Try again" }));
    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent("Message Crate could not be started.");
  });

  it("stops listening when it goes", async () => {
    const { unmount } = render(<NoticeBar />);
    await settle();
    unmount();
    expect(desktop.unlisten).toHaveBeenCalledTimes(1);
  });

  it("listens for nothing in the browser", async () => {
    desktop.isTauri = false;
    const { container } = render(<NoticeBar />);
    await settle();
    expect(desktop.handler).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });
});
