-- What the user had added by hand to someone they then merged into another person:
-- kept together as a card of its own, so "Not the same person" can separate it again.
CREATE TABLE person_own_cards (
    record     TEXT PRIMARY KEY,         -- the id that contact had before the merge
    person_id  TEXT NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    nickname   TEXT,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX person_own_cards_by_person ON person_own_cards (person_id);

-- People merged into someone else, so old links (a page address, a chat mention)
-- still find who they are now.
CREATE TABLE people_merged (
    id        TEXT PRIMARY KEY,          -- the id the merged-away person had
    into_id   TEXT NOT NULL,             -- who they were merged into
    merge_id  TEXT NOT NULL,             -- the merge that did it (people_merges.id)
    merged_at INTEGER NOT NULL
) STRICT;

CREATE INDEX people_merged_by_into ON people_merged (into_id);

-- Recent merges, with what they changed, so the user can undo one exactly.
CREATE TABLE people_merges (
    id         TEXT PRIMARY KEY,
    keep       TEXT NOT NULL,            -- the person everyone was merged into
    snapshot   TEXT NOT NULL,            -- JSON: what to put back
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX people_merges_by_keep ON people_merges (keep, created_at);
