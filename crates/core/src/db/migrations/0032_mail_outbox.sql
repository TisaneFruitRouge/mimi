-- Mail waiting to go (`mail::outbox`): the seconds Undo is offered after Send, and
-- messages scheduled for later. Kept here, in the encrypted database, so closing the
-- window or restarting doesn't lose or double-send them. A row leaves once its message
-- is sent or the user takes it back.
--
-- kind:   undo | scheduled
-- status: waiting | sending | failed
-- draft:  the MailDraft as the user wrote it (JSON), restored on Undo or Cancel.
-- files:  the files it carries by reference (a forward's originals, the assistant's
--         email and chat files), fetched when it was queued (JSON list of
--         NewMailAttachment with their content), so what goes is what was checked.
-- connection_id / from_address: the account and address it goes from, fixed when queued.
-- error:  what went wrong, in plain words; with status waiting, a try that couldn't
--         reach the server (attempts counts them) and send_at is the next one.
CREATE TABLE mail_outbox (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,
    status        TEXT NOT NULL,
    draft         TEXT NOT NULL,
    files         TEXT NOT NULL DEFAULT '{}',
    connection_id TEXT NOT NULL,
    from_address  TEXT NOT NULL,
    send_at       INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    error         TEXT,
    attempts      INTEGER NOT NULL DEFAULT 0,
    by_assistant  INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX mail_outbox_due ON mail_outbox (status, send_at);
