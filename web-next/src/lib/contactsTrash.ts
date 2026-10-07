import type Database from "better-sqlite3";
import { currentAccountId } from "./accountScope";
import { getContact } from "./contactsRead";
import { resetDb } from "./dbCore";
import { deleteContacts } from "./contactsWrite";
import {
  INDIVIDUAL_CONVERSATIONS_OF_HANDLE,
  trashHandlesInDb,
} from "./handlesWrite";
import { openWritableVaultDb } from "./vaultSchema";

function contactHandleIds(
  db: Database.Database,
  contactId: number,
  accountId: string,
): number[] {
  return (
    db
      .prepare(
        `SELECT handle_id FROM contact_handles WHERE contact_id = ? AND account_id = ?`,
      )
      .all(contactId, accountId) as Array<{ handle_id: number }>
  ).map((r) => r.handle_id);
}

function contactHandles(
  db: Database.Database,
  contactId: number,
  accountId: string,
): string[] {
  return (
    db
      .prepare(
        `SELECT h.raw AS handle
         FROM contact_handles cp
         JOIN handles h ON h.id = cp.handle_id
         WHERE cp.contact_id = ? AND cp.account_id = ?`,
      )
      .all(contactId, accountId) as Array<{ handle: string }>
  ).map((r) => r.handle);
}

/** Handles on a contact that have at least one 1:1 message. */
function contactHandlesWithMessages(
  db: Database.Database,
  contactId: number,
  accountId: string,
): string[] {
  return (
    db
      .prepare(
        `SELECT h.raw AS handle
         FROM contact_handles cp
         JOIN handles h ON h.id = cp.handle_id
         WHERE cp.contact_id = ? AND cp.account_id = ?
           AND EXISTS (
             SELECT 1
             FROM conversations c
             JOIN messages m ON m.conversation_id = c.id
             WHERE c.conversation_type = 'individual'
               AND c.chat_handle_id = cp.handle_id
               AND c.account_id = cp.account_id
           )`,
      )
      .all(contactId, accountId) as Array<{ handle: string }>
  ).map((r) => r.handle);
}

function assertContactsExist(ids: number[]): void {
  for (const id of ids) {
    if (!getContact(id)) {
      throw new Error(`contact ${id} not found`);
    }
  }
}

/** Soft-trash contacts and the 1:1 conversations of all their handles. */
export function trashContactWithMessages(ids: number[]): number {
  const accountId = currentAccountId();
  const unique = [...new Set(ids.filter((id) => Number.isFinite(id)))];
  if (unique.length === 0) return 0;
  assertContactsExist(unique);

  const writeDb = openWritableVaultDb();
  try {
    const upsertContact = writeDb.prepare(
      `INSERT INTO trashed_contacts (account_id, contact_id, trashed_at)
       VALUES (?, ?, datetime('now'))
       ON CONFLICT(account_id, contact_id) DO UPDATE SET trashed_at = excluded.trashed_at`,
    );
    const tx = writeDb.transaction(() => {
      for (const id of unique) {
        upsertContact.run(accountId, id);
        trashHandlesInDb(
          writeDb,
          contactHandles(writeDb, id, accountId),
          accountId,
        );
      }
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
  return unique.length;
}

/** Soft-trash the 1:1 conversations of contacts' handles; leave contacts visible. */
export function trashContactMessagesOnly(ids: number[]): {
  count: number;
  handles: string[];
} {
  const accountId = currentAccountId();
  const unique = [...new Set(ids.filter((id) => Number.isFinite(id)))];
  if (unique.length === 0) return { count: 0, handles: [] };
  assertContactsExist(unique);

  const handles: string[] = [];
  const writeDb = openWritableVaultDb();
  try {
    const tx = writeDb.transaction(() => {
      for (const id of unique) {
        const next = contactHandlesWithMessages(writeDb, id, accountId);
        handles.push(...next);
        trashHandlesInDb(writeDb, next, accountId);
      }
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
  return { count: unique.length, handles: [...new Set(handles)] };
}

/** Restore soft-trashed contacts and their handles' 1:1 conversations. */
export function restoreTrashedContacts(ids: number[]): number {
  const accountId = currentAccountId();
  const unique = [...new Set(ids.filter((id) => Number.isFinite(id)))];
  if (unique.length === 0) return 0;

  const writeDb = openWritableVaultDb();
  try {
    const delContact = writeDb.prepare(
      `DELETE FROM trashed_contacts WHERE account_id = ? AND contact_id = ?`,
    );
    const delHandleConversations = writeDb.prepare(
      `DELETE FROM trashed_conversations
       WHERE account_id = ? AND conversation_id IN (${INDIVIDUAL_CONVERSATIONS_OF_HANDLE})`,
    );
    const tx = writeDb.transaction(() => {
      for (const id of unique) {
        const trashed = writeDb
          .prepare(
            `SELECT 1 AS ok FROM trashed_contacts WHERE account_id = ? AND contact_id = ?`,
          )
          .get(accountId, id) as { ok: number } | undefined;
        if (!trashed) {
          throw new Error(`contact ${id} is not in trash`);
        }
        const handleIds = contactHandleIds(writeDb, id, accountId);
        delContact.run(accountId, id);
        for (const handleId of handleIds) {
          delHandleConversations.run(accountId, accountId, handleId);
        }
      }
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
  return unique.length;
}

/**
 * Permanently delete soft-trashed contacts: wipe 1:1 conversations for their
 * handles, then hard-delete the contact rows (+ CSV).
 */
export function permanentlyDeleteTrashedContacts(ids: number[]): number {
  const accountId = currentAccountId();
  const unique = [...new Set(ids.filter((id) => Number.isFinite(id)))];
  if (unique.length === 0) return 0;

  const writeDb = openWritableVaultDb();
  try {
    writeDb.pragma("foreign_keys = ON");
    const delConv = writeDb.prepare(
      `DELETE FROM conversations
       WHERE account_id = ? AND conversation_type = 'individual' AND chat_handle_id = ?`,
    );
    const delHandleConversationTrash = writeDb.prepare(
      `DELETE FROM trashed_conversations
       WHERE account_id = ? AND conversation_id IN (${INDIVIDUAL_CONVERSATIONS_OF_HANDLE})`,
    );
    const delContactTrash = writeDb.prepare(
      `DELETE FROM trashed_contacts WHERE account_id = ? AND contact_id = ?`,
    );
    const tx = writeDb.transaction(() => {
      for (const id of unique) {
        const trashed = writeDb
          .prepare(
            `SELECT 1 AS ok FROM trashed_contacts WHERE account_id = ? AND contact_id = ?`,
          )
          .get(accountId, id) as { ok: number } | undefined;
        if (!trashed) {
          throw new Error(`contact ${id} is not in trash`);
        }
        const handleIds = contactHandleIds(writeDb, id, accountId);
        for (const handleId of handleIds) {
          delHandleConversationTrash.run(accountId, accountId, handleId);
          delConv.run(accountId, handleId);
        }
        delContactTrash.run(accountId, id);
      }
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
  return deleteContacts(unique);
}
