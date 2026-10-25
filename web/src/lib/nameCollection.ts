import type { InfiniteData } from "@tanstack/react-query";
import type reservedNames from "./reservedNames.json";
import type { OffsetPage, RouteQueryKey } from "./routeQuery";
import { narrow } from "./searchQuery";

/**
 * Contact Groups and Message Tags are the same feature over different nouns: a
 * named set the account owns, and a membership that puts rows in or out of it.
 * This builds one from a description of the nouns so the two do not drift
 * apart. `useNameCollection` reads one (`hooks/useNameCollection.ts`), and
 * `useNameCollectionActions` writes to it (`hooks/useNameCollectionActions.ts`).
 *
 * The server addresses a set by its id; screens, the sidebar, and the router
 * hold names. The lookup from one to the other lives in
 * `useNameCollectionActions` and nowhere else:
 * the id comes from the cached list, or from the server once when the cached
 * list does not hold the name, and a name the server does not know is an error
 * before any write is sent. See `docs/architecture/rules/http-api.md`, API-1.
 */

/** One set as the server answers it. */
export type NamedSet = { id: number; name: string };

/** Members to put in and take out of a set, in one request. Either side may be left off. */
export type MembersPatch = { add?: number[]; remove?: number[] };

/** What a membership write answers with. */
export type MembersChanged = { added: number; removed: number };

/** A name to put on or take off some rows, as a screen asks for it. */
export type SetMembersVars = { name: string; patch: MembersPatch };

/**
 * A cached shape whose rows carry this collection's names as chips.
 *
 * A membership write patches these before the server answers, so a ticked box
 * shows on a long list without a round trip. The collection describes them;
 * the screen does not, which is why no screen keeps an override map.
 */
export type ChipTarget = {
  /** Prefix of the entries to patch. */
  key: RouteQueryKey;
  /** Field the names sit in on a row. */
  field: "groups" | "tags";
  /** `pages` for an offset-paged list entry, `row` for one row on its own. */
  shape: "pages" | "row";
};

/** The server calls one of these collections is built from. */
export type NameCollectionRoutes = {
  list: (opts?: { signal?: AbortSignal }) => Promise<NamedSet[]>;
  create: (body: { name: string }) => Promise<NamedSet>;
  update: (id: number, body: { name: string }) => Promise<NamedSet>;
  remove: (id: number) => Promise<void>;
  updateMembers: (
    id: number,
    body: { add: number[]; remove: number[] },
  ) => Promise<{ added: number; removed: number }>;
};

type NameCollectionConfig = {
  routes: NameCollectionRoutes;
  /** This collection's cache prefix, from `queryKeys`. */
  key: RouteQueryKey;
  /** Cached shapes to patch with this collection's names before the server answers. */
  chips: readonly ChipTarget[];
  /** This collection's search term for one name, e.g. `forGroup` from `searchQuery.ts`. */
  forName: (name: string) => string;
  /**
   * This collection's entry in `reservedNames.json`: what the server calls
   * it, the names it refuses, and the names it refuses with a sentence of
   * their own. The server's test writes the file from its own lists, in
   * lower case, and fails when the committed file differs.
   */
  reserved: ReservedNames;
};

type ReservedNames = (typeof reservedNames)[keyof typeof reservedNames];

export type NameCollection = {
  /** Cache key parts, before the account is put in front of them. */
  key: RouteQueryKey;
  routes: NameCollectionRoutes;
  chips: readonly ChipTarget[];
  /**
   * What one of these is called on the screen, "Contact Group" or "Message
   * Tag": the server's own name for it, from `reservedNames.json`.
   */
  label: string;
  isReserved: (name: string) => boolean;
  reservedError: (name: string) => string;
  /** Build the list query for one page of this collection plus a typed search. */
  listQuery: (name: string | "none" | null, search: string) => string;
};

/**
 * The key a name is looked up by among the reserved names: trimmed, with
 * the letters A to Z lower-cased and every other character as it is. The
 * server folds the same way, A to Z only (`to_ascii_lowercase` in
 * `db/named_membership.rs`), so a name such as `UNKNOWN` spelled with the
 * Kelvin sign (U+212A), which `toLowerCase` would turn into `unknown`, is
 * refused by neither side. Matching a name against the account's own sets
 * uses another fold: `setNameKey`.
 */
function reservedKey(name: string): string {
  return name.trim().replace(/[A-Z]/g, (letter) => letter.toLowerCase());
}

/**
 * The key two set names are matched by: each letter lowered on its own, with
 * `İ` (U+0130) lowered to a plain `i`. The server matches a set's name with
 * its own `lower()` (`lower_each_letter` in `db/sqlite_functions.rs`), which
 * lowers this way. `toLowerCase` reads a letter's neighbors, lowering a
 * word-final `Σ` to `ς`, and lowers `İ` to `i` and a combining dot, so it
 * would miss a set the server calls the same name.
 */
export function setNameKey(name: string): string {
  let key = "";
  for (const letter of name) key += letter === "\u0130" ? "i" : letter.toLowerCase();
  return key;
}

export function createNameCollection(config: NameCollectionConfig): NameCollection {
  const reservedSet = new Set(config.reserved.reserved);
  const isReserved = (name: string) => reservedSet.has(reservedKey(name));
  // The sentence the server answers for a reserved name, word for word.
  const reservedError = (name: string) => {
    const key = reservedKey(name);
    const special = config.reserved.special.find((s) => s.name === key);
    return special?.message ?? `"${name.trim()}" is a reserved ${config.reserved.label}`;
  };

  function listQuery(name: string | "none" | null, search: string): string {
    return narrow(name ? config.forName(name) : "", search);
  }

  return {
    key: config.key,
    routes: config.routes,
    chips: config.chips,
    label: config.reserved.label,
    isReserved,
    reservedError,
    listQuery,
  };
}

/** The cache holds every set of the collection as the server answered it, ids included. */
export function fetchSets(collection: NameCollection, signal: AbortSignal): Promise<NamedSet[]> {
  return collection.routes.list({ signal });
}

/** One row of a list that shows names as chips. */
type ChipRow = { id: string | number } & Record<string, unknown>;

/**
 * Add or remove one name, matching letter case the way the lists match it.
 *
 * `Family` and `family` are one name to a person, so ticking the box when a
 * row already has the name under another spelling changes nothing.
 */
export function withName(names: readonly string[], name: string, enable: boolean): string[] {
  const key = setNameKey(name);
  const has = names.some((n) => setNameKey(n) === key);
  if (enable) return has ? [...names] : [...names, name];
  return names.filter((n) => setNameKey(n) !== key);
}

/** Rewrite one cache entry so the rows named by `ids` gain or lose the name. */
export function patchChips(
  entry: unknown,
  target: ChipTarget,
  ids: ReadonlySet<string>,
  name: string,
  enable: boolean,
): unknown {
  if (!entry || ids.size === 0) return entry;
  const patchRow = (row: ChipRow): ChipRow => {
    if (!ids.has(String(row.id))) return row;
    const current = row[target.field];
    return {
      ...row,
      [target.field]: withName(Array.isArray(current) ? (current as string[]) : [], name, enable),
    };
  };
  if (target.shape === "row") return patchRow(entry as ChipRow);
  const paged = entry as InfiniteData<OffsetPage<ChipRow>>;
  if (!Array.isArray(paged.pages)) return entry;
  return {
    ...paged,
    pages: paged.pages.map((page) => ({ ...page, items: page.items.map(patchRow) })),
  };
}
