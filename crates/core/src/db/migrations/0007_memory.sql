-- The memory library: short markdown notes addressed by path (people/sam.md…).
-- `profile.md` is the always-loaded summary about the user.
CREATE TABLE memory_notes (
    path       TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    subject    TEXT,          -- optional link to what the note is about (e.g. a contact id)
    source     TEXT NOT NULL, -- you | assistant | learned
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- Full-text index over the notes, kept in step by triggers.
CREATE VIRTUAL TABLE memory_fts USING fts5(
    path, title, body,
    content = 'memory_notes',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER memory_notes_ai AFTER INSERT ON memory_notes BEGIN
    INSERT INTO memory_fts (rowid, path, title, body)
    VALUES (new.rowid, new.path, new.title, new.body);
END;

CREATE TRIGGER memory_notes_ad AFTER DELETE ON memory_notes BEGIN
    INSERT INTO memory_fts (memory_fts, rowid, path, title, body)
    VALUES ('delete', old.rowid, old.path, old.title, old.body);
END;

CREATE TRIGGER memory_notes_au AFTER UPDATE ON memory_notes BEGIN
    INSERT INTO memory_fts (memory_fts, rowid, path, title, body)
    VALUES ('delete', old.rowid, old.path, old.title, old.body);
    INSERT INTO memory_fts (rowid, path, title, body)
    VALUES (new.rowid, new.path, new.title, new.body);
END;

-- What a note looked like before each change, so any change can be undone.
CREATE TABLE memory_revisions (
    id              INTEGER PRIMARY KEY,
    path            TEXT NOT NULL,
    before_title    TEXT,     -- NULL: the note didn't exist
    before_body     TEXT,
    before_source   TEXT,
    conversation_id TEXT,     -- where the change was made, if in a conversation
    undone          INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL
) STRICT;

-- How far each conversation has been read by the background learning pass.
CREATE TABLE memory_learned (
    conversation_id TEXT PRIMARY KEY,
    until_ms        INTEGER NOT NULL
) STRICT;
