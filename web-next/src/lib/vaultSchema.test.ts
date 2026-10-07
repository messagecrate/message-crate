import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { describe, it } from "node:test";
import Database from "better-sqlite3";

import { ensureVaultSchema } from "./vaultSchema";

const ACCOUNT_ID = 101;

function columns(db: Database.Database, table: string): string[] {
  return (
    db.prepare(`PRAGMA table_info(${table})`).all() as Array<{ name: string }>
  ).map((column) => column.name);
}

function indexExists(db: Database.Database, name: string): boolean {
  const row = db
    .prepare(
      `SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'index' AND name = ?`,
    )
    .get(name) as { n: number };
  return row.n === 1;
}

describe("fresh vault schema", () => {
  it("creates the complete current schema idempotently", () => {
    const db = new Database(":memory:");
    ensureVaultSchema(db);
    const contract = JSON.parse(
      fs.readFileSync(
        path.join(process.cwd(), "..", "tests", "fixtures", "schema", "current-schema.json"),
        "utf8",
      ),
    ) as {
      tables: string[];
      indexes: string[];
      triggers: string[];
      metadata: string[];
    };

    const tables = new Set(
      (
        db
          .prepare(`SELECT name FROM sqlite_master WHERE type = 'table'`)
          .all() as Array<{ name: string }>
      ).map((row) => row.name),
    );
    for (const table of contract.tables) {
      assert.ok(tables.has(table), `missing table ${table}`);
    }

    assert.deepEqual(columns(db, "accounts"), [
      "id",
      "username",
      "password_hash",
      "preferred_name",
      "time_zone",
      "disabled",
      "must_set_up_profile",
      "can_import",
      "can_export",
      "can_delete",
      "last_login_at",
    ]);
    assert.deepEqual(columns(db, "contacts"), [
      "id",
      "account_id",
      "preferred_name",
      "origin",
      "created_at",
      "last_modified",
    ]);
    assert.deepEqual(columns(db, "handles"), [
      "id",
      "account_id",
      "raw",
      "normalized",
      "normalized_note",
      "handle_type",
      "service",
      "origin",
      "created_at",
      "last_modified",
    ]);
    for (const column of ["account_id", "source", "content_key", "duplicate_of"]) {
      assert.ok(columns(db, "messages").includes(column));
    }
    assert.ok(columns(db, "staging_messages").includes("account_id"));
    assert.ok(columns(db, "attachments").includes("size_bytes"));
    assert.ok(columns(db, "staging_attachments").includes("size_bytes"));

    for (const index of contract.indexes) {
      assert.ok(indexExists(db, index), `missing index ${index}`);
    }

    for (const trigger of contract.triggers) {
      const row = db
        .prepare(
          `SELECT COUNT(*) AS n FROM sqlite_master
           WHERE type = 'trigger' AND name = ?`,
        )
        .get(trigger) as { n: number };
      assert.equal(row.n, 1, `missing trigger ${trigger}`);
    }
    for (const key of contract.metadata) {
      const row = db
        .prepare(`SELECT COUNT(*) AS n FROM schema_meta WHERE key = ?`)
        .get(key) as { n: number };
      assert.equal(row.n, 1, `missing runtime metadata ${key}`);
    }

    ensureVaultSchema(db);
    db.close();
  });

  it("keeps fresh FTS triggers in sync", () => {
    const db = new Database(":memory:");
    ensureVaultSchema(db);
    db.prepare(`INSERT INTO accounts (id, username) VALUES (?, ?)`).run(
      ACCOUNT_ID,
      "alice",
    );
    db.prepare(
      `INSERT INTO handles (account_id, raw, normalized, handle_type, service)
       VALUES (?, '+15555550100', '+15555550100', 'phone', 'phone')`,
    ).run(ACCOUNT_ID);
    const handleId = Number(
      db
        .prepare(
          `SELECT id FROM handles
           WHERE account_id = ? AND normalized = '+15555550100' AND handle_type = 'phone'`,
        )
        .pluck()
        .get(ACCOUNT_ID),
    );
    db.prepare(
      `INSERT INTO conversations (
         account_id, chat_handle_id, conversation_type, source_file
       ) VALUES (?, ?, 'individual', 'test.json')`,
    ).run(ACCOUNT_ID, handleId);
    const conversationId = Number(
      db.prepare(`SELECT id FROM conversations`).pluck().get(),
    );
    const message = db
      .prepare(
        `INSERT INTO messages (
           conversation_id, account_id, source, guid, timestamp, time_precision,
           is_from_me, sort_order, body
         ) VALUES (?, ?, 'sms', 'g1', '2020-01-01T00:00:00Z', 'seconds', 0, 0, ?)`,
      )
      .run(conversationId, ACCOUNT_ID, "hello vault");
    db.prepare(
      `INSERT INTO attachments (message_id, original_name, transcription)
       VALUES (?, 'voice.m4a', 'secret phrase')`,
    ).run(message.lastInsertRowid);

    const hits = (term: string): number =>
      Number(
        db
          .prepare(`SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH ?`)
          .pluck()
          .get(term),
      );
    assert.equal(hits("vault"), 1);
    assert.equal(hits("secret"), 1);

    db.prepare(`UPDATE messages SET body = 'goodbye' WHERE id = ?`).run(
      message.lastInsertRowid,
    );
    assert.equal(hits("vault"), 0);
    assert.equal(hits("goodbye"), 1);

    db.prepare(`DELETE FROM attachments WHERE message_id = ?`).run(
      message.lastInsertRowid,
    );
    db.prepare(`DELETE FROM messages WHERE id = ?`).run(message.lastInsertRowid);
    assert.equal(hits("goodbye"), 0);
    db.close();
  });
});
