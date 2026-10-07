import { currentAccountId } from "./accountScope";
import { resetDb } from "./dbCore";
import { openWritableVaultDb } from "./vaultSchema";

/** Move a group conversation into Trash. */
export function trashConversation(conversationId: number): void {
  const accountId = currentAccountId();
  if (!Number.isFinite(conversationId)) {
    throw new Error("conversationId required");
  }

  const writeDb = openWritableVaultDb();
  try {
    const row = writeDb
      .prepare(
        `SELECT 1 AS ok FROM conversations
         WHERE id = ? AND account_id = ? AND conversation_type = 'group'`,
      )
      .get(conversationId, accountId) as { ok: number } | undefined;
    if (!row) {
      throw new Error("group conversation not found");
    }
    writeDb
      .prepare(
        `INSERT INTO trashed_conversations (account_id, conversation_id, trashed_at)
         VALUES (?, ?, datetime('now'))
         ON CONFLICT(account_id, conversation_id) DO UPDATE SET trashed_at = excluded.trashed_at`,
      )
      .run(accountId, conversationId);
  } finally {
    writeDb.close();
  }
  resetDb();
}

/** Restore a group conversation from Trash. */
export function restoreConversation(conversationId: number): void {
  const accountId = currentAccountId();
  if (!Number.isFinite(conversationId)) {
    throw new Error("conversationId required");
  }

  const writeDb = openWritableVaultDb();
  try {
    writeDb
      .prepare(
        `DELETE FROM trashed_conversations WHERE account_id = ? AND conversation_id = ?`,
      )
      .run(accountId, conversationId);
  } finally {
    writeDb.close();
  }
  resetDb();
}

/**
 * Permanently remove a trashed group conversation (cascades messages) and
 * clears the trash entry.
 */
export function permanentlyDeleteConversation(conversationId: number): void {
  const accountId = currentAccountId();
  if (!Number.isFinite(conversationId)) {
    throw new Error("conversationId required");
  }

  const writeDb = openWritableVaultDb();
  try {
    const trashed = writeDb
      .prepare(
        `SELECT 1 AS ok FROM trashed_conversations
         WHERE account_id = ? AND conversation_id = ?`,
      )
      .get(accountId, conversationId) as { ok: number } | undefined;
    if (!trashed) {
      throw new Error("conversation is not in trash");
    }

    writeDb.pragma("foreign_keys = ON");
    const tx = writeDb.transaction(() => {
      writeDb
        .prepare(
          `DELETE FROM conversations
           WHERE id = ? AND account_id = ? AND conversation_type = 'group'`,
        )
        .run(conversationId, accountId);
      writeDb
        .prepare(
          `DELETE FROM trashed_conversations WHERE account_id = ? AND conversation_id = ?`,
        )
        .run(accountId, conversationId);
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
}
