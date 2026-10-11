/** @vitest-environment jsdom */

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "./api";
import { SessionQueryClientProvider } from "./sessionQueryClient";
import { onSessionRefused } from "./sessionRefusal";

describe("SessionQueryClientProvider", () => {
  const refused = vi.fn();
  let stop: () => void = () => {};

  beforeEach(() => {
    refused.mockReset();
    stop = onSessionRefused(refused);
  });

  afterEach(() => {
    stop();
  });

  it("reports a query the server refuses as the session ending", async () => {
    renderHook(
      () =>
        useQuery({
          queryKey: ["ended"],
          queryFn: async () => {
            throw new ApiError(401, "expired session token");
          },
        }),
      { wrapper: SessionQueryClientProvider },
    );

    await waitFor(() => expect(refused).toHaveBeenCalledTimes(1));
    // No token: the refusal is of the session in use now.
    expect(refused).toHaveBeenCalledWith();
  });

  /** Run a mutation that fails with `error` through the provider's client. */
  async function failMutation(error: ApiError) {
    const { result } = renderHook(() => useQueryClient(), {
      wrapper: SessionQueryClientProvider,
    });
    const mutation = result.current.getMutationCache().build(result.current, {
      mutationFn: async () => {
        throw error;
      },
    });
    await act(async () => {
      await mutation.execute(undefined).catch(() => {});
    });
  }

  it("reports a mutation the server refuses as the session ending", async () => {
    await failMutation(new ApiError(401, "expired session token"));

    expect(refused).toHaveBeenCalledTimes(1);
  });

  it("does not end the session over a mistyped password", async () => {
    await failMutation(
      new ApiError(401, "current password is incorrect", {
        type: "https://messagecrate.app/problems/invalid-credentials",
        title: "Invalid credentials",
        status: 401,
      }),
    );

    expect(refused).not.toHaveBeenCalled();
  });

  it("keeps one client, and its cache, across renders", async () => {
    const queryFn = vi.fn(async () => "value");
    const { result, rerender } = renderHook(
      () => useQuery({ queryKey: ["kept"], queryFn, staleTime: Number.POSITIVE_INFINITY }),
      { wrapper: SessionQueryClientProvider },
    );
    await waitFor(() => expect(result.current.data).toBe("value"));

    rerender();

    expect(result.current.data).toBe("value");
    expect(queryFn).toHaveBeenCalledTimes(1);
  });
});
