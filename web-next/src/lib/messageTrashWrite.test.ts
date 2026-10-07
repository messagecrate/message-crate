import assert from "node:assert/strict";
import { describe, it } from "node:test";
import Database from "better-sqlite3";

import { setMessageTrashInDb } from "./messageTrashWrite";
import { ensureVaultSchema } from "./vaultSchema";

const ACCOUNT_A = "101";

function testDb(): Database.Database {
  const db = new Database(":memory:");
  ensureVaultSchema(db);
  db.exec(`
    INSERT INTO accounts (id, username) VALUES (101, 'a'), (102, 'b');
    INSERT INTO handles (id, account_id, raw, normalized, handle_type, service)
      VALUES
        (1, 101, '+15555550123', '+15555550123', 'phone', 'phone'),
        (2, 101, 'chat-group', 'chat-group', 'other', 'phone'),
        (3, 102, 'chat-other', 'chat-other', 'other', 'phone');
    INSERT INTO contacts (id, account_id, preferred_name) VALUES (10, 101, 'Pat');
    INSERT INTO contact_handles (account_id, handle_id, contact_id)
      VALUES (101, 1, 10);
    INSERT INTO conversations (id, account_id, chat_handle_id, conversation_type, source_file)
      VALUES
        (20, 101, 2, 'group', 't.json'),
        (21, 101, 1, 'individual', 't.json'),
        (30, 102, 3, 'group', 't.json');
  `);
  return db;
}

function trashedConversationIds(db: Database.Database): number[] {
  return db
    .prepare(
      `SELECT conversation_id FROM trashed_conversations ORDER BY conversation_id`,
    )
    .pluck()
    .all() as number[];
}

describe("mixed message trash writes", () => {
  it("trashes and restores direct handles and group conversations together", () => {
    const db = testDb();
    try {
      const targets = {
        handles: [" +15555550123 ", "+15555550123"],
        conversationIds: [20, 20],
      };
      const trashed = setMessageTrashInDb(db, targets, true, ACCOUNT_A);
      assert.deepEqual(trashed, {
        handles: ["+15555550123"],
        conversationIds: [20],
        count: 2,
      });
      assert.equal(
        (
          db.prepare(`SELECT COUNT(*) AS n FROM contacts WHERE id = 10`).get() as {
            n: number;
          }
        ).n,
        1,
      );
      // The handle's 1:1 conversation (21) and the group (20) are in the trash.
      assert.deepEqual(trashedConversationIds(db), [20, 21]);

      const restored = setMessageTrashInDb(db, targets, false, ACCOUNT_A);
      assert.equal(restored.count, 2);
      assert.deepEqual(trashedConversationIds(db), []);
    } finally {
      db.close();
    }
  });

  it("rolls back the whole batch when a group target is invalid", () => {
    const db = testDb();
    try {
      assert.throws(
        () =>
          setMessageTrashInDb(
            db,
            {
              handles: ["+15555550123"],
              conversationIds: [21, 30],
            },
            true,
            ACCOUNT_A,
          ),
        /group conversation 21 not found/,
      );
      assert.deepEqual(trashedConversationIds(db), []);
    } finally {
      db.close();
    }
  });
});
