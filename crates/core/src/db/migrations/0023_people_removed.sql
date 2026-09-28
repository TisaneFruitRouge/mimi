-- People the user deleted from Mimi. Their address books and mail accounts are never
-- changed: the cards are remembered here so syncs skip them, and the person can be
-- brought back (with the same id).
CREATE TABLE people_removed (
    id          TEXT PRIMARY KEY,        -- the person's id, given back on restore
    name        TEXT NOT NULL,
    nickname    TEXT,
    name_locked INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    removed_at  INTEGER NOT NULL
) STRICT;

-- The cards of removed people, by the ids syncs know them by. A restored person's
-- cards stay here, marked `restoring`, until the next sync brings them back to it.
CREATE TABLE person_records_removed (
    source     TEXT NOT NULL,            -- connection id
    record     TEXT NOT NULL,            -- the card's id within the source
    person_id  TEXT NOT NULL,            -- who the card belonged to
    name       TEXT NOT NULL,            -- the name on the card
    restoring  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source, record)
) STRICT;

CREATE INDEX person_records_removed_by_person ON person_records_removed (person_id);
