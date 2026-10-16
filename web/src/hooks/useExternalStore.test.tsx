/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createExternalStore } from "../lib/externalStore";
import { useExternalStore } from "./useExternalStore";

describe("useExternalStore", () => {
  it("re-renders with the selected part whenever the store changes", () => {
    const store = createExternalStore({ count: 1, label: "x" });
    const { result } = renderHook(() => useExternalStore(store, (state) => state.count));
    expect(result.current).toBe(1);
    act(() => store.set({ count: 2, label: "x" }));
    expect(result.current).toBe(2);
  });
});
