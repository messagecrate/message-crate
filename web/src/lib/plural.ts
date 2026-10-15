/**
 * A count with its noun: "1 message", "1,234 messages", "2 identities".
 * The number is written with `toLocaleString()`; `many` is the plural, `noun` + "s" unless given.
 */
export function countOf(n: number, noun: string, many = `${noun}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? noun : many}`;
}

/** "a", "a and b", "a, b and c"; null for an empty list. */
export function joinWithAnd(items: readonly string[]): string | null {
  const last = items.at(-1);
  if (last === undefined) return null;
  const rest = items.slice(0, -1);
  return rest.length === 0 ? last : `${rest.join(", ")} and ${last}`;
}
