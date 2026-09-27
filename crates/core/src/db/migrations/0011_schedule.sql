-- Reminders and routines. `schedule` is the rule as JSON (see mimi_protocol::Schedule);
-- `next_at` is derived from it and recomputed whenever the rule, the clock or the time
-- zone changes.
CREATE TABLE schedule_items (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,
    title           TEXT NOT NULL,
    instruction     TEXT,
    schedule        TEXT NOT NULL,
    paused          INTEGER NOT NULL DEFAULT 0,
    next_at         INTEGER,
    snoozed_until   INTEGER,
    last_at         INTEGER,
    ended           TEXT,
    event_start     INTEGER,  -- for event-relative items: the event's start as last seen
    conversation_id TEXT,     -- routines: where results go
    created_in      TEXT,     -- the conversation it was set up from, if any
    anchor_at       INTEGER NOT NULL, -- start of counting for interval schedules
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX schedule_items_due ON schedule_items (next_at);

CREATE TABLE schedule_deliveries (
    id              TEXT PRIMARY KEY,
    item_id         TEXT NOT NULL REFERENCES schedule_items (id) ON DELETE CASCADE,
    due_at          INTEGER NOT NULL,
    at              INTEGER NOT NULL,
    status          TEXT NOT NULL,
    detail          TEXT,
    conversation_id TEXT
) STRICT;

CREATE INDEX schedule_deliveries_recent ON schedule_deliveries (at DESC);

-- Undo for changes the assistant made: the item as it was before (NULL = didn't exist).
CREATE TABLE schedule_revisions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id         TEXT NOT NULL,
    before          TEXT,
    conversation_id TEXT,
    undone          INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL
) STRICT;
