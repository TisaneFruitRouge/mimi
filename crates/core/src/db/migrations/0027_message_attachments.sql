-- Photos the user sends with a message, kept in this (encrypted) database rather than
-- as loose files. Only pictures for now (`kind`); they're stored as normalised: decoded,
-- shrunk, without metadata, saved again as JPEG or PNG. Deleting a message (and so a
-- conversation) deletes its attachments.
CREATE TABLE message_attachments (
    id         TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    name       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    width      INTEGER,
    height     INTEGER,
    data       BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX message_attachments_by_message ON message_attachments (message_id, position);

-- The model the message went to couldn't see its photos (it was only told about them).
ALTER TABLE messages ADD COLUMN attachments_unseen INTEGER NOT NULL DEFAULT 0;
