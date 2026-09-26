-- Browser sessions for the web interface. Only a SHA-256 of each session token is kept,
-- so a copy of the database can't be used to log in.
CREATE TABLE web_sessions (
    token_hash TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
