import { act as rtlAct } from "@testing-library/react";
import { beforeEach } from "vitest";

/** The signal of the test running now, which Vitest aborts when the test times out. */
let testSignal: AbortSignal | null = null;

// Importing this module registers the hook in the importing test file.
beforeEach(({ signal }) => {
  testSignal = signal;
});

/** Rejects when `signal` aborts. */
function aborted(signal: AbortSignal): Promise<never> {
  return new Promise<never>((_, reject) => {
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  });
}

/**
 * Testing Library's `act`, given up when the test times out. React counts the
 * `act` scopes open across the file, and renders inside a scope wait for every
 * scope to close. A test that times out while it awaits a held call inside
 * `act` would leave its scope open, and every later test's `render` or
 * `renderHook` would then draw nothing. Racing the call against the test's
 * signal closes the scope when the test times out, so that failure stays with
 * that test.
 *
 * An `afterEach` that runs after the test timed out finds the signal already
 * aborted. Its `act` runs without the race, so it can still end what the test
 * left running.
 */
export function act<T>(callback: () => Promise<T>): Promise<T>;
export function act(callback: () => void): void;
export function act(callback: () => unknown): unknown {
  const signal = testSignal;
  return rtlAct(() => {
    const done = callback();
    if (!(done instanceof Promise) || signal === null || signal.aborted) return done;
    return Promise.race([done, aborted(signal)]);
  });
}
