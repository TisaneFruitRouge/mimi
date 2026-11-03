-- New mail waiting to be announced on the desktop (`mail::notify`). The sync adds a row
-- when it stores a new Inbox message (never on an account's first pass); the notifier
-- removes it once announced or passed over, so nothing is announced twice.
CREATE TABLE mail_notify_queue (
    message_id INTEGER PRIMARY KEY REFERENCES mail_messages (id) ON DELETE CASCADE,
    queued_at  INTEGER NOT NULL
) STRICT;

-- Messages (by Message-ID) already considered for a notification, so a copy that comes
-- back with a new UID (moved out of the Inbox and back), or the same message reaching a
-- second account, isn't announced again. Kept a week.
CREATE TABLE mail_notify_seen (
    message_id TEXT PRIMARY KEY,
    at         INTEGER NOT NULL
) STRICT;
