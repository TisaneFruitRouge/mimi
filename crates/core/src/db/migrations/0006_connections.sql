CREATE TABLE connections (
    id          TEXT PRIMARY KEY,
    integration TEXT NOT NULL,
    name        TEXT NOT NULL,
    config      TEXT NOT NULL, -- JSON, may hold secrets (the database is encrypted)
    created_at  INTEGER NOT NULL
) STRICT;
