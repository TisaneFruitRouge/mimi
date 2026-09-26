CREATE TABLE conversations (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    reasoning       TEXT NOT NULL,
    status          TEXT NOT NULL,
    provider_id     TEXT,
    model           TEXT,
    locality        TEXT,
    error           TEXT,
    created_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX messages_by_conversation ON messages (conversation_id, created_at);
CREATE INDEX conversations_by_activity ON conversations (updated_at DESC);
