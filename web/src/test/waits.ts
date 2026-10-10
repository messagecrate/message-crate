/**
 * The wait a test gives a screen to reach a state that can take longer than
 * the 1000 ms `findBy*` and `waitFor` allow by default, in their options.
 *
 * Two things take that long. Drawing a list's first rows takes 100 to 230 ms
 * on an idle machine and crossed 1000 ms at a load average near 160 (#1654).
 * A screen that polls shows its next state only after its poll interval,
 * such as the Demo Account card's 1500 ms while a build runs, or two of the
 * Tools Directory check's 1000 ms polls. 4000 ms covers both, leaves the rest
 * of a test inside Vitest's default 5000 ms budget, and the wait still ends
 * the moment the state is reached.
 *
 * A longer wait is never the fix for a test that failed for another reason.
 */
export const SLOW_STATE_WAIT: Readonly<{ timeout: number }> = { timeout: 4000 };
