import Database from "better-sqlite3";
import { currentAccountId } from "./accountScope";
import { handleIdsForRaws, resetDb } from "./dbCore";
import {
  inferHandleType,
  normalizeHandle,
  phoneReviewNote,
  type IdentityType,
} from "./handleKind";
import { openWritableVaultDb } from "./vaultSchema";

/**
 * Ensure a `handles` row exists for (raw, handle_type, service) and return its
 * id. Matching includes the platform service so the same phone can be used for
 * both text messaging and WhatsApp.
 */
export function resolveHandleId(
  db: Database.Database,
  accountId: string,
  raw: string,
  handleType: IdentityType,
  service = "phone",
): number {
  const trimmed = raw.trim();
  if (!trimmed) throw new Error("handle required");
  const normalized = normalizeHandle(trimmed, handleType);
  // Guarded policy: ambiguous phone values carry a review note (mirroring the
  // import-time note), so web-created handles surface the same needs-review
  // badge. Rows created by an import keep their existing note.
  const note = handleType === "phone" ? phoneReviewNote(trimmed) : null;
  db.prepare(
    `INSERT OR IGNORE INTO handles (account_id, raw, normalized, normalized_note, handle_type, service)
     VALUES (?, ?, ?, ?, ?, ?)`,
  ).run(accountId, trimmed, normalized, note, handleType, service);
  const row = db
    .prepare(
      `SELECT id FROM handles
       WHERE account_id = ? AND normalized = ? AND handle_type = ? AND service = ?`,
    )
    .get(accountId, normalized, handleType, service) as { id: number } | undefined;
  if (!row) throw new Error(`failed to resolve handle ${trimmed}`);
  return row.id;
}

/**
 * Handle id for an existing raw handle, or null when no such handle row.
 * `handleType` disambiguates when the raw is known by type (e.g. from the
 * unassigned/trash lists); omitted, the type is inferred from the raw's shape.
 */
export function handleIdForRaw(
  db: Database.Database,
  accountId: string,
  raw: string,
  handleType?: IdentityType,
  service = "phone",
): number | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const type = handleType ?? inferHandleType(trimmed);
  const normalized = normalizeHandle(trimmed, type);
  const row = db
    .prepare(
      `SELECT id FROM handles
       WHERE account_id = ? AND normalized = ? AND handle_type = ? AND service = ?`,
    )
    .get(accountId, normalized, type, service) as { id: number } | undefined;
  return row?.id ?? null;
}

/**
 * A handle has no trash of its own: what goes in the trash is the handle's
 * 1:1 conversation (`trashed_conversations`), as on the server.
 */
const INDIVIDUAL_CONVERSATIONS_OF_HANDLE = `SELECT id FROM conversations
   WHERE account_id = ? AND conversation_type = 'individual' AND chat_handle_id = ?`;

/** Take one handle's 1:1 conversation out of the trash. */
export function untrashHandleConversations(
  db: Database.Database,
  accountId: string,
  handleId: number,
): void {
  db.prepare(
    `DELETE FROM trashed_conversations
     WHERE account_id = ? AND conversation_id IN (${INDIVIDUAL_CONVERSATIONS_OF_HANDLE})`,
  ).run(accountId, accountId, handleId);
}

/** Take these handles' 1:1 conversations out of the trash (e.g. after assigning to a contact). */
export function clearTrashedHandles(
  db: Database.Database,
  handles: string[],
  accountId: string = currentAccountId(),
): void {
  for (const handleId of handleIdsForRaws(db, accountId, handles)) {
    untrashHandleConversations(db, accountId, handleId);
  }
}

/**
 * Put these handles' 1:1 conversations in the trash (owned or unassigned).
 * `handleType` applies to every handle in the batch when given; otherwise each
 * handle's type is inferred from its raw shape.
 *
 * Returns the handles that have no 1:1 conversation (a handle seen only in
 * group chats): nothing was trashed for them, and the caller decides whether
 * that refuses the request.
 */
export function trashHandlesInDb(
  db: Database.Database,
  handles: string[],
  accountId: string = currentAccountId(),
  handleType?: IdentityType,
): string[] {
  const trimmed = [...new Set(handles.map((h) => h.trim()).filter(Boolean))];
  if (trimmed.length === 0) return [];
  // `WHERE true` is not a leftover: SQLite needs a WHERE between a SELECT and
  // ON CONFLICT, or it reads ON as the start of a join constraint.
  const upsert = db.prepare(
    `INSERT INTO trashed_conversations (account_id, conversation_id, trashed_at)
     SELECT ?, id, datetime('now') FROM (${INDIVIDUAL_CONVERSATIONS_OF_HANDLE})
     WHERE true
     ON CONFLICT(account_id, conversation_id) DO UPDATE SET trashed_at = excluded.trashed_at`,
  );
  const nothingToTrash: string[] = [];
  for (const handle of trimmed) {
    const type = handleType ?? inferHandleType(handle);
    const handleId = resolveHandleId(db, accountId, handle, type);
    if (upsert.run(accountId, accountId, handleId).changes === 0) {
      nothingToTrash.push(handle);
    }
  }
  return nothingToTrash;
}

/** The error for handles that have no 1:1 conversation to trash. */
export function noConversationToTrash(handles: string[]): Error {
  return new Error(
    `nothing to trash: no one-to-one conversation for ${handles.join(", ")}`,
  );
}

/** Move a handle's 1:1 conversation into Trash (the handle may still belong to a contact). */
export function trashHandle(handle: string, handleType?: IdentityType): void {
  const accountId = currentAccountId();
  const trimmed = handle.trim();
  if (!trimmed) throw new Error("handle required");

  const writeDb = openWritableVaultDb();
  try {
    const trash = writeDb.transaction(() => {
      const missing = trashHandlesInDb(writeDb, [trimmed], accountId, handleType);
      if (missing.length > 0) throw noConversationToTrash(missing);
    });
    trash();
  } finally {
    writeDb.close();
  }
  resetDb();
}

/** Restore a handle's 1:1 conversation from Trash. */
export function restoreHandle(handle: string, handleType?: IdentityType): void {
  const accountId = currentAccountId();
  const trimmed = handle.trim();
  if (!trimmed) throw new Error("handle required");

  const writeDb = openWritableVaultDb();
  try {
    const handleId = handleIdForRaw(writeDb, accountId, trimmed, handleType);
    if (handleId != null) {
      untrashHandleConversations(writeDb, accountId, handleId);
    }
  } finally {
    writeDb.close();
  }
  resetDb();
}

/**
 * Permanently remove a handle's trashed 1:1 conversation (cascades
 * messages/attachments) and its trash entry. Contact ownership is OK
 * (messages-only trash for a live contact).
 */
export function permanentlyDeleteHandle(
  handle: string,
  handleType?: IdentityType,
): void {
  const accountId = currentAccountId();
  const trimmed = handle.trim();
  if (!trimmed) throw new Error("handle required");

  const writeDb = openWritableVaultDb();
  try {
    const handleId = handleIdForRaw(writeDb, accountId, trimmed, handleType);
    if (handleId == null) {
      throw new Error("handle is not in trash");
    }
    const trashedIds = (
      writeDb
        .prepare(
          `SELECT tc.conversation_id AS id
           FROM trashed_conversations tc
           WHERE tc.account_id = ?
             AND tc.conversation_id IN (${INDIVIDUAL_CONVERSATIONS_OF_HANDLE})`,
        )
        .all(accountId, accountId, handleId) as Array<{ id: number }>
    ).map((row) => row.id);
    if (trashedIds.length === 0) {
      throw new Error("handle is not in trash");
    }

    writeDb.pragma("foreign_keys = ON");
    const deleteConversation = writeDb.prepare(
      `DELETE FROM conversations WHERE account_id = ? AND id = ?`,
    );
    const deleteTrashEntry = writeDb.prepare(
      `DELETE FROM trashed_conversations WHERE account_id = ? AND conversation_id = ?`,
    );
    const tx = writeDb.transaction(() => {
      for (const id of trashedIds) {
        deleteConversation.run(accountId, id);
        deleteTrashEntry.run(accountId, id);
      }
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
}
