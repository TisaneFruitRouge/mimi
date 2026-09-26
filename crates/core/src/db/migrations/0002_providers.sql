CREATE TABLE providers (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    base_url   TEXT NOT NULL,
    locality   TEXT NOT NULL,
    api_key    TEXT,
    created_at INTEGER NOT NULL
) STRICT;
