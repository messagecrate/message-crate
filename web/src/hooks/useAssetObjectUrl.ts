import { useCallback, useEffect, useRef, useState } from "react";
import type { AssetVersion } from "../lib/assetUrl";
import { fetchAssetObjectUrl } from "../lib/serverApi";

/** One version of one attachment to hold as a blob URL. */
export type AssetRequest = { sha256: string; version: AssetVersion };

/** Where one requested version stands: loading until it has a URL or an error. */
export type AssetObjectUrl = { url: string | null; error: string | null; loading: boolean };

type Held = { controller: AbortController; url: string | null };

const keyOf = (sha256: string, version: AssetVersion) => `${version}:${sha256.trim()}`;

/**
 * Hold the versions of attachments in `requests` as temporary blob URLs.
 *
 * A version is fetched when it first appears in `requests`, kept while it
 * stays there, and released (its download stopped, its URL revoked) when it
 * leaves or the component unmounts. The viewer passes the photo on screen
 * and its neighbours, so stepping to the next one finds it already loaded.
 *
 * Outside TanStack Query on purpose: the component showing the attachment owns
 * the URL and revokes it, which a cache entry cannot do. One of the two named
 * exceptions in `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */
export function useAssetObjectUrls(
  requests: readonly AssetRequest[],
): (sha256: string | null | undefined, version: AssetVersion) => AssetObjectUrl {
  const [entries, setEntries] = useState<
    ReadonlyMap<string, { url: string | null; error: string | null }>
  >(() => new Map());
  const held = useRef(new Map<string, Held>());
  // The requests by key. The map is replaced only when the set of keys
  // changes, so the effect below runs when something is wanted or released,
  // not on every render. Setting state while rendering is React's way to
  // keep a value derived from earlier renders.
  const next = new Map(
    requests
      .filter((r) => r.sha256.trim())
      .map((r) => [keyOf(r.sha256, r.version), { sha256: r.sha256.trim(), version: r.version }]),
  );
  const nextKeys = [...next.keys()].sort().join("|");
  const [latest, setLatest] = useState<{ keys: string; map: ReadonlyMap<string, AssetRequest> }>(
    () => ({ keys: nextKeys, map: next }),
  );
  if (latest.keys !== nextKeys) setLatest({ keys: nextKeys, map: next });
  const wanted = latest.keys === nextKeys ? latest.map : next;

  useEffect(() => {
    const dropped: string[] = [];
    for (const [key, entry] of held.current) {
      if (wanted.has(key)) continue;
      entry.controller.abort();
      if (entry.url) URL.revokeObjectURL(entry.url);
      held.current.delete(key);
      dropped.push(key);
    }
    if (dropped.length > 0) {
      setEntries((prev) => {
        const next = new Map(prev);
        for (const key of dropped) next.delete(key);
        return next;
      });
    }
    for (const [key, { sha256, version }] of wanted) {
      if (held.current.has(key)) continue;
      const entry: Held = { controller: new AbortController(), url: null };
      held.current.set(key, entry);
      fetchAssetObjectUrl(sha256, { version, signal: entry.controller.signal })
        .then((url) => {
          if (held.current.get(key) !== entry) {
            URL.revokeObjectURL(url);
            return;
          }
          entry.url = url;
          setEntries((prev) => new Map(prev).set(key, { url, error: null }));
        })
        .catch((e: unknown) => {
          if (held.current.get(key) !== entry) return;
          if (e instanceof DOMException && e.name === "AbortError") return;
          setEntries((prev) =>
            new Map(prev).set(key, {
              url: null,
              error: e instanceof Error ? e.message : String(e),
            }),
          );
        });
    }
  }, [wanted]);

  useEffect(() => {
    const all = held.current;
    return () => {
      for (const entry of all.values()) {
        entry.controller.abort();
        if (entry.url) URL.revokeObjectURL(entry.url);
      }
      all.clear();
    };
  }, []);

  return useCallback(
    (sha256, version) => {
      if (!sha256?.trim()) return { url: null, error: null, loading: false };
      const key = keyOf(sha256, version);
      const entry = entries.get(key);
      if (entry) return { ...entry, loading: false };
      return { url: null, error: null, loading: wanted.has(key) };
    },
    [entries, wanted],
  );
}

/**
 * Hold one version of one attachment as a temporary blob URL, or nothing
 * while `sha256` or `version` is null. See `useAssetObjectUrls`.
 */
export function useAssetObjectUrl(
  sha256: string | null | undefined,
  version: AssetVersion | null,
): AssetObjectUrl {
  const lookup = useAssetObjectUrls(sha256 && version ? [{ sha256, version }] : []);
  return version ? lookup(sha256, version) : { url: null, error: null, loading: false };
}
