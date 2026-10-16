/**
 * Labels are the vault's Contact Groups (`/v1/contact-groups`). web-next's
 * screens still say "label"; the names come from the same table.
 */
import { labelSlug } from "@/lib/labelSlug";
import { RESERVED_LABEL_NAMES } from "@/lib/reservedLabels";

import { memo, vaultAll, type Schemas } from "./client";

type NamedSet = Schemas["NamedSet"];

const LABELS_TTL_MS = 5_000;

async function contactGroups(): Promise<NamedSet[]> {
  return memo("contact-groups", LABELS_TTL_MS, () =>
    vaultAll<NamedSet>("/v1/contact-groups"),
  );
}

/** Contact Group names, A–Z, minus the reserved section names. */
export async function listLabels(): Promise<string[]> {
  const groups = await contactGroups();
  return groups
    .map((g) => g.name)
    .filter((name) => !RESERVED_LABEL_NAMES.has(name.trim().toLowerCase()))
    .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
}

async function groupIdByName(name: string): Promise<number | null> {
  const folded = name.trim().toLowerCase();
  if (!folded) return null;
  const groups = await contactGroups();
  return groups.find((g) => g.name.trim().toLowerCase() === folded)?.id ?? null;
}

/** Contact ids in a named Contact Group (case-insensitive name). */
export async function listLabelMemberContactIds(name: string): Promise<number[]> {
  const id = await groupIdByName(name);
  if (id == null) return [];
  return vaultAll<number>(`/v1/contact-groups/${id}/members`);
}

export async function labelFromSlug(slug: string): Promise<string | null> {
  const trimmed = slug.trim();
  if (!trimmed) return null;
  const labels = await listLabels();
  for (const name of labels) {
    if (labelSlug(name) === trimmed) return name;
  }
  const folded = trimmed.toLowerCase();
  for (const name of labels) {
    if (labelSlug(name).toLowerCase() === folded) return name;
  }
  return null;
}
