-- Invitation emails Mimi offered to send after an event with guests was saved, changed or
-- removed. Calendars email nobody; these go out through the user's own email only when
-- they say so. Sent rows stay: `sent_sequence` keeps each later message for the same
-- event (UID and occurrence) newer than what the guests already have.
CREATE TABLE calendar_invitations (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,            -- invite | update | cancel
    calendar_id   TEXT NOT NULL,
    calendar      TEXT NOT NULL,            -- the calendar's name (for @-mention ids)
    event_uid     TEXT NOT NULL,            -- how Mimi finds the event again
    event_start   INTEGER NOT NULL,         -- the occurrence's start, ms
    ical_uid      TEXT NOT NULL,            -- the UID other calendars know it by
    occurrence    INTEGER,                  -- RECURRENCE-ID (ms) for one occurrence
    snapshot      TEXT NOT NULL,            -- the event as it was, JSON
    recipients    TEXT NOT NULL,            -- JSON list of guests
    created_at    INTEGER NOT NULL,
    sent_at       INTEGER,
    sent_sequence INTEGER,
    sent_from     TEXT
) STRICT;

CREATE INDEX calendar_invitations_start ON calendar_invitations (event_start);
CREATE INDEX calendar_invitations_uid ON calendar_invitations (ical_uid, occurrence);
