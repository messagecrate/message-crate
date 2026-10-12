/** @vitest-environment jsdom */

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useAsyncAction } from "./useAsyncAction";

describe("useAsyncAction", () => {
  it("sets busy while running and clears on success", async () => {
    const { result } = renderHook(() => useAsyncAction());
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });

    let runPromise!: Promise<void>;
    act(() => {
      runPromise = result.current.run(async () => {
        await gate;
      });
    });

    await waitFor(() => expect(result.current.busy).toBe(true));
    expect(result.current.error).toBe("");

    await act(async () => {
      release();
      await runPromise;
    });

    expect(result.current.busy).toBe(false);
    expect(result.current.error).toBe("");
  });

  it("captures an error as a line of its own, and clearError resets it", async () => {
    const { result } = renderHook(() => useAsyncAction());

    await act(async () => {
      await result.current.run(async () => {
        throw new Error("boom");
      });
    });

    expect(result.current.busy).toBe(false);
    expect(result.current.error).toBe("Boom");

    act(() => {
      result.current.clearError();
    });
    expect(result.current.error).toBe("");
  });
});
