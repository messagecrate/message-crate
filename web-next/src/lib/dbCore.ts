import fs from "fs";

import Database from "better-sqlite3";
import { inferHandleType, normalizeHandle, type IdentityType } from "./handleKind";
import { formatPhoneDisplay } from "./phoneE164";
import { ensureDbParentDir } from "./paths";

const g = globalThis as unknown as {
  __mvReadonlyDb?: Database.Database | null;
  __mvReadonlyDbIdentity?: { dev: number; ino: number } | null;
  __mvHasDuplicateOf?: boolean | null;
};

function dbFileIdentity(file: string): { dev: number; ino: number } | null {
  try {
    const st = fs.statSync(file);
    return { dev: st.dev, ino: st.ino };
  } catch {
    return null;
  }
}

export function getDb(): Database.Database {
  const file = ensureDbParentDir();
  const identity = dbFileIdentity(file);
  // reset-demo (and similar) unlink+recreate vault.db; a cached better-sqlite3
  // handle keeps the deleted inode open and serves stale rows until we reopen.
  if (
    g.__mvReadonlyDb &&
    (!identity ||
      !g.__mvReadonlyDbIdentity ||
      identity.dev !== g.__mvReadonlyDbIdentity.dev ||
      identity.ino !== g.__mvReadonlyDbIdentity.ino)
  ) {
    resetDb();
  }

  if (!g.__mvReadonlyDb) {
    if (!identity) {
      throw new Error(
        `Vault database not found at ${file}. Start the vault with docker compose up, or create an account at /login.`,
      );
    }
    g.__mvReadonlyDb = new Database(file, {
      readonly: true,
      fileMustExist: true,
      timeout: 15000,
    });
    // Prefer WAL when the writer (serve/import) has enabled it; ignore if readonly open can't set it.
    try {
      g.__mvReadonlyDb.pragma("journal_mode = WAL");
    } catch {
      /* readonly connection may not change journal mode */
    }
    g.__mvReadonlyDb.pragma("busy_timeout = 15000");
    g.__mvReadonlyDb.pragma("foreign_keys = ON");
    g.__mvReadonlyDbIdentity = identity;
    g.__mvHasDuplicateOf = null;
  }
  return g.__mvReadonlyDb;
}

/** Close the cached readonly connection so the next read sees recent writes. */
export function resetDb(): void {
  if (g.__mvReadonlyDb) {
    try {
      g.__mvReadonlyDb.close();
    } catch {
      /* already closed / deleted inode */
    }
    g.__mvReadonlyDb = null;
  }
  g.__mvReadonlyDbIdentity = null;
  g.__mvHasDuplicateOf = null;
  const profileCache = (globalThis as unknown as {
    __mvAccountProfileCache?: Map<string, unknown>;
  }).__mvAccountProfileCache;
  profileCache?.clear();
}

export function hasDuplicateOfColumn(): boolean {
  if (g.__mvHasDuplicateOf != null) return g.__mvHasDuplicateOf;
  const db = getDb();
  const row = db
    .prepare(
      `SELECT COUNT(*) AS n FROM pragma_table_info('messages') WHERE name = 'duplicate_of'`,
    )
    .get() as { n: number };
  g.__mvHasDuplicateOf = row.n > 0;
  return g.__mvHasDuplicateOf;
}

/** When no source filter is set (All combined), hide soft-deduped cross-source copies. */
export function combinedDedupeSql(source?: string | null, alias?: string): string {
  if (source || !hasDuplicateOfColumn()) return "";
  const col = alias ? `${alias}.duplicate_of` : "duplicate_of";
  return ` AND ${col} IS NULL`;
}

/** Join first + last into a preferred display name (null when both empty). */
export function joinPreferredName(
  firstName: string | null | undefined,
  lastName: string | null | undefined,
): string | null {
  const parts = [firstName, lastName]
    .map((p) => p?.trim())
    .filter(Boolean) as string[];
  return parts.length ? parts.join(" ") : null;
}

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
  return {
    first: first || last,
    last: last || first,
  };
}

export function displayName(row: {
  preferred_name?: string | null;
  preferred_handle?: string | null;
  preferred_handle_type?: IdentityType | null;
}): string {
  const preferred = row.preferred_name?.trim();
  if (preferred) return preferred;
  const handle = row.preferred_handle?.trim();
  if (handle) {
    // Phones get international display formatting; emails/usernames pass through.
    if (
      !row.preferred_handle_type ||
      row.preferred_handle_type === "phone"
    ) {
      return formatPhoneDisplay(handle);
    }
    return handle;
  }
  return "Unknown";
}

export function sortFields(row: {
  preferred_name?: string | null;
  preferred_handle?: string | null;
}): { sortFirst: string; sortLast: string; letter: string } {
  const preferred = (row.preferred_name || "").trim();
  const handle = row.preferred_handle?.trim() || "";
  if (preferred) {
    const { first, last } = splitNameParts(preferred);
    const sortFirst = first || preferred;
    const sortLast = last || preferred;
    const ch = sortLast.charAt(0).toUpperCase();
    const letter = ch >= "A" && ch <= "Z" ? ch : "#";
    return { sortFirst, sortLast, letter };
  }
  const fallback = handle || "Unknown";
  const ch = fallback.charAt(0).toUpperCase();
  const letter = ch >= "A" && ch <= "Z" ? ch : "#";
  return { sortFirst: fallback, sortLast: fallback, letter };
}

export function hasTrashedConversationsTable(db: Database.Database): boolean {
  const row = db
    .prepare(
      `SELECT COUNT(*) AS n FROM sqlite_master
       WHERE type = 'table' AND name = 'trashed_conversations'`,
    )
    .get() as { n: number };
  return row.n > 0;
}

export function hasTrashedContactsTable(db: Database.Database): boolean {
  const row = db
    .prepare(
      `SELECT COUNT(*) AS n FROM sqlite_master
       WHERE type = 'table' AND name = 'trashed_contacts'`,
    )
    .get() as { n: number };
  return row.n > 0;
}

/** One handle row as exposed by the shared contact_handles join. */
export type ContactHandleRow = {
  handle_id: number;
  raw: string;
  handle_type: IdentityType;
  service: string | null;
  /** Review note when the handle's normalized form is ambiguous (not E.164). */
  normalized_note: string | null;
};

/** Every handle on the given contacts, phones first, keyed by contact id. */
export function contactHandlesByContact(
  db: Database.Database,
  accountId: string,
  contactIds: number[],
): Map<number, ContactHandleRow[]> {
  const out = new Map<number, ContactHandleRow[]>();
  const ids = [...new Set(contactIds.filter((id) => Number.isFinite(id)))];
  if (!ids.length) return out;
  const placeholders = ids.map(() => "?").join(",");
  const rows = db
    .prepare(
      `SELECT ch.contact_id AS contact_id,
              h.id AS handle_id,
              h.raw AS raw,
              h.handle_type AS handle_type,
              h.service AS service,
              h.normalized_note AS normalized_note
       FROM contact_handles ch
       JOIN handles h ON h.id = ch.handle_id
       WHERE ch.account_id = ? AND ch.contact_id IN (${placeholders})
       ORDER BY CASE h.handle_type WHEN 'phone' THEN 0 ELSE 1 END, h.raw`,
    )
    .all(accountId, ...ids) as Array<
    ContactHandleRow & { contact_id: number }
  >;
  for (const r of rows) {
    const list = out.get(r.contact_id) ?? [];
    list.push({
      handle_id: r.handle_id,
      raw: r.raw,
      handle_type: r.handle_type,
      service: r.service,
      normalized_note: r.normalized_note,
    });
    out.set(r.contact_id, list);
  }
  return out;
}

/** First handle (phones first), used wherever preferred_handle used to live. */
export function preferredHandleOf(handles: ContactHandleRow[]): string | null {
  const first = handles[0];
  return first ? first.raw : null;
}

/** Handle type of the preferred handle, if any. */
export function preferredHandleTypeOf(
  handles: ContactHandleRow[],
): IdentityType | null {
  const first = handles[0];
  return first ? first.handle_type : null;
}

/**
 * Resolve raw handles to handle ids for one platform service, dropping raws
 * with no handle row. Matching includes the service because the same phone can
 * have separate text-message and WhatsApp identities.
 */
export function handleIdsForRaws(
  db: Database.Database,
  accountId: string,
  raws: string[],
  service = "phone",
): number[] {
  const seen = new Map<string, { type: IdentityType; normalized: string }>();
  for (const raw of raws) {
    const trimmed = raw.trim();
    if (!trimmed) continue;
    const type = inferHandleType(trimmed);
    const normalized = normalizeHandle(trimmed, type);
    seen.set(`${type}\0${normalized}`, { type, normalized });
  }
  const needles = [...seen.values()];
  if (!needles.length) return [];
  const where = needles
    .map(() => `(h.normalized = ? AND h.handle_type = ? AND h.service = ?)`)
    .join(" OR ");
  const params: unknown[] = [accountId];
  for (const n of needles) params.push(n.normalized, n.type, service);
  const rows = db
    .prepare(
      `SELECT h.id AS id
       FROM handles h
       WHERE h.account_id = ? AND (${where})`,
    )
    .all(...params) as Array<{ id: number }>;
  return rows.map((r) => r.id);
}

/**
 * NOT EXISTS filter: the handle (by id expression) does not have its 1:1
 * conversation in the trash. A handle has no trash of its own.
 * `handleIdExpr` and `accountExpr` are SQL expressions from the outer query.
 */
export function notTrashedHandleSql(
  handleIdExpr: string,
  accountExpr: string,
): string {
  return `AND NOT EXISTS (
    SELECT 1 FROM trashed_conversations th
    JOIN conversations thc ON thc.id = th.conversation_id
    WHERE thc.conversation_type = 'individual'
      AND thc.chat_handle_id = ${handleIdExpr}
      AND th.account_id = ${accountExpr}
  )`;
}

function looksLikePhone(value: string): boolean {
  const t = value.trim();
  if (!t) return false;
  if (t.startsWith("+") && /^[+\d\s().-]+$/.test(t)) return true;
  const digits = t.replace(/\D/g, "");
  return digits.length >= 7 && digits.length === t.replace(/[\s().+-]/g, "").length;
}

export { looksLikePhone };

/** Prefer a real display hint; ignore phones and placeholder "(Unknown)" labels. */
export function usefulNameAlias(
  hint: string | null | undefined,
  handle: string | null | undefined,
): string | null {
  const t = hint?.trim() || null;
  if (!t) return null;
  if (looksLikePhone(t)) return null;
  if (handle && t.toLowerCase() === handle.toLowerCase()) return null;
  if (/^\(?unknown\)?$/i.test(t)) return null;
  return t;
}
