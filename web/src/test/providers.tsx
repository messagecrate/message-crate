/**
 * Render a component that fetches server data.
 *
 * Anything using `useRouteQuery` needs two things from the tree: a query client
 * to cache into, and a logged-in account to name the cache entry after. This
 * supplies the first. The second comes from `useAuth`, which a test fakes with
 * `authModule` below.
 *
 * The client is built once per mounted `Providers`, so no test can read a
 * cache another test filled, and retries are off so a rejected request fails
 * the test at once rather than after a delay.
 *
 * "Once per mount" rather than "once per render" is load-bearing.
 * `renderHook(...).rerender()` re-renders the wrapper, so a client built in
 * the component body was a different, empty cache on every re-render — which
 * left the tests that assert TanStack Query keeps two conversations in two
 * entries asserting it against a cache that was being thrown away underneath
 * them. `useState` holds one client for the life of the mount instead, so
 * those tests read the cache they mean to.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type RenderOptions, type RenderResult, render } from "@testing-library/react";
import { type ReactElement, type ReactNode, useState } from "react";
import type { useAuth } from "../hooks/useAuth";

/**
 * A query client with retries and background refetching off.
 *
 * Entries nothing on screen reads are dropped at once, so one test's leftovers
 * never reach the next. A test about an entry nothing reads for a while passes
 * `keepUnread`, which holds such entries the way the app's client does: one
 * written ahead of the screen that reads it (a seed from a mutation's answer),
 * or one the screen read, stopped reading, and reads again (a conversation
 * selected again after another).
 *
 * Every entry is stale at once, so a screen that reads one asks the server
 * again. A test that seeds an entry and means the screen to show it without
 * asking passes `neverStale`.
 */
export function testQueryClient({
  keepUnread = false,
  neverStale = false,
}: {
  keepUnread?: boolean;
  neverStale?: boolean;
} = {}): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        refetchOnWindowFocus: false,
        staleTime: neverStale ? Number.POSITIVE_INFINITY : 0,
        gcTime: keepUnread ? Number.POSITIVE_INFINITY : 0,
      },
      mutations: { retry: false },
    },
  });
}

/** Wrap children in a query client that lives as long as this mount. */
export function Providers({
  children,
  keepUnread = false,
}: {
  children: ReactNode;
  /** Hold entries nothing on screen reads; see `testQueryClient`. */
  keepUnread?: boolean;
}) {
  const [client] = useState(() => testQueryClient({ keepUnread }));
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

/** `render`, with the query client a server query needs. */
export function renderWithProviders(ui: ReactElement, options?: RenderOptions): RenderResult {
  return render(ui, { wrapper: Providers, ...options });
}

/** What `useAuth` returns, the whole of it. */
type AuthValue = ReturnType<typeof useAuth>;

/**
 * What the mocked `useAuth` returns: a logged-in account, so a server query
 * has an account to name its cache entry after, and every other field the
 * real context carries, as a function that does nothing.
 *
 * Any id works; what matters is that it is stable within a test, since a
 * changing account id is a different cache entry by design.
 */
export const mockedAuth = {
  serverUrl: "http://127.0.0.1:8080",
  accountId: 7,
  token: "test-token",
  isAuthenticated: true,
  login: async () => {},
  updateToken: () => {},
  logout: async () => {},
  setServer: () => {},
  retrySavedLogin: () => {},
} satisfies AuthValue;

/**
 * The module a test's `vi.mock("<path>/hooks/useAuth", () => authModule())`
 * puts in place of the real one: `useAuth` returns `mockedAuth` with
 * `overrides` laid over it.
 *
 * `overrides` is typed against the real context, so a field the app starts
 * reading is there in every test, and a misspelled one fails to compile.
 * A test that changes the login between renders passes a function, which is
 * read on every call; an object is read once.
 *
 * The `vi.mock` call stays in each test file, because Vitest hoists only the
 * `vi.mock` calls written in the test file itself. Vitest runs the factory
 * when a module first imports `useAuth`; when that module is imported
 * before this one, `authModule` does not exist yet, and the factory imports
 * it itself: `vi.mock(path, async () => (await import("<path>/test/providers")).authModule())`.
 */
export function authModule(overrides: Partial<AuthValue> | (() => Partial<AuthValue>) = {}): {
  useAuth: () => AuthValue;
} {
  if (typeof overrides === "function") {
    return { useAuth: () => ({ ...mockedAuth, ...overrides() }) };
  }
  const value = { ...mockedAuth, ...overrides };
  return { useAuth: () => value };
}
