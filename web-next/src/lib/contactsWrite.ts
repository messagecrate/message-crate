import Database from "better-sqlite3";
import { currentAccountId } from "./accountScope";
import { contactHandlesByContact, getDb, joinPreferredName } from "./dbCore";
import { getContact } from "./contactsRead";
import { resetDb } from "./dbCore";
import { openWritableVaultDb } from "./vaultSchema";
import {
  inferHandleType,
  normalizeHandle,
  type IdentityType,
} from "./handleKind";
import { clearTrashedHandles, resolveHandleId } from "./handlesWrite";
import type { ContactDetail } from "./types";
import {
  isReservedLabelName,
  RESERVED_LABEL_NAMES,
  reservedLabelError,
} from "./reservedLabels";
import {
  assertNotOwnerHandle,
  ownerHandleMatcher,
} from "./owner";
import {
  listUnassignedGroupParticipantHandles,
  listUnassignedHandles,
} from "./unassignedRead";

/** Bump `contacts.last_modified` after an address-book shape change. */
function touchContact(
  db: Database.Database,
  accountId: string,
  contactId: number,
): void {
  db.prepare(
    `UPDATE contacts SET last_modified = datetime('now')
     WHERE id = ? AND account_id = ?`,
  ).run(contactId, accountId);
}

/** One handle input for contact create/update. */
export type ContactHandleInput = {
  raw: string;
  /** Optional; inferred from the handle's shape when omitted. */
  handle_type?: IdentityType;
};

function normalizeHandleInputs(input: ContactHandleInput[]): Array<{
  raw: string;
  handle_type: IdentityType;
}> {
  const out: Array<{ raw: string; handle_type: IdentityType }> = [];
  const seen = new Set<string>();
  for (const h of input) {
    const raw = h.raw.trim();
    if (!raw) continue;
    const handle_type = h.handle_type ?? inferHandleType(raw);
    const key = `${handle_type}\0${normalizeHandle(raw, handle_type)}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ raw, handle_type });
  }
  return out;
}

export type ContactPatch = {
  labels?: string[];
  preferredName?: string | null;
  /** @deprecated Prefer preferredName; joined with lastName when preferredName omitted. */
  firstName?: string | null;
  /** @deprecated Prefer preferredName; joined with firstName when preferredName omitted. */
  lastName?: string | null;
  /** Handles as raw strings (legacy alias for handles; types are inferred). */
  phones?: string[];
  /** Handles with types; replaces phones when both are given. */
  handles?: ContactHandleInput[];
};

function assertAllowedLabelName(name: string): void {
  if (isReservedLabelName(name)) {
    throw new Error(reservedLabelError(name));
  }
}

export type ContactCreate = {
  preferredName?: string | null;
  /** @deprecated Prefer preferredName; joined with lastName when preferredName omitted. */
  firstName?: string | null;
  /** @deprecated Prefer preferredName; joined with firstName when preferredName omitted. */
  lastName?: string | null;
  /** Handles as raw strings (legacy alias for handles; types are inferred). */
  phones?: string[];
  /** Handles with types; replaces phones when both are given. */
  handles?: ContactHandleInput[];
  labels?: string[];
};

/** Resolve preferred display name from preferredName or legacy first+last. */
function resolvePreferredName(input: {
  preferredName?: string | null;
  firstName?: string | null;
  lastName?: string | null;
}): string | null {
  if (input.preferredName !== undefined) {
    return input.preferredName?.trim() || null;
  }
  return joinPreferredName(input.firstName, input.lastName);
}

function contactHasName(contact: ContactDetail): boolean {
  return Boolean((contact.preferredName ?? "").trim());
}

/** Insert a new contact in SQLite; returns the contact. */
export function createContact(input: ContactCreate): ContactDetail {
  const accountId = currentAccountId();
  const preferredName = resolvePreferredName(input);
  if (!preferredName) {
    throw new Error("display name required");
  }
  const handles = normalizeHandleInputs(
    input.handles ??
      (input.phones ?? []).map((p) => ({ raw: p })),
  );
  if (handles.length === 0) {
    throw new Error("at least one handle (phone or email) required");
  }
  const labels = (input.labels ?? [])
    .map((t) => t.trim())
    .filter(Boolean)
    .filter((t) => !RESERVED_LABEL_NAMES.has(t.toLowerCase()));

  for (const handle of handles) {
    assertNotOwnerHandle(handle.raw);
  }

  let newId = 0;
  const writeDb = openWritableVaultDb();
  try {
    const tx = writeDb.transaction(() => {
      for (const handle of handles) {
        const owner = handleOwner(writeDb, handle.raw, handle.handle_type, accountId);
        if (owner != null) {
          throw new Error(`handle ${handle.raw} already belongs to another contact`);
        }
      }

      const result = writeDb
        .prepare(
          `INSERT INTO contacts (
             account_id, preferred_name
           ) VALUES (?, ?)`,
        )
        .run(accountId, preferredName);
      newId = Number(result.lastInsertRowid);

      const insertHandle = writeDb.prepare(
        `INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES (?, ?, ?)`,
      );
      for (const handle of handles) {
        const handleId = resolveHandleId(
          writeDb,
          accountId,
          handle.raw,
          handle.handle_type,
        );
        insertHandle.run(accountId, handleId, newId);
      }
      clearTrashedHandles(
        writeDb,
        handles.map((h) => h.raw),
        accountId,
      );

      if (labels.length > 0) {
        const insertMember = writeDb.prepare(
          `INSERT OR IGNORE INTO contact_group_members (contact_id, group_id) VALUES (?, ?)`,
        );
        for (const name of labels) {
          const groupId = ensureLabelId(writeDb, name, accountId);
          insertMember.run(newId, groupId);
        }
      }
    });
    tx();
  } finally {
    writeDb.close();
  }

  resetDb();

  const created = getContact(newId);
  if (!created) {
    throw new Error("contact missing after create");
  }
  return created;
}

function ensureLabelId(
  db: Database.Database,
  name: string,
  accountId: string,
): number {
  assertAllowedLabelName(name);
  db.prepare(
    `INSERT OR IGNORE INTO contact_groups (account_id, name) VALUES (?, ?)`,
  ).run(accountId, name);
  const row = db
    .prepare(`SELECT id FROM contact_groups WHERE account_id = ? AND name = ?`)
    .get(accountId, name) as { id: number } | undefined;
  if (!row) throw new Error(`failed to ensure label ${name}`);
  return row.id;
}

function findLabelId(
  db: Database.Database,
  name: string,
  accountId: string,
): number | null {
  const row = db
    .prepare(`SELECT id FROM contact_groups WHERE account_id = ? AND name = ?`)
    .get(accountId, name) as { id: number } | undefined;
  return row?.id ?? null;
}

/** Add or remove one label for many contacts in one database transaction. */
export function setContactsLabelMembership(
  contactIds: number[],
  name: string,
  enable: boolean,
): number {
  const accountId = currentAccountId();
  const ids = [
    ...new Set(contactIds.filter((id) => Number.isFinite(id) && id > 0)),
  ];
  if (ids.length === 0) throw new Error("contact ids required");
  const label = name.trim();
  if (!label) throw new Error("label name required");
  assertAllowedLabelName(label);

  for (const id of ids) {
    const contact = getContact(id);
    if (!contact) throw new Error(`contact ${id} not found`);
  }
  const changedIds = new Set<number>();
  const writeDb = openWritableVaultDb();
  try {
    const tx = writeDb.transaction(() => {
      const labelId = enable
        ? ensureLabelId(writeDb, label, accountId)
        : findLabelId(writeDb, label, accountId);
      if (labelId == null) return;
      const insert = writeDb.prepare(
        `INSERT OR IGNORE INTO contact_group_members (contact_id, group_id)
         SELECT id, ? FROM contacts WHERE id = ? AND account_id = ?`,
      );
      const remove = writeDb.prepare(
        `DELETE FROM contact_group_members
         WHERE contact_id = ? AND group_id = ?
           AND EXISTS (
             SELECT 1 FROM contacts
             WHERE contacts.id = contact_group_members.contact_id
               AND contacts.account_id = ?
           )`,
      );
      for (const id of ids) {
        const result = enable
          ? insert.run(labelId, id, accountId)
          : remove.run(id, labelId, accountId);
        if (result.changes > 0) {
          changedIds.add(id);
          touchContact(writeDb, accountId, id);
        }
      }
    });
    tx();
  } finally {
    writeDb.close();
  }

  if (changedIds.size === 0) return 0;
  resetDb();
  return changedIds.size;
}


export function createLabel(name: string): string {
  const accountId = currentAccountId();
  const trimmed = name.trim();
  if (!trimmed) throw new Error("name required");
  assertAllowedLabelName(trimmed);

  const writeDb = openWritableVaultDb();
  try {
    const existing = writeDb
      .prepare(
        `SELECT name FROM contact_groups WHERE account_id = ? AND name = ? COLLATE NOCASE`,
      )
      .get(accountId, trimmed) as { name: string } | undefined;
    if (existing) {
      throw new Error("label already exists");
    }
    writeDb
      .prepare(`INSERT INTO contact_groups (account_id, name) VALUES (?, ?)`)
      .run(accountId, trimmed);
  } finally {
    writeDb.close();
  }

  resetDb();
  return trimmed;
}

export function renameLabel(from: string, to: string): string {
  const accountId = currentAccountId();
  const oldName = from.trim();
  const newName = to.trim();
  if (!oldName || !newName) throw new Error("name required");
  assertAllowedLabelName(newName);
  if (oldName.toLowerCase() === newName.toLowerCase()) {
    // Same name ignoring case — allow casing fix
    if (oldName === newName) return newName;
  }

  const writeDb = openWritableVaultDb();
  try {
    const id = findLabelId(writeDb, oldName, accountId);
    if (id == null) throw new Error("label not found");

    const clash = writeDb
      .prepare(
        `SELECT id FROM contact_groups
         WHERE account_id = ? AND name = ? COLLATE NOCASE AND id != ?`,
      )
      .get(accountId, newName, id) as { id: number } | undefined;
    if (clash) throw new Error("label already exists");

    writeDb
      .prepare(`UPDATE contact_groups SET name = ? WHERE id = ? AND account_id = ?`)
      .run(newName, id, accountId);
  } finally {
    writeDb.close();
  }

  resetDb();
  return newName;
}

export function deleteLabel(name: string): void {
  const accountId = currentAccountId();
  const trimmed = name.trim();
  if (!trimmed) throw new Error("name required");

  const writeDb = openWritableVaultDb();
  try {
    const id = findLabelId(writeDb, trimmed, accountId);
    if (id == null) throw new Error("label not found");
    writeDb
      .prepare(`DELETE FROM contact_group_members WHERE group_id = ?`)
      .run(id);
    writeDb
      .prepare(`DELETE FROM contact_groups WHERE id = ? AND account_id = ?`)
      .run(id, accountId);
  } finally {
    writeDb.close();
  }

  resetDb();
}

/** Contact that owns a handle (by normalized identity), if any. */
function handleOwner(
  db: Database.Database,
  raw: string,
  handleType: IdentityType,
  accountId: string,
): number | null {
  const normalized = normalizeHandle(raw, handleType);
  const row = db
    .prepare(
      `SELECT cp.contact_id AS contact_id
       FROM handles h
       JOIN contact_handles cp ON cp.handle_id = h.id AND cp.account_id = h.account_id
       WHERE h.account_id = ? AND h.normalized = ? AND h.handle_type = ?`,
    )
    .get(accountId, normalized, handleType) as { contact_id: number } | undefined;
  return row?.contact_id ?? null;
}

/**
 * Retarget message/conversation handle links when a contact handle changes so
 * the person stays linked to their threads (list filters require handle joins).
 */
function remapHandle(
  db: Database.Database,
  contactId: number,
  from: { raw: string; handle_type: IdentityType },
  to: { raw: string; handle_type: IdentityType },
  accountId: string,
): void {
  if (from.raw === to.raw && from.handle_type === to.handle_type) return;

  const owner = handleOwner(db, to.raw, to.handle_type, accountId);
  if (owner != null && owner !== contactId) {
    throw new Error(`handle ${to.raw} already belongs to another contact`);
  }

  const fromId = resolveHandleId(db, accountId, from.raw, from.handle_type);
  const toId = resolveHandleId(db, accountId, to.raw, to.handle_type);

  // The edit is only a raw reformatting of the same identity (guarded
  // normalization is unchanged): nothing to re-point. The review note stays —
  // the value is still ambiguous.
  if (fromId === toId) return;

  // Prefer updating in place; if `to` already exists on this contact, drop the
  // old link instead (merge).
  if (owner === contactId) {
    db.prepare(
      `DELETE FROM contact_handles WHERE account_id = ? AND handle_id = ?`,
    ).run(accountId, fromId);
  } else {
    db.prepare(
      `UPDATE contact_handles SET handle_id = ?
       WHERE account_id = ? AND handle_id = ?`,
    ).run(toId, accountId, fromId);
  }

  db.prepare(
    `UPDATE conversations SET chat_handle_id = ?
     WHERE account_id = ? AND chat_handle_id = ?`,
  ).run(toId, accountId, fromId);
  db.prepare(`UPDATE participants SET handle_id = ? WHERE handle_id = ?`).run(toId, fromId);
  db.prepare(`UPDATE messages SET sender_handle_id = ? WHERE sender_handle_id = ?`).run(toId, fromId);
  db.prepare(`UPDATE tapbacks SET sender_handle_id = ? WHERE sender_handle_id = ?`).run(toId, fromId);

  // Re-normalizing a flagged handle clears its review note: the old row is
  // orphaned after re-pointing, and the new row normalizes cleanly.
  db.prepare(`UPDATE handles SET normalized_note = NULL WHERE id = ?`).run(fromId);
}

function syncContactHandles(
  db: Database.Database,
  contactId: number,
  oldHandles: Array<{ raw: string; handle_type: IdentityType }>,
  newHandles: Array<{ raw: string; handle_type: IdentityType }>,
  accountId: string,
): void {
  const shared = Math.min(oldHandles.length, newHandles.length);
  for (let i = 0; i < shared; i++) {
    const from = oldHandles[i]!;
    const to = newHandles[i]!;
    if (from.raw !== to.raw || from.handle_type !== to.handle_type) {
      remapHandle(db, contactId, from, to, accountId);
    }
  }

  for (let i = shared; i < oldHandles.length; i++) {
    const old = oldHandles[i]!;
    const oldId = resolveHandleId(db, accountId, old.raw, old.handle_type);
    db.prepare(
      `DELETE FROM contact_handles WHERE account_id = ? AND handle_id = ?`,
    ).run(accountId, oldId);
  }

  const insert = db.prepare(
    `INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES (?, ?, ?)`,
  );
  for (let i = shared; i < newHandles.length; i++) {
    const next = newHandles[i]!;
    const owner = handleOwner(db, next.raw, next.handle_type, accountId);
    if (owner != null && owner !== contactId) {
      throw new Error(`handle ${next.raw} already belongs to another contact`);
    }
    if (owner == null) {
      const handleId = resolveHandleId(db, accountId, next.raw, next.handle_type);
      insert.run(accountId, handleId, contactId);
    }
  }
}

/** Update contact fields in SQLite; returns refreshed contact. */
export function patchContact(
  id: number,
  patch: ContactPatch,
): ContactDetail {
  const accountId = currentAccountId();
  const existing = getContact(id);
  if (!existing) {
    throw new Error("contact not found");
  }

  const labels = patch.labels ?? existing.labels;

  let preferredName = existing.preferredName;
  if (patch.preferredName !== undefined) {
    preferredName = patch.preferredName?.trim() || null;
  } else if (patch.firstName !== undefined || patch.lastName !== undefined) {
    preferredName = joinPreferredName(
      patch.firstName !== undefined ? patch.firstName : existing.firstName,
      patch.lastName !== undefined ? patch.lastName : existing.lastName,
    );
  }

  const handlesChanged =
    patch.handles !== undefined || patch.phones !== undefined;
  const nextHandles = handlesChanged
    ? normalizeHandleInputs(
        patch.handles ??
          (patch.phones ?? []).map((p) => ({ raw: p })),
      )
    : existing.handles.map((h) => ({ raw: h.raw, handle_type: h.handle_type }));

  if (nextHandles.length === 0) {
    throw new Error("at least one handle (phone or email) required");
  }
  if (handlesChanged) {
    for (const handle of nextHandles) {
      assertNotOwnerHandle(handle.raw);
    }
  }

  const writeDb = openWritableVaultDb();
  try {
    const tx = writeDb.transaction(() => {
      if (handlesChanged) {
        syncContactHandles(
          writeDb,
          id,
          existing.handles.map((h) => ({ raw: h.raw, handle_type: h.handle_type })),
          nextHandles,
          accountId,
        );
        clearTrashedHandles(
          writeDb,
          nextHandles.map((h) => h.raw),
          accountId,
        );
      }

      writeDb
        .prepare(
          `UPDATE contacts
           SET preferred_name = ?
           WHERE id = ? AND account_id = ?`,
        )
        // preferred_name is NOT NULL; an empty string renders as "no name"
        // (displayName falls back to the preferred handle).
        .run(preferredName ?? "", id, accountId);

      if (handlesChanged || patch.preferredName !== undefined || patch.firstName !== undefined || patch.lastName !== undefined || patch.labels) {
        touchContact(writeDb, accountId, id);
      }

      if (patch.labels) {
        writeDb
          .prepare(`DELETE FROM contact_group_members WHERE contact_id = ?`)
          .run(id);
        const insert = writeDb.prepare(
          `INSERT OR IGNORE INTO contact_group_members (contact_id, group_id) VALUES (?, ?)`,
        );
        for (const name of labels) {
          const groupId = ensureLabelId(writeDb, name, accountId);
          insert.run(id, groupId);
        }
      }
    });
    tx();
  } finally {
    writeDb.close();
  }

  resetDb();

  const updated = getContact(id);
  if (!updated) {
    throw new Error("contact missing after update");
  }
  return updated;
}

/** @deprecated Prefer {@link addHandleToContact} (type inferred from the raw). */
export function addPhoneToContact(id: number, phone: string): ContactDetail {
  return addHandleToContact(id, phone);
}

/**
 * Append a handle to an existing contact (Unassigned assign, handle edit).
 * `handleType` disambiguates the handles-table identity; when omitted it is
 * inferred from the raw's shape.
 */
export function addHandleToContact(
  id: number,
  raw: string,
  handleType?: IdentityType,
): ContactDetail {
  const accountId = currentAccountId();
  const existing = getContact(id);
  if (!existing) throw new Error("contact not found");
  const trimmed = raw.trim();
  if (!trimmed) throw new Error("handle required");
  assertNotOwnerHandle(trimmed);
  if (existing.phones.includes(trimmed)) return existing;

  const type = handleType ?? inferHandleType(trimmed);
  const writeDb = openWritableVaultDb();
  try {
    const owner = handleOwner(writeDb, trimmed, type, accountId);
    if (owner != null && owner !== id) {
      throw new Error(`handle ${trimmed} already belongs to another contact`);
    }
    if (owner == null) {
      const handleId = resolveHandleId(writeDb, accountId, trimmed, type);
      writeDb
        .prepare(
          `INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES (?, ?, ?)`,
        )
        .run(accountId, handleId, id);
      clearTrashedHandles(writeDb, [trimmed], accountId);
      touchContact(writeDb, accountId, id);
    }
  } finally {
    writeDb.close();
  }
  resetDb();
  const updated = getContact(id);
  if (!updated) throw new Error("contact missing after update");
  return updated;
}

/** @deprecated Prefer {@link removeHandleFromContact} (type inferred from the raw). */
export function removePhoneFromContact(
  id: number,
  phone: string,
): ContactDetail {
  return removeHandleFromContact(id, phone);
}

/**
 * Remove a handle from a contact. Does not delete conversations or messages.
 * Used to undo assign-from-unassigned.
 */
export function removeHandleFromContact(
  id: number,
  raw: string,
  handleType?: IdentityType,
): ContactDetail {
  const accountId = currentAccountId();
  const existing = getContact(id);
  if (!existing) throw new Error("contact not found");
  const trimmed = raw.trim();
  if (!trimmed) throw new Error("handle required");
  if (!existing.phones.includes(trimmed)) {
    throw new Error("handle not on contact");
  }

  const type = handleType ?? inferHandleType(trimmed);
  const writeDb = openWritableVaultDb();
  try {
    const owner = handleOwner(writeDb, trimmed, type, accountId);
    if (owner != null && owner !== id) {
      throw new Error(`handle ${trimmed} already belongs to another contact`);
    }
    const handleId = resolveHandleId(writeDb, accountId, trimmed, type);
    writeDb
      .prepare(
        `DELETE FROM contact_handles WHERE account_id = ? AND handle_id = ?`,
      )
      .run(accountId, handleId);
    touchContact(writeDb, accountId, id);
  } finally {
    writeDb.close();
  }
  resetDb();
  const updated = getContact(id);
  if (!updated) throw new Error("contact missing after update");
  return updated;
}

/**
 * Recreate a deleted group and re-attach member contacts.
 * Contacts that no longer exist are skipped.
 */
export function restoreLabel(
  name: string,
  memberContactIds: number[],
): string {
  const trimmed = name.trim();
  if (!trimmed) throw new Error("name required");
  assertAllowedLabelName(trimmed);

  const created = createLabel(trimmed);
  for (const contactId of memberContactIds) {
    const contact = getContact(contactId);
    if (!contact) continue;
    if (contact.labels.some((g) => g.toLowerCase() === created.toLowerCase())) {
      continue;
    }
    patchContact(contactId, {
      labels: [...contact.labels, created].sort((a, b) =>
        a.localeCompare(b, undefined, { sensitivity: "base" }),
      ),
    });
  }
  return created;
}

/**
 * Create nameless contacts for handles with messages but no contact: 1:1 handles
 * that still appear as Unassigned, plus group participants who never had a 1:1
 * thread. Returns how many contacts were created. No-op when read-only.
 */
export function ensureUnknownContacts(): number {
  try {
  } catch {
    return 0;
  }
  const accountId = currentAccountId();
  // Owner handles are resolved up front: the matcher opens its own connection,
  // which would deadlock against the write transaction below.
  const isOwner = ownerHandleMatcher();
  const candidates = [
    ...listUnassignedHandles().map((row) => ({
      handle: row.handle,
      handleType: row.handleType,
      nameAlias: row.nameAlias,
    })),
    ...listUnassignedGroupParticipantHandles(),
  ];
  const byIdentity = new Map<
    string,
    { raw: string; handleType: IdentityType; nameAlias: string | null }
  >();
  for (const candidate of candidates) {
    const raw = candidate.handle.trim();
    if (!raw || isOwner(raw)) continue;
    const handleType = candidate.handleType ?? inferHandleType(raw);
    const key = `${handleType}\0${normalizeHandle(raw, handleType)}`;
    const hint = candidate.nameAlias?.trim() || null;
    const prev = byIdentity.get(key);
    if (!prev || (!prev.nameAlias && hint)) {
      byIdentity.set(key, { raw, handleType, nameAlias: hint });
    }
  }
  if (byIdentity.size === 0) return 0;

  let created = 0;
  const writeDb = openWritableVaultDb();
  try {
    const tx = writeDb.transaction(() => {
      for (const entry of byIdentity.values()) {
        const owner = handleOwner(writeDb, entry.raw, entry.handleType, accountId);
        if (owner != null) continue;

        const result = writeDb
          .prepare(
            `INSERT INTO contacts (
               account_id, preferred_name
             ) VALUES (?, ?)`,
          )
          // preferred_name is NOT NULL; an empty string keeps the contact
          // nameless (display falls back to the handle).
          .run(accountId, entry.nameAlias ?? "");
        const newId = Number(result.lastInsertRowid);
        const handleId = resolveHandleId(
          writeDb,
          accountId,
          entry.raw,
          entry.handleType,
        );
        writeDb
          .prepare(
            `INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES (?, ?, ?)`,
          )
          .run(accountId, handleId, newId);
        clearTrashedHandles(writeDb, [entry.raw], accountId);
        created += 1;
      }
    });
    tx();
  } finally {
    writeDb.close();
  }
  resetDb();
  return created;
}

/**
 * Move all handles from a nameless source contact onto a named target, then
 * delete the source. Messages stay linked via handle ids; group participants
 * are re-pointed at the target contact.
 */
export function mergeContacts(fromId: number, intoId: number): ContactDetail {
  if (fromId === intoId) throw new Error("cannot merge a contact into itself");

  const source = getContact(fromId);
  if (!source) throw new Error("source contact not found");
  const target = getContact(intoId);
  if (!target) throw new Error("target contact not found");

  if (contactHasName(source)) {
    throw new Error("only nameless contacts can be merged into another contact");
  }
  if (!contactHasName(target)) {
    throw new Error("merge target must have a name");
  }

  const accountId = currentAccountId();
  // Handle rows carry handle_id (ContactDetail.handles does not); resolve them
  // from the shared readonly connection before the write transaction opens.
  const sourceHandles =
    contactHandlesByContact(getDb(), accountId, [fromId]).get(fromId) ?? [];

  const writeDb = openWritableVaultDb();
  try {
    const tx = writeDb.transaction(() => {
      for (const handle of sourceHandles) {
        const owner = handleOwner(writeDb, handle.raw, handle.handle_type, accountId);
        if (owner != null && owner !== fromId && owner !== intoId) {
          throw new Error(`handle ${handle.raw} already belongs to another contact`);
        }
        if (owner === intoId) {
          writeDb
            .prepare(
              `DELETE FROM contact_handles WHERE account_id = ? AND handle_id = ? AND contact_id = ?`,
            )
            .run(accountId, handle.handle_id, fromId);
          continue;
        }
        writeDb
          .prepare(
            `UPDATE contact_handles SET contact_id = ?
             WHERE account_id = ? AND handle_id = ? AND contact_id = ?`,
          )
          .run(intoId, accountId, handle.handle_id, fromId);
      }
      // Participants follow the merge on their own: a participant names a
      // handle, and the moved contact_handles rows say whose it is.
      writeDb
        .prepare(`DELETE FROM contact_group_members WHERE contact_id = ?`)
        .run(fromId);
      writeDb
        .prepare(`DELETE FROM contacts WHERE id = ? AND account_id = ?`)
        .run(fromId, accountId);
      touchContact(writeDb, accountId, intoId);
    });
    tx();
  } finally {
    writeDb.close();
  }

  resetDb();

  const updated = getContact(intoId);
  if (!updated) throw new Error("target missing after merge");
  return updated;
}

/** Delete contacts from SQLite. */
export function deleteContacts(ids: number[]): number {
  const accountId = currentAccountId();
  const unique = [...new Set(ids.filter((id) => Number.isFinite(id)))];
  if (unique.length === 0) return 0;

  let existingCount = 0;
  for (const id of unique) {
    const existing = getContact(id);
    if (!existing) continue;
    existingCount += 1;
  }
  if (existingCount === 0) {
    throw new Error("contact not found");
  }

  const writeDb = openWritableVaultDb();
  try {
    const del = writeDb.prepare(
      `DELETE FROM contacts WHERE id = ? AND account_id = ?`,
    );
    const tx = writeDb.transaction(() => {
      for (const id of unique) {
        del.run(id, accountId);
      }
    });
    tx();
  } finally {
    writeDb.close();
  }

  resetDb();
  return existingCount;
}
