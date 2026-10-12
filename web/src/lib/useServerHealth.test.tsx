/** @vitest-environment jsdom */

import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advanceTimersAndSettle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { HEALTH_URL_DEBOUNCE_MS } from "./serverHealth";
import { useServerHealth } from "./useServerHealth";

describe("useServerHealth", () => {
  beforeEach(() => {
    startFakeClock();
  });

  afterEach(() => {
    stopFakeClock();
  });

  it("does not fetch when probing is disabled", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    const { result } = renderHook(() => useServerHealth(null));
    expect(result.current).toBe("unknown");

    await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS + 50);

    expect(fetchMock).not.toHaveBeenCalled();
    expect(result.current).toBe("unknown");
  });

  it("probes this origin after debounce when the URL is blank", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    const { result } = renderHook(() => useServerHealth(""));
    expect(result.current).toBe("unknown");

    await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS - 1);
    expect(result.current).toBe("unknown");
    expect(fetchMock).not.toHaveBeenCalled();

    await advanceTimersAndSettle(1);
    expect(result.current).toBe("ok");
    expect(fetchMock).toHaveBeenCalledWith(
      "/health",
      expect.objectContaining({ method: "GET", cache: "no-store" }),
    );
  });

  it("marks fail for a non-http URL without calling fetch", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    const { result } = renderHook(() => useServerHealth("not-a-url"));
    await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS - 1);
    expect(result.current).toBe("unknown");

    await advanceTimersAndSettle(1);
    expect(result.current).toBe("fail");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
