import { startTimeLimit } from "./timeLimit";

/**
 * How long `pageAnswers` waits. Nothing shows in a failed screen's place
 * while it asks, so a page that stalls counts as one that doesn't answer.
 */
export const PAGE_ANSWER_TIMEOUT_MS = 5_000;

/**
 * Load the whole app again from where it came from, as the browser's reload
 * does. Its own module so a test can stand in for it: jsdom's
 * `location.reload` cannot be replaced.
 */
export function reloadPage(): void {
  location.reload();
}

/**
 * Whether the place this page was loaded from answers for it now, within
 * `PAGE_ANSWER_TIMEOUT_MS`. On the website that is the server, which may be
 * down, and a reload then leaves the browser's own "Unable to connect" page
 * in place of the app.
 */
export async function pageAnswers(): Promise<boolean> {
  const limit = startTimeLimit(PAGE_ANSWER_TIMEOUT_MS);
  try {
    const res = await fetch(location.href.split("#")[0], {
      cache: "no-store",
      signal: limit.signal,
    });
    return res.ok;
  } catch {
    return false;
  } finally {
    limit.dispose();
  }
}
