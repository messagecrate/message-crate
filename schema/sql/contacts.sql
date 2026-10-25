-- Address-book person for one account.
CREATE TABLE IF NOT EXISTS contacts (
    -- Surrogate primary key for this contact row. AUTOINCREMENT, so an id is
    -- never handed out twice: undoing a contact merge puts a merged contact
    -- back under the id it had (`contact_merge_contacts`), and a contact made
    -- meanwhile must not hold it.
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Display name shown in the UI. Empty until something supplies a name;
    -- a contact with identities and no preferred name is Unknown.
    preferred_name TEXT NOT NULL,
    -- What made this row: 'address_book' (an address book load), 'import',
    -- or 'user'. It becomes 'user' when the person types the name. A load
    -- renames a contact of any origin without changing this, and leaves
    -- contacts its file does not list alone.
    origin TEXT NOT NULL DEFAULT 'user',
    -- When the row was first recorded. Stored and queryable; not displayed.
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- Address-book shape last changed (not message activity).
    last_modified TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS ix_contacts_account_id ON contacts (account_id);

-- One platform identity (phone, email, username, or other) per account.
CREATE TABLE IF NOT EXISTS handles (
    -- Surrogate primary key for this handle row.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Identity string exactly as the backup/source wrote it.
    raw TEXT NOT NULL,
    -- Dedup key: E.164 when unambiguous for phones; otherwise cleaned digits/text.
    -- A phone number carries its country inside this key: one that starts
    -- with + names its calling code, because the number was written with its
    -- + or without it in a country an import run or a person stated. A phone
    -- number written without its + code, in no country anyone stated, is the
    -- digits as typed and matches no + number until a country is picked for
    -- it (#1676).
    normalized TEXT NOT NULL,
    -- Human-readable reason the number needs review; NULL when normalization is trusted.
    normalized_note TEXT,
    -- Shape of the identity: 'phone' | 'email' | 'username' | 'other'.
    handle_type TEXT NOT NULL,
    -- Platform identity: 'text_message' | 'whatsapp' (not per-message SMS/iMessage/RCS).
    service TEXT NOT NULL,
    -- What made this row: 'address_book' (an address book load), 'import',
    -- or 'user'. Whatever made it, an identity taken off its contact is
    -- deleted when nothing else refers to it.
    origin TEXT NOT NULL DEFAULT 'import',
    -- When the identity was first recorded. Queryable; not displayed.
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- When the identity last changed. Queryable; not displayed.
    last_modified TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id, normalized, handle_type, service)
);

CREATE INDEX IF NOT EXISTS ix_handles_account_id ON handles (account_id);
CREATE INDEX IF NOT EXISTS ix_handles_normalized ON handles (account_id, normalized);

-- Links one handle to at most one contact within an account.
CREATE TABLE IF NOT EXISTS contact_handles (
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Linked identity (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- Address-book person that owns this handle (`contacts.id`).
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    -- What made or last moved this link: 'address_book' (an address book
    -- load), 'import', or 'user'.
    origin TEXT NOT NULL DEFAULT 'import',
    PRIMARY KEY (account_id, handle_id)
);

CREATE INDEX IF NOT EXISTS ix_contact_handles_contact_id
    ON contact_handles (contact_id);

-- Named group a user can attach to contacts.
CREATE TABLE IF NOT EXISTS contact_groups (
    -- Surrogate primary key for this group.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Group text unique per account.
    name TEXT NOT NULL,
    -- How the row was born: 'manual' when a person made it, 'import' when the
    -- server created it at the end of an import run. An import group is a
    -- shortcut pointing at that run and the person may delete it; the run's own
    -- record is permanent. Unknown is not stored here — its membership is
    -- computed from contact state.
    kind TEXT NOT NULL DEFAULT 'manual',
    UNIQUE(account_id, name)
);

-- Membership of a contact in a group.
CREATE TABLE IF NOT EXISTS contact_group_members (
    -- Contact in the group (`contacts.id`).
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    -- Group that includes the contact (`contact_groups.id`).
    group_id INTEGER NOT NULL REFERENCES contact_groups(id) ON DELETE CASCADE,
    PRIMARY KEY (contact_id, group_id)
);

-- An import run's Contact Group membership kept by identity while its
-- contact is gone: the holder's import contact, deleted when an identity is
-- linked after the import (#1662). Removing the identity again puts the
-- contact made then back in the group, and the row leaves this table.
CREATE TABLE IF NOT EXISTS contact_group_members_set_aside (
    -- The import run's group (`contact_groups.id`).
    group_id INTEGER NOT NULL REFERENCES contact_groups(id) ON DELETE CASCADE,
    -- An identity the deleted contact held (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, handle_id)
);

-- Soft-delete marker for a conversation; chat rows stay until purge.
CREATE TABLE IF NOT EXISTS trashed_conversations (
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Conversation marked trash (`conversations.id`, no FK so chat can remain).
    conversation_id INTEGER NOT NULL,
    -- When the conversation entered trash (SQLite datetime string).
    trashed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (account_id, conversation_id)
);

-- Soft-delete marker for a contact. Deleting a trashed contact blanks its
-- name and removes this marker; the contact row and its handles stay.
CREATE TABLE IF NOT EXISTS trashed_contacts (
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Contact marked trash (`contacts.id`, no FK so contact can remain).
    contact_id INTEGER NOT NULL,
    -- When the contact entered trash (SQLite datetime string).
    trashed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (account_id, contact_id)
);

-- The account's most recent contact merge, kept so that it can be undone
-- (docs/architecture/rules/contacts-identities-and-messages.md, CONTACT-56).
-- One per account: the next merge drops this row and everything below that
-- names it, and the contacts it merged are gone for good.
CREATE TABLE IF NOT EXISTS contact_merges (
    -- Owning account (`accounts.id`).
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    -- The contact the others were merged into (`contacts.id`). Deleting it
    -- drops the merge, which then cannot be undone.
    target_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    -- When the merge was made (SQLite datetime string).
    merged_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- A contact the merge took: its `contacts` row as it was, deleted from
-- `contacts` by the merge and written back by an undo under the same id.
CREATE TABLE IF NOT EXISTS contact_merge_contacts (
    -- The merge (`contact_merges.account_id`).
    account_id INTEGER NOT NULL REFERENCES contact_merges(account_id) ON DELETE CASCADE,
    -- The id the contact had and gets back (`contacts.id`, no FK: the row is
    -- gone while the merge stands).
    contact_id INTEGER NOT NULL,
    -- `contacts.preferred_name` as it was.
    preferred_name TEXT NOT NULL,
    -- `contacts.origin` as it was.
    origin TEXT NOT NULL,
    -- `contacts.created_at` as it was.
    created_at TEXT NOT NULL,
    -- `contacts.last_modified` as it was.
    last_modified TEXT NOT NULL,
    PRIMARY KEY (account_id, contact_id)
);

-- An identity a merged contact held, which the merge moved to the target.
CREATE TABLE IF NOT EXISTS contact_merge_identities (
    -- The merge (`contact_merges.account_id`).
    account_id INTEGER NOT NULL REFERENCES contact_merges(account_id) ON DELETE CASCADE,
    -- The merged contact that held it (`contact_merge_contacts.contact_id`).
    contact_id INTEGER NOT NULL,
    -- The identity (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- `contact_handles.origin` of the link as it was.
    origin TEXT NOT NULL,
    PRIMARY KEY (account_id, handle_id)
);

-- A Contact Group a merged contact was in, which the merge put the target in.
CREATE TABLE IF NOT EXISTS contact_merge_groups (
    -- The merge (`contact_merges.account_id`).
    account_id INTEGER NOT NULL REFERENCES contact_merges(account_id) ON DELETE CASCADE,
    -- The merged contact that was in it (`contact_merge_contacts.contact_id`).
    contact_id INTEGER NOT NULL,
    -- The Contact Group (`contact_groups.id`).
    group_id INTEGER NOT NULL REFERENCES contact_groups(id) ON DELETE CASCADE,
    -- 1 when the target was in the group before the merge, so an undo leaves
    -- it there.
    target_had INTEGER NOT NULL,
    PRIMARY KEY (account_id, contact_id, group_id)
);

-- An Import Run record (`import_contacts`) of a merged contact, which the
-- merge moved to the target.
CREATE TABLE IF NOT EXISTS contact_merge_imports (
    -- The merge (`contact_merges.account_id`).
    account_id INTEGER NOT NULL REFERENCES contact_merges(account_id) ON DELETE CASCADE,
    -- The merged contact the record was about (`contact_merge_contacts.contact_id`).
    contact_id INTEGER NOT NULL,
    -- The Import Run (`imports.id`).
    import_id INTEGER NOT NULL REFERENCES imports(id) ON DELETE CASCADE,
    -- `import_contacts.reason` the merged contact's record gave.
    reason TEXT NOT NULL,
    -- The target's own `import_contacts.reason` for the run before the
    -- merge, NULL when the run had no record of the target, so an undo puts
    -- the target's record back as it was.
    target_reason TEXT,
    PRIMARY KEY (account_id, contact_id, import_id)
);
