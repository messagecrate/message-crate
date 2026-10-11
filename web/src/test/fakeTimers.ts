/**
 * Moving Vitest's fake clock in a test that renders React.
 *
 * A test that faked the clock with `vi.useFakeTimers` moves it with
 * `advanceTimersAndSettle`, never with a real wait, so a debounce or a poll
 * fires when the test says and not when a busy machine gets to it.
 */

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
