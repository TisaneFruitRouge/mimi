-- Finding memories by meaning: one embedding per library note, made by a small local
-- model. `hash` covers the text that was embedded, so a changed note is re-embedded;
-- `model` says which model made it, so switching models re-embeds everything.
CREATE TABLE memory_vectors (
    path       TEXT PRIMARY KEY REFERENCES memory_notes (path) ON DELETE CASCADE,
    model      TEXT NOT NULL,
    hash       TEXT NOT NULL,
    vector     BLOB NOT NULL, -- little-endian f32s
    updated_at INTEGER NOT NULL
) STRICT;

-- Notes about a person in the people directory (`subject` is their id).
CREATE INDEX memory_notes_subject ON memory_notes (subject) WHERE subject IS NOT NULL;
