/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createExternalStore, useExternalStore } from "./externalStore";

describe("createExternalStore", () => {
  it("holds the value it was given until set replaces it", () => {
    const store = createExternalStore(1);
    expect(store.get()).toBe(1);
    store.set(2);
    expect(store.get()).toBe(2);
  });

  it("tells every subscriber of each change, and stops telling one once it unsubscribes", () => {
    const store = createExternalStore("a");
    const first: string[] = [];
    const second: string[] = [];
    const unsubscribe = store.subscribe(() => first.push(store.get()));
    store.subscribe(() => second.push(store.get()));
    store.set("b");
    unsubscribe();
    store.set("c");
    expect(first).toEqual(["b"]);
    expect(second).toEqual(["b", "c"]);
  });

  it("tells no one when set is given the value it already holds", () => {
    const value = { n: 1 };
    const store = createExternalStore(value);
    let calls = 0;
    store.subscribe(() => {
      calls += 1;
    });
    store.set(value);
    expect(calls).toBe(0);
    store.set({ n: 1 });
    expect(calls).toBe(1);
  });
});

describe("useExternalStore", () => {
  it("re-renders with the selected part whenever the store changes", () => {
    const store = createExternalStore({ count: 1, label: "x" });
    const { result } = renderHook(() => useExternalStore(store, (state) => state.count));
    expect(result.current).toBe(1);
    act(() => store.set({ count: 2, label: "x" }));
    expect(result.current).toBe(2);
  });
});
