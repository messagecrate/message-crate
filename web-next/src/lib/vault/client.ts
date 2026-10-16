/**
 * HTTP client for the vault's `/v1` API. Every read in web-next goes through
 * here. The session token comes from the `mv_session` cookie that
 * `POST /api/auth/login` sets after calling `POST /v1/session`.
 *
 * The vault host comes from `VAULT_API_URL` (default `http://127.0.0.1:8080`).
 */
import { cookies } from "next/headers";

import { SESSION_COOKIE } from "@/lib/accountCookie";

import type { components } from "./types.generated";

export type Schemas = components["schemas"];

/** One page of a `/v1` list. */
export type Page<T> = {
  items: T[];
  total: number;
  limit: number;
  offset: number;
};

/** Largest page the vault serves (`paging.rs`). */
export const MAX_PAGE = 500;

/** Thrown when the vault answers with a failure other than 401. */
export class VaultError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = "VaultError";
    this.status = status;
  }
}

/** Matches the check every route handler already makes on `err.message`. */
export const NOT_SIGNED_IN = "Not signed in";

export function vaultBaseUrl(): string {
  const raw = (process.env.VAULT_API_URL ?? "").trim();
  return (raw || "http://127.0.0.1:8080").replace(/\/+$/, "");
}

export async function sessionToken(): Promise<string | null> {
  const store = await cookies();
  return store.get(SESSION_COOKIE)?.value?.trim() || null;
}

export type VaultRequest = {
  method?: string;
  body?: unknown;
  /** Explicit token; `null` sends no Authorization header. */
  token?: string | null;
  headers?: Record<string, string>;
};

type QueryValue = string | number | boolean | null | undefined;

/** `?a=1&b=x` from an object, skipping empty values. */
export function qs(params: Record<string, QueryValue>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === "") continue;
    search.set(key, String(value));
  }
  const s = search.toString();
  return s ? `?${s}` : "";
}

export async function vaultFetch(
  path: string,
  req: VaultRequest = {},
): Promise<Response> {
  const token = req.token === undefined ? await sessionToken() : req.token;
  const headers: Record<string, string> = { ...(req.headers ?? {}) };
  if (token) headers.Authorization = `Bearer ${token}`;
  let body: string | undefined;
  if (req.body !== undefined) {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(req.body);
  }
  return fetch(`${vaultBaseUrl()}${path}`, {
    method: req.method ?? "GET",
    headers,
    body,
    cache: "no-store",
  });
}

/** The problem's `detail`, else its `title` (`application/problem+json`). */
export async function failureMessage(res: Response): Promise<string> {
  try {
    const json = (await res.json()) as { detail?: unknown; title?: unknown };
    if (json && typeof json.detail === "string" && json.detail) return json.detail;
    if (json && typeof json.title === "string" && json.title) return json.title;
  } catch {
    /* not JSON */
  }
  return `${res.status} ${res.statusText}`.trim();
}

/** JSON body of a `/v1` call. A 401 becomes the "Not signed in" error. */
export async function vaultJson<T>(
  path: string,
  req: VaultRequest = {},
): Promise<T> {
  const res = await vaultFetch(path, req);
  if (res.status === 401) throw new Error(NOT_SIGNED_IN);
  if (!res.ok) throw new VaultError(res.status, await failureMessage(res));
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

/** `null` for a 404 instead of an error. */
export async function vaultJsonOrNull<T>(
  path: string,
  req: VaultRequest = {},
): Promise<T | null> {
  try {
    return await vaultJson<T>(path, req);
  } catch (err) {
    if (err instanceof VaultError && err.status === 404) return null;
    throw err;
  }
}

/** One page of a list. */
export async function vaultPage<T>(
  path: string,
  params: Record<string, QueryValue> = {},
): Promise<Page<T>> {
  return vaultJson<Page<T>>(`${path}${qs(params)}`);
}

/**
 * Every row of a list, walking pages of {@link MAX_PAGE}. The vault caps
 * `offset` at 50 000 on the list routes, so a list longer than that is cut
 * there.
 */
export async function vaultAll<T>(
  path: string,
  params: Record<string, QueryValue> = {},
): Promise<T[]> {
  const out: T[] = [];
  let offset = 0;
  for (;;) {
    const page = await vaultPage<T>(path, { ...params, limit: MAX_PAGE, offset });
    out.push(...page.items);
    offset += page.items.length;
    if (page.items.length === 0 || offset >= page.total || offset > 50_000) {
      break;
    }
  }
  return out;
}

/** `total` of a list without loading its rows. */
export async function vaultCount(
  path: string,
  params: Record<string, QueryValue> = {},
): Promise<number> {
  const page = await vaultPage<unknown>(path, { ...params, limit: 1, offset: 0 });
  return page.total;
}

/** Run `fn` over `items` with at most `width` calls in flight. */
export async function mapPool<T, R>(
  items: readonly T[],
  width: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results: R[] = new Array(items.length);
  let next = 0;
  const workers = Array.from(
    { length: Math.min(width, items.length) },
    async () => {
      for (;;) {
        const i = next++;
        if (i >= items.length) return;
        results[i] = await fn(items[i]!, i);
      }
    },
  );
  await Promise.all(workers);
  return results;
}

const memoStore = new Map<string, { at: number; value: Promise<unknown> }>();

/**
 * Cache a read for a few seconds, keyed by session token, so one page load
 * that needs the same list from several lib functions fetches it once.
 */
export async function memo<T>(
  key: string,
  ttlMs: number,
  fn: () => Promise<T>,
): Promise<T> {
  const token = (await sessionToken()) ?? "";
  const fullKey = `${token}\0${key}`;
  const now = Date.now();
  const hit = memoStore.get(fullKey);
  if (hit && now - hit.at < ttlMs) return hit.value as Promise<T>;
  const value = fn();
  memoStore.set(fullKey, { at: now, value });
  value.catch(() => memoStore.delete(fullKey));
  if (memoStore.size > 200) {
    for (const [k, v] of memoStore) {
      if (now - v.at >= ttlMs) memoStore.delete(k);
    }
  }
  return value;
}

/** Drop every cached read (after login, or when a test needs a fresh view). */
export function clearMemo(): void {
  memoStore.clear();
}

/** `2024-03-01T…` or `2024-03-01 …` → `2024-03-01`; empty stays empty. */
export function dayOf(timestamp: string | null | undefined): string {
  return (timestamp ?? "").slice(0, 10);
}

export function yearOf(timestamp: string | null | undefined): number | null {
  const y = Number((timestamp ?? "").slice(0, 4));
  return Number.isFinite(y) && y > 0 ? y : null;
}
