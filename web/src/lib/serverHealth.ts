import { NEVER_FOLLOW } from "./api";
import { startTimeLimit } from "./timeLimit";

/** How long to wait after failure attempt `failureIndex` (0-based) before the next probe. */
export const HEALTH_BACKOFF_CAP_MS = 30_000;
export const HEALTH_SUCCESS_RECHECK_MS = 30_000;
export const HEALTH_URL_DEBOUNCE_MS = 400;
/** Give up on a single /health request so a black-holed host cannot leave the light grey. */
export const HEALTH_PROBE_TIMEOUT_MS = 8_000;
/** How long `checkServerHealthTwice` waits after a failed probe before it asks again. */
export const HEALTH_RECHECK_AFTER_FAILURE_MS = 1_000;

/** Progressive backoff: 1s → 2s → 4s → … capped at 30s. */
export function healthBackoffMs(failureIndex: number): number {
  const n = Math.max(0, Math.floor(failureIndex));
  const delay = 1000 * 2 ** n;
  return Math.min(delay, HEALTH_BACKOFF_CAP_MS);
}

export type ServerHealthStatus = "unknown" | "checking" | "ok" | "fail";

/**
 * URL to GET to ask whether the server can answer.
 * A blank value means this origin (same as the API client empty base URL).
 * Returns null when the value is not empty and not an absolute http(s) URL.
 */
export function healthProbeUrl(baseUrl: string): string | null {
  const trimmed = baseUrl.trim();
  if (!trimmed) return "/health";
  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return null;
    }
  } catch {
    return null;
  }
  return `${trimmed.replace(/\/+$/, "")}/health`;
}

/**
 * Ask GET /health whether the server can answer: the server says `ok` only
 * when it can reach its database, so the light agrees with the API calls.
 * Returns true only when the response is OK.
 *
 * Calls `fetch` itself rather than going through `serverApi.ts`: the address
 * may be one the person is still typing on the login screen, not the server
 * `apiClient` talks to. One of the two named exceptions in
 * `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */
export async function checkServerHealth(baseUrl: string, signal?: AbortSignal): Promise<boolean> {
  const url = healthProbeUrl(baseUrl);
  if (!url) return false;
  if (signal?.aborted) return false;

  const limit = startTimeLimit(HEALTH_PROBE_TIMEOUT_MS, signal);

  try {
    const res = await fetch(url, {
      method: "GET",
      signal: limit.signal,
      cache: "no-store",
      // The probe sends no credential, but it must agree with the calls that
      // do, so an address that redirects reads Disconnected here too, not
      // Connected beside a login that fails.
      redirect: NEVER_FOLLOW,
    });
    return res.ok;
  } catch {
    return false;
  } finally {
    limit.dispose();
  }
}

/**
 * Ask GET /health, and when it fails, ask once more
 * `HEALTH_RECHECK_AFTER_FAILURE_MS` later. Returns false only when both fail,
 * or when `signal` aborts.
 *
 * One failure is not enough to call a server unreachable: `/health` fails
 * when it cannot take a database connection within 1.5 seconds, which
 * happens while an import's write lock keeps every connection waiting,
 * though a `/v1` read would wait and succeed (#2718). The desktop app's watch
 * on its own server asks twice in a row for the same reason (#2204).
 */
export async function checkServerHealthTwice(
  baseUrl: string,
  signal?: AbortSignal,
): Promise<boolean> {
  if (await checkServerHealth(baseUrl, signal)) return true;
  if (!(await waitUnlessAborted(HEALTH_RECHECK_AFTER_FAILURE_MS, signal))) return false;
  return checkServerHealth(baseUrl, signal);
}

/** Wait `ms`. Resolves false at once when `signal` aborts first. */
function waitUnlessAborted(ms: number, signal?: AbortSignal): Promise<boolean> {
  return new Promise((resolve) => {
    if (signal?.aborted) {
      resolve(false);
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      resolve(false);
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve(true);
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}
