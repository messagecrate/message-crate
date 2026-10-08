import type { ContactHandleInput } from "@/lib/contactsWrite";
import type { IdentityType } from "@/lib/handleKind";

export const HANDLE_TYPES: readonly IdentityType[] = [
  "phone",
  "email",
  "username",
  "other",
];

export function isHandleType(value: unknown): value is IdentityType {
  return (
    typeof value === "string" &&
    (HANDLE_TYPES as readonly string[]).includes(value)
  );
}

/**
 * Parse a `handles: [{raw, handle_type}]` request field. Returns undefined when
 * the field is absent or contains no valid rows. Throws when `handle_type` is
 * present but not one of phone/email/username/other (callers map this to 400).
 */
export function parseHandlesBody(
  body: Record<string, unknown>,
): ContactHandleInput[] | undefined {
  if (!Array.isArray(body.handles)) return undefined;
  const out: ContactHandleInput[] = [];
  for (const item of body.handles) {
    if (!item || typeof item !== "object") continue;
    const row = item as Record<string, unknown>;
    if (typeof row.raw !== "string" || !row.raw.trim()) continue;
    const rawType = row.handle_type;
    if (rawType !== undefined && !isHandleType(rawType)) {
      throw new Error(
        "invalid handle_type (expected phone, email, username, or other)",
      );
    }
    out.push({
      raw: row.raw.trim(),
      handle_type: rawType as IdentityType | undefined,
    });
  }
  return out.length > 0 ? out : undefined;
}
