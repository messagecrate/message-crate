/**
 * The query client the web app fetches server data through, and the types
 * its route hooks share with modules that are not hooks. The hooks
 * themselves, `useRouteQuery` and its kin, are in `hooks/useRouteQuery.ts`.
 *
 * See `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */

import { MutationCache, QueryCache, QueryClient } from "@tanstack/react-query";
import { ApiError } from "./api";
import type { RouteQueryKey } from "./routeQueryKey";

/**
 * Whether a failure says the session token is no longer any good.
 *
 * The server answers `401 Unauthorized` both for a token it no longer accepts
 * (`authentication-required`) and for a mistyped current password
 * (`invalid-credentials`). Only the first ends the session: a wrong password
 * typed into Settings must not log the person out. The Import Run's own
 * server calls, made outside TanStack Query, ask it too
 * (`screens/import/run/serverCalls.ts`).
 */
export function endsSession(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401 && error.type !== "invalid-credentials";
}

/** Whether the server answered with a `4xx`: it refused the request itself. */
function refusedRequest(error: unknown): boolean {
  return error instanceof ApiError && error.status >= 400 && error.status < 500;
}

/** Whether the call's time limit ended it before any answer arrived. */
function timedOut(error: unknown): boolean {
  return error instanceof ApiError && error.timedOut;
}

/**
 * How long to wait before trying a failed read again.
 *
 * A database too busy to take the request (`database-busy`) says how many
 * seconds to wait. The database was busy because another request, such as an
 * import batch, held the write lock, or because every database connection
 * stayed in use. Either way a second try at once meets the same busy
 * database. Anything else waits `tanstackDefaultRetryDelay`.
 */
function retryDelay(failureCount: number, error: unknown): number {
  if (error instanceof ApiError && error.type === "database-busy" && error.retryAfter !== null) {
    return error.retryAfter * 1000;
  }
  return tanstackDefaultRetryDelay(failureCount);
}

/**
 * TanStack Query's own default wait before a retry. It is copied from the
 * library's `retryer.ts`, which does not export it. The wait starts at one
 * second and doubles with each failure, up to 30 seconds. A `retryDelay`
 * replaces the default for every failure, so the failures that are not
 * `database-busy` keep the default through this.
 */
function tanstackDefaultRetryDelay(failureCount: number): number {
  return Math.min(1000 * 2 ** failureCount, 30_000);
}

/**
 * Build the query client.
 *
 * Exported as a factory rather than a singleton so each test gets a client of
 * its own and cannot inherit another test's cache.
 *
 * `onUnauthorized` runs when any query or mutation fails because the server no
 * longer accepts the session token. Without it every screen showed its own
 * error and the person was never sent back to the login screen.
 */
export function createQueryClient({
  onUnauthorized = () => {},
}: {
  onUnauthorized?: () => void;
} = {}): QueryClient {
  const onError = (error: unknown) => {
    if (endsSession(error)) onUnauthorized();
  };
  return new QueryClient({
    queryCache: new QueryCache({ onError }),
    mutationCache: new MutationCache({ onError }),
    defaultOptions: {
      queries: {
        // The server is usually on the same host or a local network, so a
        // refetch is cheap. Half a minute is long enough that moving between
        // screens does not refetch, and short enough that a stale list
        // corrects itself without anyone reloading.
        staleTime: 30_000,
        // One retry, and only for a failure a second try can change: a
        // request that never reached the server, or a `5xx`. A `4xx` answers
        // the same request the same way again, so asking again only delays
        // the error on screen (or the login screen, for an ended session).
        // A call its time limit ended is not tried again either: the server
        // already had the whole limit to answer, and a second try would
        // double the wait before the error shows.
        retry: (failureCount, error) =>
          failureCount < 1 && !refusedRequest(error) && !timedOut(error),
        retryDelay,
        refetchOnWindowFocus: true,
      },
      // A write is never tried again: its failure is shown, and the person
      // can send it again once they have read why. This is TanStack Query's
      // own default, written here so the rule is on the page.
      mutations: { retry: false },
    },
  });
}

/** One page of an offset-paged list, with the total the server reported. */
export type OffsetPage<T> = {
  items: T[];
  total: number;
};

/**
 * How a screen loads one page. A route that answers more than the rows and
 * the total, as the Messages list answers its `search`, names the rest as `E`.
 */
export type PagedFetchPage<T, E extends object = object> = (args: {
  limit: number;
  offset: number;
  signal: AbortSignal;
}) => Promise<OffsetPage<T> & E>;

/** Entries as `snapshot` took them. The key is complete, account included. */
export type RouteCacheEntries = readonly [readonly unknown[], unknown][];

export type { RouteQueryKey };
