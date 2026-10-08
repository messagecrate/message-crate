"use client";

import type { IdentityType } from "@/lib/handleKind";

const LABELS: Record<IdentityType, string> = {
  phone: "Phone",
  email: "Email",
  username: "Username",
  other: "Other",
};

/** Compact chip showing a handle's identity type (Phone / Email / …). */
export function HandleTypeBadge({
  type,
  className = "",
}: {
  type: IdentityType | null | undefined;
  className?: string;
}) {
  if (!type) return null;
  return (
    <span
      className={`inline-flex shrink-0 items-center rounded bg-elevated px-1 py-px text-[10px] leading-4 font-medium text-muted ${className}`}
    >
      {LABELS[type]}
    </span>
  );
}
