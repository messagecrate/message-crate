import Database from "better-sqlite3";
import { currentAccountId } from "./accountScope";
import {
  serializeContactsCsv,
  type VaultContactCsvRow,
} from "./contactsCsv";
import { phoneHandlesOnly } from "./handleKind";
import { dbPath } from "./paths";

/**
 * Export the current account’s contacts as a vault-owned CSV projection
 * sourced from SQLite (not raw VCF).
 *
 * Includes: sanitized phones, names, and every vault label.
 * Omits: DB-only email handles and all unrelated VCF fields.
 */
export function exportContactsCsvFromDb(
  accountId = currentAccountId(),
): string {
  const db = new Database(dbPath(), { readonly: true });
  try {
    const contacts = db
      .prepare(
        `SELECT id, preferred_name
         FROM contacts
         WHERE account_id = ?
         ORDER BY
           CASE WHEN preferred_name IS NULL OR TRIM(preferred_name) = '' THEN 1 ELSE 0 END,
           preferred_name COLLATE NOCASE,
           id`,
      )
      .all(accountId) as Array<{
      id: number;
      preferred_name: string | null;
    }>;

    const handleStmt = db.prepare(
      `SELECT h.raw AS handle
       FROM contact_handles cp
       JOIN handles h ON h.id = cp.handle_id
       WHERE cp.contact_id = ? AND cp.account_id = ?
       ORDER BY h.raw`,
    );
    const labelStmt = db.prepare(
      `SELECT cl.name FROM contact_group_members clm
       JOIN contact_groups cl ON cl.id = clm.group_id
       WHERE clm.contact_id = ? AND cl.account_id = ?
       ORDER BY cl.name COLLATE NOCASE`,
    );

    const rows: VaultContactCsvRow[] = [];
    for (const c of contacts) {
      const handles = handleStmt.all(c.id, accountId) as Array<{
        handle: string;
      }>;
      const phones = phoneHandlesOnly(handles.map((h) => h.handle));
      if (phones.length === 0) continue;

      const labels = (
        labelStmt.all(c.id, accountId) as Array<{ name: string }>
      ).map((l) => l.name);

      rows.push({
        phones,
        preferredName: c.preferred_name?.trim() || null,
        labels,
      });
    }

    return serializeContactsCsv(rows);
  } finally {
    db.close();
  }
}
