/**
 * A time limit on one request: a signal that aborts when the caller's signal
 * aborts or when `ms` have passed, whichever comes first.
 *
 * A timer and an `AbortController` rather than `AbortSignal.timeout` and
 * `AbortSignal.any`, which older WebViews lack, and a timer is what Vitest's
 * fake timers control.
 */
type TimeLimit = {
  /** The signal to hand `fetch`. */
  signal: AbortSignal;
  /** True when the limit, not the caller, aborted the signal. */
  timedOut: () => boolean;
  /**
   * Stop the clock and keep the caller's link, for a request whose answer
   * may take as long as it needs once it has started to arrive.
   */
  stopClock: () => void;
  /** Stop the clock and let go of the caller's signal. Call it once the request is over. */
  dispose: () => void;
};

export function startTimeLimit(ms: number, caller?: AbortSignal): TimeLimit {
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    if (controller.signal.aborted) return;
    timedOut = true;
    controller.abort();
  }, ms);
  const onCallerAbort = () => controller.abort(caller?.reason);
  if (caller?.aborted) {
    onCallerAbort();
  } else {
    caller?.addEventListener("abort", onCallerAbort, { once: true });
  }
  return {
    signal: controller.signal,
    timedOut: () => timedOut,
    stopClock: () => clearTimeout(timer),
    dispose: () => {
      clearTimeout(timer);
      caller?.removeEventListener("abort", onCallerAbort);
    },
  };
}
