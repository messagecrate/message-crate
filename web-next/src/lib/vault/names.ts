/**
 * Name and sort helpers for rows built from `/v1` responses. These mirror the
 * helpers in `dbCore.ts` without importing that module, which loads
 * better-sqlite3 on import.
 */
import { inferHandleType, type IdentityType } from "@/lib/handleKind";
import { formatPhoneDisplay } from "@/lib/phoneE164";

/** The vault's placeholder name for a contact nobody has named. */
export const UNKNOWN_NAME = "Unknown";

/** Split display name on the first space: first half / remainder as last. */
export function splitNameParts(name: string | null | undefined): {
  first: string;
  last: string;
} {
  const trimmed = (name ?? "").trim();
  if (!trimmed) return { first: "", last: "" };
  const i = trimmed.indexOf(" ");
  if (i < 0) return { first: trimmed, last: trimmed };
  const first = trimmed.slice(0, i).trim();
  const last = trimmed.slice(i + 1).trim();
  return { first: first || last, last: last || first };
}

/** First/last name as the list rows expose them; null when nameless. */
export function nameParts(preferredName: string | null): {
  firstName: string | null;
  lastName: string | null;
} {
  const trimmed = (preferredName ?? "").trim();
  if (!trimmed) return { firstName: null, lastName: null };
  const { first, last } = splitNameParts(trimmed);
  const hasSpace = trimmed.includes(" ");
  return { firstName: first || null, lastName: hasSpace ? last || null : null };
}

/** The vault's `name` is the preferred name, or "Unknown" when there is none. */
export function preferredNameOf(name: string | null | undefined): string | null {
  const trimmed = (name ?? "").trim();
  if (!trimmed || trimmed === UNKNOWN_NAME) return null;
  return trimmed;
}

export function handleTypeOf(handle: string | null | undefined): IdentityType | null {
  const trimmed = (handle ?? "").trim();
  return trimmed ? inferHandleType(trimmed) : null;
}

export function displayName(
  preferredName: string | null,
  preferredHandle: string | null,
): string {
  if (preferredName) return preferredName;
  const handle = preferredHandle?.trim();
  if (handle) {
    const type = inferHandleType(handle);
    return type === "phone" ? formatPhoneDisplay(handle) : handle;
  }
  return UNKNOWN_NAME;
}

export function sortFields(
  preferredName: string | null,
  preferredHandle: string | null,
): { sortFirst: string; sortLast: string; letter: string } {
  const preferred = (preferredName || "").trim();
  const handle = preferredHandle?.trim() || "";
  if (preferred) {
    const { first, last } = splitNameParts(preferred);
    const sortFirst = first || preferred;
    const sortLast = last || preferred;
    const ch = sortLast.charAt(0).toUpperCase();
    const letter = ch >= "A" && ch <= "Z" ? ch : "#";
    return { sortFirst, sortLast, letter };
  }
  const fallback = handle || UNKNOWN_NAME;
  const ch = fallback.charAt(0).toUpperCase();
  const letter = ch >= "A" && ch <= "Z" ? ch : "#";
  return { sortFirst: fallback, sortLast: fallback, letter };
}

function looksLikePhone(value: string): boolean {
  const t = value.trim();
  if (!t) return false;
  if (t.startsWith("+") && /^[+\d\s().-]+$/.test(t)) return true;
  const digits = t.replace(/\D/g, "");
  return (
    digits.length >= 7 && digits.length === t.replace(/[\s().+-]/g, "").length
  );
}

/** True for titles that are only a chat id or a list of numbers. */
export function isGenericGroupTitle(title: string | null | undefined): boolean {
  if (!title) return true;
  const t = title.trim();
  if (!t) return true;
  if (/^chat\d+/i.test(t)) return true;
  let rest = t.replace(/^group:\s*/i, "").trim();
  rest = rest.replace(/,?\s*and\s+\d+\s+others?\.?$/i, "").trim();
  if (!rest) return true;
  const parts = rest
    .split(/[,;]/)
    .map((p) => p.trim())
    .filter(Boolean);
  return parts.length > 0 && parts.every(looksLikePhone);
}

const MAX_VISIBLE_NAMES = 8;

/** "Ann, Bob, and 3 others" plus the full list, from participant labels. */
export function formatPeopleTitle(names: string[]): {
  short: string;
  full: string;
  count: number;
} {
  const seen = new Set<string>();
  const unique: string[] = [];
  for (const name of names) {
    const key = name.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(name);
  }
  const full = unique.join(", ");
  if (unique.length <= MAX_VISIBLE_NAMES) {
    return { short: full, full, count: unique.length };
  }
  const shown = unique.slice(0, MAX_VISIBLE_NAMES).join(", ");
  const rest = unique.length - MAX_VISIBLE_NAMES;
  return {
    short: `${shown}, and ${rest} other${rest === 1 ? "" : "s"}`,
    full,
    count: unique.length,
  };
}
