import type { UseMutationResult } from "@tanstack/react-query";
import { useCallback, useMemo, useRef } from "react";
import { ApiError } from "../lib/api";
import {
  fetchSets,
  type MembersChanged,
  type MembersPatch,
  type NameCollection,
  type NamedSet,
  patchChips,
  type SetMembersVars,
  setNameKey,
} from "../lib/nameCollection";
import type { RouteCacheEntries } from "../lib/routeQuery";
import { useRouteCache, useRouteMutation } from "./useRouteQuery";

/**
 * The id behind a name: from the cache, else from the server once, else an
 * error and no request. The server-once path covers creating a set and adding
 * to it before the list, marked stale by the create, has come back.
 */
function useIdOf(collection: NameCollection): (name: string) => Promise<number> {
  const cache = useRouteCache();
  return useCallback(
    async (name: string) => {
      const wanted = setNameKey(name.trim());
      const find = (sets: NamedSet[] | undefined) =>
        sets?.find((set) => setNameKey(set.name) === wanted)?.id;
      const hit = find(cache.read<NamedSet[]>(collection.key));
      if (hit !== undefined) return hit;
      const fresh = find(
        await cache.fetch<NamedSet[]>(collection.key, (signal) => fetchSets(collection, signal)),
      );
      if (fresh !== undefined) return fresh;
      throw new Error(`${collection.label} not found`);
    },
    [cache, collection],
  );
}

/** A name nobody may use, refused before any request is sent. */
function checkedName(collection: NameCollection, name: string): string {
  const trimmed = name.trim();
  if (!trimmed) throw new Error("name required");
  if (collection.isReserved(trimmed)) throw new Error(collection.reservedError(trimmed));
  return trimmed;
}

function useCreateNamedSet(collection: NameCollection): UseMutationResult<NamedSet, Error, string> {
  return useRouteMutation<NamedSet, Error, string>({
    mutationFn: async (name) => collection.routes.create({ name: checkedName(collection, name) }),
  });
}

function useRenameNamedSet(
  collection: NameCollection,
): UseMutationResult<NamedSet, Error, { from: string; to: string }> {
  const idOf = useIdOf(collection);
  return useRouteMutation<NamedSet, Error, { from: string; to: string }>({
    mutationFn: async ({ from, to }) => {
      const name = checkedName(collection, to);
      return collection.routes.update(await idOf(from), { name });
    },
  });
}

function useDeleteNamedSet(collection: NameCollection): UseMutationResult<void, Error, string> {
  const idOf = useIdOf(collection);
  return useRouteMutation<void, Error, string>({
    mutationFn: async (name) => collection.routes.remove(await idOf(name)),
  });
}

/** The rows as they were before an optimistic membership write touched them. */
type ChipSnapshot = { entries: RouteCacheEntries };

/**
 * Put rows in or out of one set, drawn before the server answers.
 *
 * The chips change on the list and on the open contact at once, and the
 * account's cache is marked stale once the write settles. Two of these can be
 * in flight together — the Clear all button fires one per name — but the
 * rollback is a whole-entry snapshot: if the earlier of two overlapping
 * writes fails, restoring its snapshot overwrites the later one's optimistic
 * chips too, until the account's cache, marked stale once each write settles,
 * refetches and the two converge on what the server actually has.
 */
export function useSetNamedSetMembers(
  collection: NameCollection,
): UseMutationResult<MembersChanged, Error, SetMembersVars, ChipSnapshot> {
  const cache = useRouteCache();
  const idOf = useIdOf(collection);
  return useRouteMutation<MembersChanged, Error, SetMembersVars, ChipSnapshot>({
    mutationFn: async ({ name, patch }) =>
      collection.routes.updateMembers(await idOf(name), {
        add: patch.add ?? [],
        remove: patch.remove ?? [],
      }),
    onMutate: async ({ name, patch }) => {
      const add = new Set((patch.add ?? []).map(String));
      const remove = new Set((patch.remove ?? []).map(String));
      for (const target of collection.chips) await cache.cancel(target.key);
      const entries = collection.chips.flatMap((target) => cache.snapshot(target.key));
      for (const target of collection.chips) {
        cache.patch<unknown>(target.key, (entry) =>
          patchChips(patchChips(entry, target, add, name, true), target, remove, name, false),
        );
      }
      return { entries };
    },
    onError: (_error, _vars, context) => {
      if (context) cache.restore(context.entries);
    },
  });
}

/** What a screen or the sidebar does to one of these collections. */
type NameCollectionActions = {
  create: (name: string) => Promise<string>;
  /**
   * The name of the set called `name` in any letter case, as the server
   * spells it, after creating it when the account has none by that name.
   */
  ensure: (name: string) => Promise<string>;
  rename: (from: string, to: string) => Promise<string>;
  remove: (name: string) => Promise<void>;
  setMembers: (name: string, patch: MembersPatch) => Promise<MembersChanged>;
  /** Any of the four in flight, so a screen needs no busy flag of its own. */
  pending: boolean;
  /** The newest of the four to fail, or null once a later one succeeds. */
  error: Error | null;
};

/**
 * The four writes, behind names.
 *
 * Screens keep passing names; the ids, the optimistic chips, the rollback and
 * the invalidation all belong to the mutations above.
 */
export function useNameCollectionActions(collection: NameCollection): NameCollectionActions {
  const cache = useRouteCache();
  const createSet = useCreateNamedSet(collection);
  const renameSet = useRenameNamedSet(collection);
  const deleteSet = useDeleteNamedSet(collection);
  const members = useSetNamedSetMembers(collection);

  const create = createSet.mutateAsync;
  const rename = renameSet.mutateAsync;
  const remove = deleteSet.mutateAsync;
  const setMembers = members.mutateAsync;
  const pending =
    createSet.isPending || renameSet.isPending || deleteSet.isPending || members.isPending;

  // Each mutate call resets that mutation's own error and stamps a fresh
  // `submittedAt`, so whichever of the four last started is also whichever
  // last settled; its error (or lack of one) is the collection's error. A
  // fixed create-then-rename-then-remove-then-setMembers order would instead
  // let an old create failure outlive every write that came after it.
  const latest = [createSet, renameSet, deleteSet, members].reduce((newest, next) =>
    next.submittedAt > newest.submittedAt ? next : newest,
  );
  const error = latest.error;

  // Creates `ensure` has sent and the server has not answered, by `setNameKey`
  // name, so a second `ensure` for the same name waits for the first rather
  // than reading a list the first create has not reached yet.
  const ensuring = useRef(new Map<string, Promise<NamedSet>>());

  // Memoised on the mutation objects' own stable `mutateAsync` identities,
  // the cache and the collection only: `pending` and `error` change on every
  // keystroke of a write, and a caller that lists this object's methods in a
  // `useEffect` dependency array (as `ContactList.tsx` does) must not see a
  // new function each time.
  const callbacks = useMemo(
    () => ({
      create: async (name: string) => (await create(name)).name,
      // Asks the server for the list rather than reading what a screen last
      // rendered, which can predate a create still settling: "family" right
      // after "Family" finds the set instead of sending a second create.
      ensure: async (name: string) => {
        const wanted = setNameKey(name.trim());
        const listed = async () => {
          const sets = await cache.fetch<NamedSet[]>(collection.key, (signal) =>
            fetchSets(collection, signal),
          );
          return sets.find((set) => setNameKey(set.name) === wanted)?.name;
        };
        const inFlight = ensuring.current.get(wanted);
        if (inFlight) {
          const settled = await inFlight.then(
            (set) => set.name,
            () => undefined,
          );
          if (settled !== undefined) return settled;
        }
        const found = await listed();
        if (found !== undefined) return found;
        const created = create(name);
        ensuring.current.set(wanted, created);
        try {
          return (await created).name;
        } catch (err) {
          // Another tab or window created the name between the list and
          // the create: the set exists, which is what was asked for.
          if (err instanceof ApiError && err.type === "name-taken") {
            const taken = await listed();
            if (taken !== undefined) return taken;
          }
          throw err;
        } finally {
          if (ensuring.current.get(wanted) === created) ensuring.current.delete(wanted);
        }
      },
      rename: async (from: string, to: string) => (await rename({ from, to })).name,
      remove: (name: string) => remove(name),
      setMembers: (name: string, patch: MembersPatch) => setMembers({ name, patch }),
    }),
    [cache, collection, create, rename, remove, setMembers],
  );

  return { ...callbacks, pending, error };
}
