import { useMemo } from "react";
import { fetchSets, type NameCollection } from "../lib/nameCollection";
import { useRouteQuery } from "./useRouteQuery";

/** Live list of one collection's names for the logged-in account. */
export function useNameCollection(collection: NameCollection): {
  names: string[];
  loading: boolean;
} {
  const { data, isPending } = useRouteQuery(collection.key, (signal) =>
    fetchSets(collection, signal),
  );
  const names = useMemo(() => (data ?? []).map((set) => set.name), [data]);
  return { names, loading: isPending };
}
