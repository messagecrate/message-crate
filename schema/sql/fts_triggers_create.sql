-- Keep messages_fts in step with messages and attachments. One index row
-- holds a message's body, subject, and the names and transcriptions of all
-- its attachments, so every change removes the message's row and writes it
-- again whole. messages_fts is a contentless-delete table (fts_virtual.sql):
-- DELETE by rowid removes every term the row indexed, attachment terms
-- included, without being told what they were.
--
-- An Unsent message has no index row. It reads "Unsent" and nothing else,
-- and a search finds a message by what it shows, so a word of the text or
-- file names it hides must not find it (#1758). Marking a message Unsent
-- removes its row, and taking the mark off writes it again.
CREATE TRIGGER messages_fts_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        new.id,
        coalesce(new.body, ''),
        coalesce(new.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(original_name, '') || ' ' || coalesce(transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments
            WHERE message_id = new.id
        )
    WHERE new.deletion IS NOT 'unsent';
END;

CREATE TRIGGER messages_fts_ad AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;

CREATE TRIGGER messages_fts_au AFTER UPDATE OF body, subject, deletion ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        new.id,
        coalesce(new.body, ''),
        coalesce(new.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(original_name, '') || ' ' || coalesce(transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments
            WHERE message_id = new.id
        )
    WHERE new.deletion IS NOT 'unsent';
END;

CREATE TRIGGER attachments_fts_ai AFTER INSERT ON attachments BEGIN
    DELETE FROM messages_fts WHERE rowid = new.message_id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        m.id,
        coalesce(m.body, ''),
        coalesce(m.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments a
            WHERE a.message_id = m.id
        )
    FROM messages m WHERE m.id = new.message_id AND m.deletion IS NOT 'unsent';
END;

CREATE TRIGGER attachments_fts_ad AFTER DELETE ON attachments BEGIN
    DELETE FROM messages_fts WHERE rowid = old.message_id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        m.id,
        coalesce(m.body, ''),
        coalesce(m.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments a
            WHERE a.message_id = m.id
        )
    FROM messages m WHERE m.id = old.message_id AND m.deletion IS NOT 'unsent';
END;

CREATE TRIGGER attachments_fts_au AFTER UPDATE OF original_name, transcription ON attachments BEGIN
    DELETE FROM messages_fts WHERE rowid = new.message_id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        m.id,
        coalesce(m.body, ''),
        coalesce(m.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments a
            WHERE a.message_id = m.id
        )
    FROM messages m WHERE m.id = new.message_id AND m.deletion IS NOT 'unsent';
END;

-- Keep message_versions_fts in step with message_versions: one index row per
-- version that has text, under the version's id. A version with no text
-- (one iMazing records) has nothing to find it by, so it gets no row.
-- Deleting a message deletes its versions through ON DELETE CASCADE, which
-- fires the delete trigger for each.
CREATE TRIGGER message_versions_fts_ai AFTER INSERT ON message_versions BEGIN
    INSERT INTO message_versions_fts(rowid, text)
    SELECT new.id, new.text WHERE new.text IS NOT NULL;
END;

CREATE TRIGGER message_versions_fts_ad AFTER DELETE ON message_versions BEGIN
    DELETE FROM message_versions_fts WHERE rowid = old.id;
END;

CREATE TRIGGER message_versions_fts_au AFTER UPDATE OF text ON message_versions BEGIN
    DELETE FROM message_versions_fts WHERE rowid = old.id;
    INSERT INTO message_versions_fts(rowid, text)
    SELECT new.id, new.text WHERE new.text IS NOT NULL;
END;
