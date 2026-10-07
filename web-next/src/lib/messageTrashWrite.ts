import Database from "better-sqlite3";

import { currentAccountId } from "./accountScope";
import { resetDb } from "./dbCore";
import {
  clearTrashedHandles,
  noConversationToTrash,
  trashHandlesInDb,
} from "./handlesWrite";
import { openWritableVaultDb } from "./vaultSchema";

export type MessageTrashTargets = {
  handles: string[];
  conversationIds: number[];
};

export type MessageTrashWriteResult = MessageTrashTargets & {
  count: number;
};

function normalizeTargets(targets: MessageTrashTargets): MessageTrashTargets {
  return {
    handles: [
      ...new Set(targets.handles.map((handle) => handle.trim()).filter(Boolean)),
    ],
    conversationIds: [
      ...new Set(targets.conversationIds.filter((id) => Number.isFinite(id))),
    ],
  };
}

/**
 * Trash or restore direct threads (named by handle, written as the handle's
 * 1:1 conversation) and group conversations atomically. Direct handles remain
 * assigned to their contacts. A handle with no 1:1 conversation refuses the
 * whole batch.
 */
export function setMessageTrashInDb(
  db: Database.Database,
  targets: MessageTrashTargets,
  trashed: boolean,
  accountId: string = currentAccountId(),
): MessageTrashWriteResult {
  const normalized = normalizeTargets(targets);
  if (normalized.handles.length + normalized.conversationIds.length === 0) {
    throw new Error("handles or conversationIds required");
  }

  const transaction = db.transaction(() => {
    if (trashed) {
      const findGroup = db.prepare(
        `SELECT 1 AS ok FROM conversations
         WHERE id = ? AND account_id = ? AND conversation_type = 'group'`,
      );
      for (const conversationId of normalized.conversationIds) {
        if (!findGroup.get(conversationId, accountId)) {
          throw new Error(`group conversation ${conversationId} not found`);
        }
      }

      const trashConversation = db.prepare(
        `INSERT INTO trashed_conversations (account_id, conversation_id, trashed_at)
         VALUES (?, ?, datetime('now'))
         ON CONFLICT(account_id, conversation_id) DO UPDATE SET trashed_at = excluded.trashed_at`,
      );
      const missing = trashHandlesInDb(db, normalized.handles, accountId);
      if (missing.length > 0) throw noConversationToTrash(missing);
      for (const conversationId of normalized.conversationIds) {
        trashConversation.run(accountId, conversationId);
      }
      return;
    }

    const restoreConversation = db.prepare(
      `DELETE FROM trashed_conversations
       WHERE account_id = ? AND conversation_id = ?`,
    );
    clearTrashedHandles(db, normalized.handles, accountId);
    for (const conversationId of normalized.conversationIds) {
      restoreConversation.run(accountId, conversationId);
    }
  });
  transaction();

  return {
    ...normalized,
    count: normalized.handles.length + normalized.conversationIds.length,
  };
}

function writeMessageTrash(
  targets: MessageTrashTargets,
  trashed: boolean,
): MessageTrashWriteResult {
  const accountId = currentAccountId();
  const writeDb = openWritableVaultDb();
  try {
    return setMessageTrashInDb(writeDb, targets, trashed, accountId);
  } finally {
    writeDb.close();
    resetDb();
  }
}

/** Trash a mixed batch of direct and group message threads. */
export function trashMessageThreads(
  targets: MessageTrashTargets,
): MessageTrashWriteResult {
  return writeMessageTrash(targets, true);
}

/** Restore a mixed batch of direct and group message threads. */
export function restoreMessageThreads(
  targets: MessageTrashTargets,
): MessageTrashWriteResult {
  return writeMessageTrash(targets, false);
}
