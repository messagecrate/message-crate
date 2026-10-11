-- One chat thread per account + chat handle.
CREATE TABLE IF NOT EXISTS conversations (
    -- Surrogate primary key for this conversation.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Thread identity handle (`handles.id`); peer for 1:1, group chat id for groups.
    chat_handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- Thread shape: 'individual' | 'group' (and any other values the importer writes).
    conversation_type TEXT NOT NULL,
    -- Group display title; NULL for 1:1 chats.
    group_title TEXT,
    -- The latest message time of the copy that gave group_title, in the form
    -- messages.timestamp holds; NULL with no title, or when that copy had no
    -- messages. A merge compares an incoming copy's latest message with it
    -- (docs/architecture/contacts-identities-and-messages.md).
    group_title_at TEXT,
    -- Path or name of the source file this thread came from.
    source_file TEXT NOT NULL,
    UNIQUE(account_id, chat_handle_id)
);

CREATE INDEX IF NOT EXISTS ix_conversations_account_id ON conversations (account_id);

-- One other person listed in a conversation. The account holder is never a
-- participant (ADR-0015); a message records which of the holder's addresses it
-- used in `messages.owner_handle_id`. A participant's contact is the one its
-- identity is on (`contact_handles`), so renaming or moving the identity
-- reaches every conversation at once.
CREATE TABLE IF NOT EXISTS participants (
    -- Surrogate primary key for this participant row.
    id INTEGER PRIMARY KEY,
    -- Parent conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Participant identity (`handles.id`). A person the source named with no
    -- address has an identity of type 'other' holding the name.
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- Display name residue from the source for this participant.
    name_alias TEXT,
    -- A participant is one person's seat in one conversation.
    UNIQUE (conversation_id, handle_id)
);

CREATE INDEX IF NOT EXISTS ix_participants_handle_id ON participants (handle_id);

-- A participant set aside because its identity became one of the account's
-- after the import wrote it. The holder is never a participant, so linking
-- the identity moves the row here, and removing the identity again moves it
-- back to `participants` (#1662). An import while the identity is linked
-- writes no participant for it, so the row comes back with the name it had
-- when it was set aside.
CREATE TABLE IF NOT EXISTS participants_set_aside (
    -- Parent conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- The identity, one of the account's (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- `participants.name_alias` as it was.
    name_alias TEXT,
    PRIMARY KEY (conversation_id, handle_id)
);

-- One message in a conversation.
CREATE TABLE IF NOT EXISTS messages (
    -- Surrogate primary key for this message.
    id INTEGER PRIMARY KEY,
    -- Parent conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Owning account (`accounts.id`) denormalized for account-scoped queries.
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Backup/source family that produced this row (for example imessage, whatsapp).
    source TEXT NOT NULL,
    -- Source-native message id; used for exact dedupe with source. The import
    -- refuses a message without one.
    guid TEXT NOT NULL CHECK (guid != ''),
    -- The instant the message was sent, to the millisecond: RFC 3339 in UTC
    -- with three fractional digits and a Z suffix (2015-03-12T18:04:22.250Z;
    -- .000 for a source that records whole seconds). One fixed form, so the
    -- text sorts in time order and lists order by it. Shown, searched and
    -- filed by day and year in the account's time zone (accounts.time_zone).
    timestamp TEXT NOT NULL,
    -- Whether the source recorded timestamp to the millisecond or in whole
    -- seconds, as the conversation file's time_precision says. The flag, never
    -- the time, decides: a millisecond time can end in .000. Within one
    -- source, a 'seconds' message is the duplicate of one that matches it in
    -- everything else with 'milliseconds' in the same second
    -- (docs/architecture/contacts-identities-and-messages.md).
    time_precision TEXT NOT NULL CHECK (time_precision IN ('seconds', 'milliseconds')),
    -- 1 = sent by the account holder; 0 = received from someone else.
    is_from_me INTEGER NOT NULL,
    -- Sender identity (`handles.id`); NULL when unknown.
    sender_handle_id INTEGER REFERENCES handles(id) ON DELETE SET NULL,
    -- The account holder's own address on this message (`handles.id`): the one
    -- it was sent from or received at, from the backup. NULL when the backup
    -- names no owner. Not necessarily one of the account's identities (ADR-0015).
    owner_handle_id INTEGER REFERENCES handles(id) ON DELETE SET NULL,
    -- Per-message transport: sms / imessage / rcs / whatsapp / …
    service TEXT,
    -- Optional subject line (for example MMS subject).
    subject TEXT,
    -- Plain-text body; may be NULL for attachment-only or announcement rows.
    body TEXT,
    -- 1 = system/announcement bubble; 0 = normal user message.
    is_announcement INTEGER NOT NULL DEFAULT 0,
    -- 1 = a reply, whether or not the message it quotes is known; 0 = not a reply.
    is_reply INTEGER NOT NULL DEFAULT 0,
    -- The guid of the message a reply quotes, when that message was in the
    -- same export; NULL otherwise. A message's reply count is the number of
    -- messages whose reply_to_guid is its guid, counted when read.
    reply_to_guid TEXT,
    -- The part of the quoted message a reply answers; NULL when not recorded.
    reply_to_part INTEGER,
    -- 'deleted_in_source_app': the person deleted it in the app it came from
    -- before the backup; 'unsent': its sender took it back. NULL for neither.
    -- The message is listed like any other either way. An Unsent message
    -- shows none of its text, so a word of it does not find the message
    -- (fts_triggers_create.sql); a Deleted one is found by its text.
    deletion TEXT CHECK (deletion IN ('deleted_in_source_app', 'unsent')),
    -- Stable order within the conversation when timestamps collide.
    sort_order INTEGER NOT NULL,
    -- Hash fingerprint for cross-source duplicate detection (chat/direction/time/body/attachments).
    content_key TEXT,
    -- Points at the kept message when this row is a flagged duplicate.
    duplicate_of INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    -- Import run that inserted this row (`imports.id`).
    import_id INTEGER REFERENCES imports(id) ON DELETE SET NULL,
    -- When the newest backup with a date that gave the message anything was
    -- made, in the form timestamp holds; NULL when no copy's file said. A
    -- later import's copy from a later backup replaces its mark and text;
    -- one from an earlier backup changes neither
    -- (docs/architecture/contacts-identities-and-messages.md).
    backup_taken_at TEXT,
    -- The mark a copy from a file without a backup date gave the message,
    -- as `deletion`; NULL for none. It outlasts every later copy without a
    -- mark, dated or not, because a file without a date does not say when
    -- the message was deleted or unsent, until a dated backup at least as
    -- new as `backup_taken_at` gives the same mark (#1989).
    undated_deletion TEXT CHECK (undated_deletion IN ('deleted_in_source_app', 'unsent')),
    -- 1 when the text and earlier versions came from a copy from a file
    -- without a backup date, so a later copy gives its own only when it
    -- records a later edit, whatever its backup's date, until a dated
    -- backup at least as new as `backup_taken_at` gives the same text and
    -- earlier versions (#1989); 0 otherwise.
    undated_body INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS ix_messages_conversation_timestamp
    ON messages (conversation_id, timestamp);
CREATE INDEX IF NOT EXISTS ix_messages_conversation_source_timestamp
    ON messages (conversation_id, source, timestamp);
CREATE INDEX IF NOT EXISTS ix_messages_account_id ON messages (account_id);
-- `GET /v1/messages` pages an account's messages by time; this serves the
-- default sort without a scan.
CREATE INDEX IF NOT EXISTS ix_messages_account_timestamp
    ON messages (account_id, timestamp, id);
-- `GET /v1/contacts` computes when the account last heard from each contact:
-- the newest message any of the contact's handles sent. This answers that
-- MAX(timestamp) per sender without a scan.
CREATE INDEX IF NOT EXISTS ix_messages_sender_timestamp
    ON messages (sender_handle_id, timestamp);
CREATE UNIQUE INDEX IF NOT EXISTS ix_messages_account_source_guid
    ON messages (account_id, source, guid);
CREATE INDEX IF NOT EXISTS ix_messages_content_key
    ON messages (content_key)
    WHERE content_key IS NOT NULL AND content_key != '';
CREATE INDEX IF NOT EXISTS ix_messages_duplicate_of
    ON messages (duplicate_of)
    WHERE duplicate_of IS NOT NULL;
-- An account identity's counts read the messages held at it (ADR-0015).
CREATE INDEX IF NOT EXISTS ix_messages_owner_handle_id ON messages (owner_handle_id);
CREATE INDEX IF NOT EXISTS ix_messages_import_id
    ON messages (import_id)
    WHERE import_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_messages_source ON messages (source);
-- A message's reply count is read by counting the replies that name it, the
-- same account, source and guid that ix_messages_account_source_guid keys it
-- by; this answers that count without a scan.
CREATE INDEX IF NOT EXISTS ix_messages_reply_to
    ON messages (account_id, source, reply_to_guid)
    WHERE reply_to_guid IS NOT NULL;

-- File or media part attached to a message.
CREATE TABLE IF NOT EXISTS attachments (
    -- Surrogate primary key for this attachment.
    id INTEGER PRIMARY KEY,
    -- Parent message (`messages.id`).
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Relative path from the export/staging layout when known.
    path TEXT,
    -- Original filename from the source.
    original_name TEXT,
    -- MIME type of the original bytes when known.
    mime_type TEXT,
    -- 1 = sticker; 0 = ordinary attachment.
    is_sticker INTEGER NOT NULL DEFAULT 0,
    -- Optional speech-to-text or OCR text for search.
    transcription TEXT,
    -- SHA-256 hex of the stored original bytes when present.
    sha256 TEXT,
    -- Path under the assets store for the original file.
    assets_path TEXT,
    -- Original file size in bytes when known.
    size_bytes INTEGER,
    -- Why bytes are absent (for example not_exported, decrypt_failed); NULL if present.
    missing_reason TEXT,
    -- SHA-256 hex of a converted/compressed derivative used by the browser.
    derived_sha256 TEXT,
    -- Path under the assets store for the derivative file.
    derived_assets_path TEXT,
    -- MIME type of the derivative file.
    derived_mime_type TEXT,
    -- Why the server's media pass could not make the Preview the last time
    -- it tried: what ffmpeg said about the file, with the paths it named
    -- made relative to the account's originals directory and the pass's
    -- work directory, or a phrase of the server's when ffmpeg did not read
    -- the file (`docs/architecture/media.md` rule 4). Written only on rows
    -- that name no Preview. NULL once the Preview is made, once the original
    -- is decided to be shown as it is, and until a try has failed.
    preview_not_made_reason TEXT,
    -- 1 when every browser shows the stored original as it is, so a viewer
    -- opens it and it gets no Preview (`media::browser_shows`,
    -- `docs/architecture/media.md` rule 2). The server's media pass decides
    -- it, and only an MP4 is opened to read its codec. 0 when it is not
    -- shown as it is, and until the pass has looked at the file.
    shown_as_is INTEGER NOT NULL DEFAULT 0 CHECK (shown_as_is IN (0, 1)),
    -- SHA-256 hex of the Thumbnail: a small JPEG of an image, or a video's
    -- first frame. NULL until the server has made it.
    thumbnail_sha256 TEXT,
    -- Path of the Thumbnail under the account's converted-media directory.
    thumbnail_assets_path TEXT,
    -- MIME type of the Thumbnail file.
    thumbnail_mime_type TEXT,
    -- Why the media pass could not make the Thumbnail the last time it
    -- tried, as `preview_not_made_reason` says for the Preview.
    thumbnail_not_made_reason TEXT,
    -- Import Run that last wrote this row: the one that added it, or that
    -- gave a row stored without its file the file (`imports.id`). When the
    -- run ends, the server queues the Assets of the rows it wrote, whichever
    -- run created their message (#1946).
    import_id INTEGER REFERENCES imports(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS ix_attachments_sha256 ON attachments (sha256);
CREATE INDEX IF NOT EXISTS ix_attachments_import_id
    ON attachments (import_id)
    WHERE import_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_attachments_message_id ON attachments (message_id);

-- Assets whose Thumbnail and Preview the server still has to make. An Import
-- Run that ends adds the Assets its messages name, and the server works
-- through the rows in the background, removing each when it is done, so the
-- rows a stopped server leaves are worked on when it starts again.
CREATE TABLE IF NOT EXISTS media_queue (
    -- Account whose Asset this is (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- SHA-256 hex of the original Asset.
    sha256 TEXT NOT NULL,
    -- When the Asset was queued, in Unix seconds.
    queued_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, sha256)
);

-- Reaction (tapback) on a message part.
CREATE TABLE IF NOT EXISTS tapbacks (
    -- Surrogate primary key for this reaction.
    id INTEGER PRIMARY KEY,
    -- Parent message (`messages.id`).
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Which part of a multi-part message this reaction targets (0 = first/default).
    part_index INTEGER NOT NULL DEFAULT 0,
    -- Reaction kind from the source (for example loved, liked, emphasized).
    kind TEXT NOT NULL,
    -- Emoji glyph when the reaction is custom/emoji-based.
    emoji TEXT,
    -- 1 = reaction from the account holder; 0 = from someone else.
    is_from_me INTEGER NOT NULL,
    -- Reactor identity (`handles.id`); NULL when unknown.
    sender_handle_id INTEGER REFERENCES handles(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS ix_tapbacks_message_id ON tapbacks (message_id);

-- One earlier version of one part of an edited message: the text that part
-- held before an edit replaced it, or only that an edit replaced it.
-- messages.body is the final version; these are the versions before it.
-- Search finds a message by any of them that has text (message_versions_fts
-- in fts_virtual.sql).
CREATE TABLE IF NOT EXISTS message_versions (
    -- Surrogate primary key; also the version's rowid in message_versions_fts.
    -- Ascending within a message in the order the source listed its versions,
    -- oldest first within each part.
    id INTEGER PRIMARY KEY,
    -- The edited message (`messages.id`).
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Which part of a multi-part message this version belongs to (0 = first/only).
    part_index INTEGER NOT NULL DEFAULT 0,
    -- The part's text in this version. NULL when the source recorded the edit
    -- and not the text it replaced, as iMazing does; such a version has no row
    -- in message_versions_fts.
    text TEXT,
    -- When this version was written, to the millisecond in the form
    -- messages.timestamp holds: the send time for the original, the edit's time for
    -- a later one. NULL when the source does not record it.
    edited_at TEXT
);

CREATE INDEX IF NOT EXISTS ix_message_versions_message_id ON message_versions (message_id);

-- Named tag a user can stamp on whole conversations.
CREATE TABLE IF NOT EXISTS message_tags (
    -- Surrogate primary key for this tag.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Tag text unique per account.
    name TEXT NOT NULL,
    UNIQUE(account_id, name)
);

-- Membership of a conversation in a message tag.
CREATE TABLE IF NOT EXISTS message_tag_members (
    -- Tagged conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Tag that includes the conversation (`message_tags.id`).
    tag_id INTEGER NOT NULL REFERENCES message_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (conversation_id, tag_id)
);

-- One message a running Export Run matched when it was created. The run's
-- pages read these rows rather than its scope, so an import, a trash, or a
-- new day between pages cannot move what the run hands over. The rows are
-- deleted when the run completes or is cancelled.
CREATE TABLE IF NOT EXISTS export_messages (
    -- Export Run (`exports.id`).
    export_id INTEGER NOT NULL REFERENCES exports(id) ON DELETE CASCADE,
    -- The message's place in the run, from 1: oldest first by timestamp,
    -- then sort_order, then id, as they stood at creation.
    row_order INTEGER NOT NULL,
    -- Matched message (`messages.id`). NULL once the message is deleted: its
    -- place stays, empty, so the places after it do not move.
    message_id INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    PRIMARY KEY (export_id, row_order)
);

CREATE INDEX IF NOT EXISTS ix_export_messages_message
    ON export_messages (message_id);
