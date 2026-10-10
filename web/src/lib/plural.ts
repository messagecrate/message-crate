/**
 * A count with its noun: "1 message", "1,234 messages", "2 identities".
 * The number is written with `toLocaleString()`; `many` is the plural, `noun` + "s" unless given.
 */
export function countOf(n: number, noun: string, many = `${noun}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? noun : many}`;
}
