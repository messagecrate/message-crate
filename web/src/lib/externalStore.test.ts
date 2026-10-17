import { describe, expect, it } from "vitest";
import { createExternalStore } from "./externalStore";

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
