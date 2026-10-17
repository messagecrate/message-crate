/** @vitest-environment jsdom */

/**
 * The query client the app builds: which failures it tries again, and how
 * long it waits first. The route hooks drive it, since the client acts only
 * on their queries and writes.
 */

import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useRouteMutation, useRouteQuery } from "../hooks/useRouteQuery";
import { advanceTimersAndSettle, settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";
import { testQueryClient } from "../test/providers";
import {
  ApiError,
  noAnswerError,
  problemFromBody,
  REQUEST_TIMEOUT_MS,
  requestTimeoutError,
} from "./api";
import { createQueryClient } from "./routeQuery";

vi.mock("../hooks/useAuth", async () =>
  (await import("../test/providers")).authModule({ accountId: 7 }),
);

let client: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  startFakeClock();
  client = testQueryClient();
});

afterEach(() => {
  stopFakeClock();
});

describe("createQueryClient", () => {
  /**
   * How many times a query that always fails with `error` asks the server,
   * under the app's own client. The retry delay is zero so a retry, when
   * there is one, happens before the error is reported.
   */
  async function callsUntilError(error: Error): Promise<number> {
    client = createQueryClient();
    const fetchGroups = vi.fn(async () => {
      throw error;
    });
    const { result } = renderHook(
      () => useRouteQuery(["contact-groups"], fetchGroups, { retryDelay: 0 }),
      { wrapper },
    );
    await settle();
    expect(result.current.isError).toBe(true);
    return fetchGroups.mock.calls.length;
  }

  it.each([
    [403, "Forbidden"],
    [404, "Not Found"],
    [422, "Unprocessable Entity"],
  ])("asks once when the server answers %i %s", async (status, reason) => {
    expect(await callsUntilError(new ApiError(status, reason))).toBe(1);
  });

  it("asks again once when the server answers 500 Internal Server Error", async () => {
    expect(await callsUntilError(new ApiError(500, "Internal Server Error"))).toBe(2);
  });

  it("asks once when the server did not answer within the time limit", async () => {
    expect(await callsUntilError(requestTimeoutError(REQUEST_TIMEOUT_MS))).toBe(1);
  });

  it("asks again once when the request never reached the server", async () => {
    expect(await callsUntilError(noAnswerError())).toBe(2);
  });

  /** The server's answer for a database too busy to take the request. */
  function databaseBusy(retryAfter: number): ApiError {
    return problemFromBody(
      503,
      JSON.stringify({
        type: "https://messagecrate.app/problems/database-busy",
        title: "Database busy",
        status: 503,
        detail: "The database is busy, so try again in a few seconds.",
        retry_after: retryAfter,
      }),
    );
  }

  describe("with a database too busy to answer", () => {
    it("waits the seconds the server named before it asks again, once", async () => {
      client = createQueryClient();
      const fetchGroups = vi
        .fn<() => Promise<string[]>>()
        .mockRejectedValueOnce(databaseBusy(5))
        .mockResolvedValueOnce(["Family"]);
      const { result } = renderHook(() => useRouteQuery(["contact-groups"], fetchGroups), {
        wrapper,
      });

      await settle();
      expect(fetchGroups).toHaveBeenCalledTimes(1);

      await advanceTimersAndSettle(4_999);
      expect(fetchGroups).toHaveBeenCalledTimes(1);

      // The retry's answer lands with the move that starts it: startFakeClock
      // has TanStack Query tell the hook in a microtask, not a zero-delay timer.
      await advanceTimersAndSettle(1);
      expect(fetchGroups).toHaveBeenCalledTimes(2);
      expect(result.current.data).toEqual(["Family"]);
    });

    it("does not send a write again", async () => {
      client = createQueryClient();
      const write = vi.fn(async () => {
        throw databaseBusy(5);
      });
      const { result } = renderHook(() => useRouteMutation({ mutationFn: write }), { wrapper });

      await act(async () => {
        await result.current.mutateAsync().catch(() => {});
      });
      await advanceTimersAndSettle(60_000);
      expect(write).toHaveBeenCalledTimes(1);
      expect(result.current.error).toBeInstanceOf(ApiError);
    });
  });
});
