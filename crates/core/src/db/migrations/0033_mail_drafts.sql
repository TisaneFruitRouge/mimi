-- Drafts (`mail::drafts`): emails being written, saved as the user writes so nothing is
-- lost, and drafts found in an account's Drafts folder on the server (written in another
-- mail app). Each is copied to the server's Drafts folder, replacing the previous copy.
--
-- draft:      the MailDraft as JSON, with its HTML, pictures and files (their content).
--             A draft from elsewhere keeps no file content here (`partial`): it's read
--             from the server when the draft is opened.
-- subject, recipients (JSON list), snippet, reply_to, files: what the list shows,
--             without reading `draft`.
-- origin:     here | server (found on the server, not changed here since).
-- rev:        changes saved here; mirrored_rev: the one whose copy is on the server.
-- dirty_since: when the first change not yet on the server was saved; asap: copy it
--             now (the editor closed); retry_at: the server couldn't be reached.
-- server_conn, server_mailbox, server_validity, server_uid: where its copy on the
--             server is (Mimi's last copy, or the other app's), to replace or delete.
-- removed:    NULL, or 'deleted' / 'sent': gone from Drafts. Kept until the server copy
--             is deleted (and a little longer: a late save of a sent draft is ignored).
CREATE TABLE mail_drafts (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT,
    draft           TEXT NOT NULL DEFAULT '{}',
    subject         TEXT NOT NULL DEFAULT '',
    recipients      TEXT NOT NULL DEFAULT '[]',
    snippet         TEXT NOT NULL DEFAULT '',
    reply_to        INTEGER,
    files           INTEGER NOT NULL DEFAULT 0,
    origin          TEXT NOT NULL DEFAULT 'here',
    partial         INTEGER NOT NULL DEFAULT 0,
    rev             INTEGER NOT NULL DEFAULT 0,
    mirrored_rev    INTEGER NOT NULL DEFAULT 0,
    dirty_since     INTEGER,
    asap            INTEGER NOT NULL DEFAULT 0,
    retry_at        INTEGER,
    server_conn     TEXT,
    server_mailbox  TEXT,
    server_validity INTEGER,
    server_uid      INTEGER,
    removed         TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX mail_drafts_server ON mail_drafts (server_conn, server_mailbox);
CREATE INDEX mail_drafts_reply ON mail_drafts (reply_to);
