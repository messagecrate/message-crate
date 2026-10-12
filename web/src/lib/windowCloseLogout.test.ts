/** @vitest-environment jsdom */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useLogoutOnWindowClose } from "./windowCloseLogout";

const isTauri = vi.fn();
const onWindowCloseRequested = vi.fn();
const destroyWindow = vi.fn();

vi.mock("./tauriCheck", () => ({
  isTauri: () => isTauri(),
}));

vi.mock("./tauri", () => ({
  onWindowCloseRequested: (...args: unknown[]) => onWindowCloseRequested(...args),
  destroyWindow: () => destroyWindow(),
}));

type CloseHandler = (event: { preventDefault: () => void }) => Promise<void>;

/** Mount the hook in the desktop app and return the close handler it registered. */
async function mountInDesktop(revokeSession: () => Promise<void>) {
  let handler: CloseHandler | undefined;
  const unlisten = vi.fn();
  onWindowCloseRequested.mockImplementation(async (h: CloseHandler) => {
    handler = h;
    return unlisten;
  });
  const hook = renderHook(() => useLogoutOnWindowClose(revokeSession));
  await waitFor(() => expect(handler).toBeTypeOf("function"));
  if (!handler) throw new Error("expected a close handler");
  return { close: handler, unlisten, ...hook };
}

describe("useLogoutOnWindowClose", () => {
  beforeEach(() => {
    isTauri.mockReset().mockReturnValue(true);
    onWindowCloseRequested.mockReset();
    destroyWindow.mockReset().mockResolvedValue(undefined);
  });

  it("does not listen for a close outside the desktop app", async () => {
    isTauri.mockReturnValue(false);
    renderHook(() => useLogoutOnWindowClose(async () => {}));
    await act(async () => {});
    expect(onWindowCloseRequested).not.toHaveBeenCalled();
  });

  it("holds the window open until the session is revoked, then closes it", async () => {
    const order: string[] = [];
    const revoke = vi.fn(async () => {
      order.push("revoke");
    });
    destroyWindow.mockImplementation(async () => {
      order.push("destroy");
    });
    const { close } = await mountInDesktop(revoke);

    const preventDefault = vi.fn();
    await act(async () => {
      await close({ preventDefault });
    });

    expect(preventDefault).toHaveBeenCalled();
    expect(order).toEqual(["revoke", "destroy"]);
  });

  it("ignores a second close while the first is still logging out", async () => {
    let finishRevoke: () => void = () => {};
    const revoke = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishRevoke = resolve;
        }),
    );
    const { close } = await mountInDesktop(revoke);

    let first: Promise<void> = Promise.resolve();
    await act(async () => {
      first = close({ preventDefault: vi.fn() });
      await close({ preventDefault: vi.fn() });
    });
    expect(revoke).toHaveBeenCalledTimes(1);

    await act(async () => {
      finishRevoke();
      await first;
    });
    expect(destroyWindow).toHaveBeenCalledTimes(1);
  });

  it("allows another close attempt when closing the window fails", async () => {
    destroyWindow.mockRejectedValueOnce(new Error("denied")).mockResolvedValueOnce(undefined);
    const { close } = await mountInDesktop(async () => {});

    await act(async () => {
      await close({ preventDefault: vi.fn() });
    });
    expect(destroyWindow).toHaveBeenCalledTimes(1);

    await act(async () => {
      await close({ preventDefault: vi.fn() });
    });
    expect(destroyWindow).toHaveBeenCalledTimes(2);
  });

  it("stops listening once unmounted", async () => {
    const { unlisten, unmount } = await mountInDesktop(async () => {});
    unmount();
    expect(unlisten).toHaveBeenCalled();
  });
});
