"use client";

import type { ContactHandle } from "@/lib/types";
import { inferHandleType, type IdentityType } from "@/lib/handleKind";

/** One handle row in the contact edit draft (raw + explicit identity type). */
export type ContactEditDraftHandle = {
  raw: string;
  handle_type: IdentityType;
  /** Review note for an ambiguous normalized value; cleared when the raw is edited. */
  normalizedNote?: string | null;
};

export type ContactEditDraft = {
  preferredName: string;
  handles: ContactEditDraftHandle[];
  labels: string[];
};

/** Handles to persist: non-empty trimmed rows with their types. */
export type ContactEditDraftSaveHandle = {
  raw: string;
  handle_type: IdentityType;
};

export function seedContactEditDraft(contact: {
  preferredName?: string | null;
  firstName?: string | null;
  lastName?: string | null;
  handles?: ContactHandle[] | string[];
  phones?: string[];
  labels?: string[];
}): ContactEditDraft {
  const preferred =
    contact.preferredName?.trim() ||
    [contact.firstName, contact.lastName].filter(Boolean).join(" ").trim() ||
    "";
  const rawHandles = contact.handles ?? contact.phones ?? [];
  const handles: ContactEditDraftHandle[] = [];
  for (const h of rawHandles) {
    const raw = (typeof h === "string" ? h : h.raw).trim();
    if (!raw) continue;
    handles.push({
      raw,
      handle_type:
        typeof h === "string" ? inferHandleType(raw) : h.handle_type,
      normalizedNote:
        typeof h === "string" ? null : h.normalizedNote ?? null,
    });
  }
  return {
    preferredName: preferred,
    handles: [...handles, { raw: "", handle_type: "phone" }],
    labels: contact.labels ? [...contact.labels] : [],
  };
}

export function emptyContactEditDraft(defaults?: {
  labels?: string[];
}): ContactEditDraft {
  return {
    preferredName: "",
    handles: [{ raw: "", handle_type: "phone" }],
    labels: defaults?.labels ? [...defaults.labels] : [],
  };
}

export function draftHasName(draft: ContactEditDraft): boolean {
  return draft.preferredName.trim() !== "";
}

/** Drop empty non-trailing rows; ensure exactly one trailing empty row. */
export function normalizeHandleRows(
  handles: ContactEditDraftHandle[],
): ContactEditDraftHandle[] {
  const filled = handles.filter((h, i) => {
    if (i === handles.length - 1) return true;
    return h.raw.trim() !== "";
  });
  const withoutTrailingEmpties = [...filled];
  while (
    withoutTrailingEmpties.length > 1 &&
    withoutTrailingEmpties[withoutTrailingEmpties.length - 1]?.raw === "" &&
    withoutTrailingEmpties[withoutTrailingEmpties.length - 2]?.raw === ""
  ) {
    withoutTrailingEmpties.pop();
  }
  if (
    withoutTrailingEmpties.length === 0 ||
    withoutTrailingEmpties[withoutTrailingEmpties.length - 1]!.raw !== ""
  ) {
    withoutTrailingEmpties.push({ raw: "", handle_type: "phone" });
  }
  return withoutTrailingEmpties;
}

export function updateHandleAt(
  handles: ContactEditDraftHandle[],
  index: number,
  value: string,
): ContactEditDraftHandle[] {
  const next = [...handles];
  const row = next[index] ?? { raw: "", handle_type: "phone" };
  // Editing the raw re-normalizes the handle: the review note no longer
  // describes the current value and is dropped from the draft.
  next[index] = { ...row, raw: value, normalizedNote: null };
  if (index === handles.length - 1 && value !== "") {
    next.push({ raw: "", handle_type: "phone" });
  }
  return next;
}

export function setHandleTypeAt(
  handles: ContactEditDraftHandle[],
  index: number,
  handle_type: IdentityType,
): ContactEditDraftHandle[] {
  const next = [...handles];
  const row = next[index] ?? { raw: "", handle_type };
  // Changing the identity type re-normalizes the handle; drop the note.
  next[index] = { ...row, handle_type, normalizedNote: null };
  return next;
}

export function removeHandleAt(
  handles: ContactEditDraftHandle[],
  index: number,
): ContactEditDraftHandle[] {
  return normalizeHandleRows(handles.filter((_, i) => i !== index));
}

export function blurHandleAt(
  handles: ContactEditDraftHandle[],
  index: number,
): ContactEditDraftHandle[] {
  if (index >= handles.length - 1) return handles;
  if (handles[index]?.raw.trim() !== "") return handles;
  return normalizeHandleRows(handles);
}

/** Handles to persist: non-empty trimmed values with their types. */
export function handlesForSave(
  handles: ContactEditDraftHandle[],
): ContactEditDraftSaveHandle[] {
  return handles
    .map((h) => ({ raw: h.raw.trim(), handle_type: h.handle_type }))
    .filter((h) => h.raw !== "");
}

const HANDLE_TYPE_OPTIONS: Array<{ value: IdentityType; label: string }> = [
  { value: "phone", label: "Phone" },
  { value: "email", label: "Email" },
  { value: "username", label: "Username" },
  { value: "other", label: "Other" },
];

/**
 * Handle editor rows: one type dropdown + raw input per row, with a trailing
 * empty row that appends another on typing. The dropdown picks the identity
 * type, which drives normalization and matching in the handles table.
 */
export function ContactHandleList({
  handles,
  onChange,
  /** When set, refuse to remove/clear below this many non-empty handles. */
  minFilled = 0,
  placeholder = "Phone, email, or username",
  removeLabel = "Remove handle",
}: {
  handles: ContactEditDraftHandle[];
  onChange: (handles: ContactEditDraftHandle[]) => void;
  minFilled?: number;
  placeholder?: string;
  removeLabel?: string;
}) {
  const filledCount = handlesForSave(handles).length;
  return (
    <div className="flex flex-col gap-1.5">
      {handles.map((row, index) => {
        const showRemove =
          row.raw.trim() !== "" && filledCount > minFilled;
        return (
          <div key={index} className="flex items-center gap-2">
            <select
              value={row.handle_type}
              aria-label="Handle type"
              onChange={(e) =>
                onChange(
                  setHandleTypeAt(
                    handles,
                    index,
                    e.target.value as IdentityType,
                  ),
                )
              }
              className="h-[30px] shrink-0 rounded-md border border-border bg-elevated/40 px-1.5 text-[12px] text-text outline-none focus:border-accent/60"
            >
              {HANDLE_TYPE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <input
              type="text"
              value={row.raw}
              onChange={(e) => {
                const next = updateHandleAt(handles, index, e.target.value);
                if (
                  minFilled > 0 &&
                  handlesForSave(next).length < minFilled
                ) {
                  return;
                }
                onChange(next);
              }}
              onBlur={() => onChange(blurHandleAt(handles, index))}
              placeholder={placeholder}
              className="min-w-0 flex-1 rounded-md border border-border bg-elevated/40 px-2.5 py-1.5 text-[13px] text-text outline-none placeholder:text-muted focus:border-accent/60"
            />
            {row.normalizedNote ? (
              <span
                title={row.normalizedNote}
                className="shrink-0 rounded bg-danger/15 px-1.5 py-0.5 text-[11px] font-medium text-danger"
              >
                needs review
              </span>
            ) : null}
            {showRemove && (
              <button
                type="button"
                onClick={() => onChange(removeHandleAt(handles, index))}
                className="shrink-0 rounded px-1.5 text-[12px] text-muted hover:bg-hover hover:text-text"
                aria-label={removeLabel}
              >
                ×
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Phone-only helpers kept for the account identity form ("Me" phone numbers).
// ---------------------------------------------------------------------------

/** Drop empty non-trailing rows; ensure exactly one trailing empty row. */
export function normalizePhoneRows(phones: string[]): string[] {
  const filled = phones.filter((p, i) => {
    if (i === phones.length - 1) return true;
    return p.trim() !== "";
  });
  const withoutTrailingEmpties = [...filled];
  while (
    withoutTrailingEmpties.length > 1 &&
    withoutTrailingEmpties[withoutTrailingEmpties.length - 1] === "" &&
    withoutTrailingEmpties[withoutTrailingEmpties.length - 2] === ""
  ) {
    withoutTrailingEmpties.pop();
  }
  if (
    withoutTrailingEmpties.length === 0 ||
    withoutTrailingEmpties[withoutTrailingEmpties.length - 1] !== ""
  ) {
    withoutTrailingEmpties.push("");
  }
  return withoutTrailingEmpties;
}

export function updatePhoneAt(
  phones: string[],
  index: number,
  value: string,
): string[] {
  const next = [...phones];
  next[index] = value;
  if (index === phones.length - 1 && value !== "") {
    next.push("");
  }
  return next;
}

export function removePhoneAt(phones: string[], index: number): string[] {
  return normalizePhoneRows(phones.filter((_, i) => i !== index));
}

export function blurPhoneAt(phones: string[], index: number): string[] {
  if (index >= phones.length - 1) return phones;
  if (phones[index]?.trim() !== "") return phones;
  return normalizePhoneRows(phones);
}

/** Phones to persist: non-empty trimmed values, no trailing empty. */
export function phonesForSave(phones: string[]): string[] {
  return phones.map((p) => p.trim()).filter(Boolean);
}

/** Phone-only handle rows (account identity edit, settings). */
export function ContactPhoneList({
  phones,
  onChange,
  /** When set, refuse to remove/clear below this many non-empty phones. */
  minFilled = 0,
  placeholder = "Phone or email",
  removeLabel = "Remove phone or email",
}: {
  phones: string[];
  onChange: (phones: string[]) => void;
  minFilled?: number;
  placeholder?: string;
  removeLabel?: string;
}) {
  const filledCount = phonesForSave(phones).length;
  return (
    <div className="flex flex-col gap-1.5">
      {phones.map((phone, index) => {
        const showRemove =
          phone.trim() !== "" && filledCount > minFilled;
        return (
          <div key={index} className="flex items-center gap-2">
            <input
              type="text"
              value={phone}
              onChange={(e) => {
                const next = updatePhoneAt(phones, index, e.target.value);
                if (
                  minFilled > 0 &&
                  phonesForSave(next).length < minFilled
                ) {
                  return;
                }
                onChange(next);
              }}
              onBlur={() => onChange(blurPhoneAt(phones, index))}
              placeholder={placeholder}
              className="min-w-0 flex-1 rounded-md border border-border bg-elevated/40 px-2.5 py-1.5 text-[13px] text-text outline-none placeholder:text-muted focus:border-accent/60"
            />
            {showRemove && (
              <button
                type="button"
                onClick={() => onChange(removePhoneAt(phones, index))}
                className="shrink-0 rounded px-1.5 text-[12px] text-muted hover:bg-hover hover:text-text"
                aria-label={removeLabel}
              >
                ×
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
