-- People the user can mention and reach, unified across sources.
CREATE TABLE people (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    nickname    TEXT,
    -- The user renamed this person: syncs no longer change the name.
    name_locked INTEGER NOT NULL DEFAULT 0,
    -- Added by the user (not imported): kept even with no cards.
    manual      INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

-- One contact card from one source (e.g. a vCard in an iCloud address book),
-- attached to the person it was unified into.
CREATE TABLE person_records (
    source     TEXT NOT NULL,            -- connection id
    record     TEXT NOT NULL,            -- the card's id within the source
    person_id  TEXT NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    PRIMARY KEY (source, record)
) STRICT;

-- Ways to reach a person. Imported handles belong to a record; manual ones don't.
CREATE TABLE person_handles (
    id         TEXT PRIMARY KEY,
    person_id  TEXT NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    channel    TEXT NOT NULL,
    value      TEXT NOT NULL,
    match_key  TEXT,                     -- normalized identity for unification, if any
    label      TEXT,
    source     TEXT,                     -- NULL: added by the user
    record     TEXT,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX person_handles_by_person ON person_handles (person_id);
CREATE INDEX person_handles_by_key ON person_handles (match_key);
CREATE INDEX person_handles_by_record ON person_handles (source, record);

-- Pairs the user said are different people, so they stop being suggested.
CREATE TABLE people_apart (
    a TEXT NOT NULL,
    b TEXT NOT NULL,
    PRIMARY KEY (a, b)
) STRICT;

-- @ mentions on user messages, and what the model was told about them.
ALTER TABLE messages ADD COLUMN mentions TEXT NOT NULL DEFAULT '[]';
ALTER TABLE messages ADD COLUMN mention_context TEXT;
