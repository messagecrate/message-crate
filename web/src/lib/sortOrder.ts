/** Which way a sorted list runs: ascending or descending. */
export type SortOrder = "asc" | "desc";

/** Whether a value read back from storage is a `SortOrder`. */
export function isSortOrder(value: unknown): value is SortOrder {
  return value === "asc" || value === "desc";
}
