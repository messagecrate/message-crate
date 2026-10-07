import Database from "better-sqlite3";
import { ensureDbParentDir } from "./paths";
import {
  ACCOUNTS_DDL,
  CONTACTS_DDL,
  FTS_TRIGGERS_CREATE_SQL,
  FTS_TRIGGERS_DROP_SQL,
  FTS_VIRTUAL_DDL,
  MESSAGES_DDL,
  STAGING_DDL,
} from "./vaultSchema.generated";

function tableExists(db: Database.Database, name: string): boolean {
  const row = db
    .prepare(
      `SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'table' AND name = ?`,
    )
    .get(name) as { n: number };
  return row.n > 0;
}

export function ensureVaultSchema(db: Database.Database): void {
  db.exec(`PRAGMA foreign_keys = ON;`);
  db.exec(ACCOUNTS_DDL);
  // CONTACTS_DDL carries the handles + contact_handles tables; conversations
  // and participants reference handles(id), so it must exist before
  // MESSAGES_DDL is applied.
  db.exec(CONTACTS_DDL);
  db.exec(MESSAGES_DDL);
  db.exec(STAGING_DDL);
  ensureMessagesFts(db);
}

/** Open a writable vault connection with the complete current schema ready. */
export function openWritableVaultDb(): Database.Database {
  const db = new Database(ensureDbParentDir(), { timeout: 15000 });
  try {
    db.pragma("journal_mode = WAL");
    db.pragma("busy_timeout = 15000");
    db.pragma("foreign_keys = ON");
    ensureVaultSchema(db);
    return db;
  } catch (error) {
    db.close();
    throw error;
  }
}

/** Marker that current FTS sync trigger definitions are installed. */
export const MESSAGES_FTS_TRIGGERS_META_KEY = "messages_fts_triggers_v1";

/** Contentless FTS5 index over message body/subject plus attachment text. */
function ensureMessagesFts(db: Database.Database): void {
  if (!tableExists(db, "messages")) return;

  db.exec(FTS_VIRTUAL_DDL);

  const triggersReady = db
    .prepare(`SELECT COUNT(*) AS n FROM schema_meta WHERE key = ?`)
    .get(MESSAGES_FTS_TRIGGERS_META_KEY) as { n: number };
  if (triggersReady.n === 0) {
    db.exec(FTS_TRIGGERS_DROP_SQL);
    db.exec(FTS_TRIGGERS_CREATE_SQL);
    db.prepare(
      `INSERT OR REPLACE INTO schema_meta (key, value) VALUES (?, '1')`,
    ).run(MESSAGES_FTS_TRIGGERS_META_KEY);
  }
}
