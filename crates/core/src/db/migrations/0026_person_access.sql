-- People the user lets ask the assistant things from their own messaging app
-- (access/): whether they may, the calendars shared with them (a JSON list of calendar
-- ids), and who approves what they ask for. Off for everyone until the user says so.
CREATE TABLE person_access (
    person_id  TEXT PRIMARY KEY REFERENCES people (id) ON DELETE CASCADE,
    enabled    INTEGER NOT NULL DEFAULT 0,
    calendars  TEXT NOT NULL DEFAULT '[]',
    approver   TEXT NOT NULL DEFAULT 'guest' CHECK (approver IN ('guest', 'owner')),
    updated_at INTEGER NOT NULL
) STRICT;

-- A trusted person's own conversations. They belong to that person: the owner's clients
-- never list or receive them. app/connection_id/address/chat: the line it's on (their
-- Matrix address and the room); NULL for a routine's conversation. No foreign keys on
-- the person or the connection: a row only ever goes with its conversation, so a
-- conversation can never lose the mark that hides it.
CREATE TABLE guest_conversations (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations (id) ON DELETE CASCADE,
    person_id       TEXT NOT NULL,
    app             TEXT,
    connection_id   TEXT,
    address         TEXT,
    chat            TEXT,
    created_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX guest_conversations_by_person ON guest_conversations (person_id);
CREATE UNIQUE INDEX guest_conversations_by_line
    ON guest_conversations (person_id, app, connection_id, chat) WHERE app IS NOT NULL;

-- Reminders and routines set up for a trusted person: delivered to them, not the owner.
ALTER TABLE schedule_items ADD COLUMN for_person TEXT;
