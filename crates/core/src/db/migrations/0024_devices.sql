-- Phones paired with this computer (Settings › Phone). A phone is known by its iroh
-- endpoint id (its public key, proven by every connection) and a token of its own; only
-- the token's SHA-256 is kept.
CREATE TABLE devices (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    endpoint_id  TEXT NOT NULL UNIQUE,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER
) STRICT;
