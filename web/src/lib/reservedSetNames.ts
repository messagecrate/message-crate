import type reservedNames from "./reservedNames.json";

/**
 * One kind of set in `reservedNames.json`: what the server calls it, the
 * names it refuses, and the names it refuses with a sentence of their own.
 * The server's test writes the file from its own lists, in lower case.
 */
type ReservedNames = (typeof reservedNames)[keyof typeof reservedNames];

/** The names a set of this kind must not be created under. */
export function reservedSet(entry: ReservedNames): Set<string> {
  return new Set(entry.reserved);
}

/** The sentence the server answers for a reserved `name`, word for word. */
export function reservedError(entry: ReservedNames, name: string): string {
  const key = name.trim().toLowerCase();
  const special = entry.special.find((s) => s.name === key);
  return special?.message ?? `"${name.trim()}" is a reserved ${entry.label}`;
}
