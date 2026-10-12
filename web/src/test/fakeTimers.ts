/**
 * Moving Vitest's fake clock in a test that renders React.
 *
 * A test that faked the clock with `vi.useFakeTimers` moves it with
 * `vi.advanceTimersByTimeAsync` inside `act`, never with a real wait, so a
 * debounce or a poll fires when the test says and not when a busy machine
 * gets to it. `advanceTimersAndSettle` does that and also lets what the
 * timers started land.
 */

import { defaultScheduler, notifyManager } from "@tanstack/react-query";
import { act } from "@testing-library/react";
import { vi } from "vitest";

/**
 * Let every timer due within `ms` fire, then let what those timers started
 * land: the request a timer sent, its answer, and the render that answer
 * causes.
 *
 * The first `act` moves the clock and runs the timers. The second moves it by
 * nothing, which runs the promises and the zero-delay timers the first one
 * queued, so the screen has drawn the answer before the test reads it.
 */
export async function advanceTimersAndSettle(ms: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

/**
 * Let Testing Library's events run on Vitest's fake clock. Testing Library
 * ends each event with a setTimeout(0), which it moves past itself only
 * through a `jest` global, so this lends it one that moves Vitest's clock.
 * Call it after `vi.useFakeTimers` fakes `setTimeout`; `vi.unstubAllGlobals`
 * takes it back.
 */
export function lendTestingLibraryTheFakeClock(): void {
  vi.stubGlobal("jest", { advanceTimersByTime: (ms: number) => vi.advanceTimersByTime(ms) });
}

/**
 * What a test that renders React runs on the fake clock: the timers a
 * debounce, a poll, TanStack Query's notifications, and React Aria's
 * animation frames use, and `Date`, so a screen that measures how long
 * something has run (such as how long a poll keeps going) reads the fake
 * clock too.
 */
const FAKED_TIMERS = [
  "setTimeout",
  "clearTimeout",
  "setInterval",
  "clearInterval",
  "requestAnimationFrame",
  "cancelAnimationFrame",
  "Date",
] as const;

/**
 * Where the fake clock starts, the same in every run, so a screen that shows
 * or compares against today shows the same day whenever the test runs.
 */
export const FAKE_CLOCK_START = new Date("2026-06-15T12:00:00Z");

/**
 * Put the test on the fake clock and lend it to Testing Library, so nothing
 * the screen does waits on real time. Call it before `render`, usually in
 * `beforeEach`, and `stopFakeClock` in `afterEach`.
 */
export function startFakeClock(): void {
  vi.useFakeTimers({ toFake: [...FAKED_TIMERS], now: FAKE_CLOCK_START });
  lendTestingLibraryTheFakeClock();
  // TanStack Query tells a screen about an answer from a setTimeout(0). The
  // fake clock runs a zero-delay timer set while it is moving 1 ms later, so
  // an answer a timer started (a poll's) would land only once the clock moved
  // again. A microtask lands it within the same `settle`.
  notifyManager.setScheduler(queueMicrotask);
}

/**
 * Take the fake clock back, and every global the test stubbed with it
 * (`vi.unstubAllGlobals`).
 */
export function stopFakeClock(): void {
  notifyManager.setScheduler(defaultScheduler);
  vi.useRealTimers();
  vi.unstubAllGlobals();
}

/**
 * Let everything already started land, without moving the clock: a mock's
 * answer, the render it causes, a transition, the refetch a mutation's
 * answer starts, and whatever those start in turn, up to the next timer due
 * later than now. Needs `startFakeClock`.
 *
 * This is how the test is told that the screen has drawn what it was waiting
 * for: when `settle` returns, the test reads the screen at once with
 * `getBy…` or `queryBy…`. A delay the screen waits out on purpose (a
 * debounce, a poll) is moved past with `advanceTimersAndSettle(ms)` instead.
 * See `web/src/test/README.md`.
 */
export async function settle(): Promise<void> {
  await advanceTimersAndSettle(0);
}

/**
 * Run what waits on the next animation frame, such as React Aria putting
 * focus back on a menu's trigger after the menu closes. `settle` never runs
 * a frame, because it doesn't move the clock. Needs `startFakeClock`. See
 * "An animation frame" in `web/src/test/README.md`.
 */
export function advanceToNextFrame(): void {
  act(() => {
    vi.advanceTimersToNextFrame();
  });
}
