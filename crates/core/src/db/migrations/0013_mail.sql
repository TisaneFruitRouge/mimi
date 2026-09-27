-- Email: a local copy of recent mail (threads and messages), searchable offline.

CREATE TABLE mail_threads (
    id            INTEGER PRIMARY KEY,
    connection_id TEXT NOT NULL,
    subject       TEXT NOT NULL,
    last_at       INTEGER NOT NULL,
    -- needs_reply | important | other; NULL until sorted.
    category      TEXT,
    summary       TEXT,
    -- The thread's last_at when it was sorted; a newer message means sort again.
    sorted_at     INTEGER,
    automated     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX mail_threads_recent ON mail_threads (connection_id, last_at DESC);

CREATE TABLE mail_messages (
    id            INTEGER PRIMARY KEY,
    connection_id TEXT NOT NULL,
    -- The server's mailbox name, and what it's for: inbox | sent | archive.
    mailbox       TEXT NOT NULL,
    folder        TEXT NOT NULL,
    uid           INTEGER NOT NULL,
    thread_id     INTEGER NOT NULL REFERENCES mail_threads (id) ON DELETE CASCADE,
    message_id    TEXT,
    in_reply_to   TEXT,
    refs          TEXT NOT NULL DEFAULT '',
    from_name     TEXT,
    from_email    TEXT NOT NULL,
    to_json       TEXT NOT NULL,
    cc_json       TEXT NOT NULL,
    subject       TEXT NOT NULL,
    date          INTEGER NOT NULL,
    body          TEXT NOT NULL,
    snippet       TEXT NOT NULL,
    attachments   TEXT NOT NULL DEFAULT '[]',
    seen          INTEGER NOT NULL,
    flagged       INTEGER NOT NULL,
    outgoing      INTEGER NOT NULL,
    automated     INTEGER NOT NULL,
    UNIQUE (connection_id, mailbox, uid)
) STRICT;

CREATE INDEX mail_messages_thread ON mail_messages (thread_id, date);
CREATE INDEX mail_messages_mid ON mail_messages (connection_id, message_id);
CREATE INDEX mail_messages_from ON mail_messages (from_email);

CREATE VIRTUAL TABLE mail_fts USING fts5 (
    subject, from_name, from_email, body,
    content = 'mail_messages', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER mail_messages_ai AFTER INSERT ON mail_messages BEGIN
    INSERT INTO mail_fts (rowid, subject, from_name, from_email, body)
    VALUES (new.id, new.subject, new.from_name, new.from_email, new.body);
END;

CREATE TRIGGER mail_messages_ad AFTER DELETE ON mail_messages BEGIN
    INSERT INTO mail_fts (mail_fts, rowid, subject, from_name, from_email, body)
    VALUES ('delete', old.id, old.subject, old.from_name, old.from_email, old.body);
END;

-- Where each mailbox's sync stands. A new UIDVALIDITY means the server renumbered it.
CREATE TABLE mail_sync (
    connection_id TEXT NOT NULL,
    mailbox       TEXT NOT NULL,
    uidvalidity   INTEGER NOT NULL,
    last_uid      INTEGER NOT NULL,
    synced_at     INTEGER NOT NULL,
    PRIMARY KEY (connection_id, mailbox)
) STRICT;
