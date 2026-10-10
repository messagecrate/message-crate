export type MembershipCheckState = "on" | "off" | "mixed";

/** on / off / mixed from how many of the selected rows already have each name. */
export function checksFromMembers(
  names: string[],
  memberLists: readonly string[][],
): Record<string, MembershipCheckState> {
  const checks: Record<string, MembershipCheckState> = {};
  for (const name of names) {
    const hits = memberLists.filter((list) =>
      list.some((item) => item.toLowerCase() === name.toLowerCase()),
    ).length;
    if (hits === 0) checks[name] = "off";
    else if (memberLists.length > 0 && hits === memberLists.length) {
      checks[name] = "on";
    } else checks[name] = "mixed";
  }
  return checks;
}

/**
 * Clear all: take every name the selected rows carry off them, one write per
 * name, all started at once rather than each waiting for the one before.
 */
export async function clearAllMembers(
  memberLists: readonly (readonly string[])[],
  removeName: (name: string) => Promise<unknown>,
): Promise<void> {
  const names = new Set(memberLists.flat());
  await Promise.allSettled([...names].map(removeName));
}
