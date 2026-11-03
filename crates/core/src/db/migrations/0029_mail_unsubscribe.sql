-- Unsubscribing from newsletters and mailing lists (`mail::unsubscribe`).
--
-- A message's List-Unsubscribe and List-Unsubscribe-Post (RFC 8058) headers, unfolded,
-- and its List-Id (the bare id, lowercased). '' when the message has none; NULL for
-- mail stored before these columns existed, read from the server the first time an
-- automatic conversation is opened.
ALTER TABLE mail_messages ADD COLUMN list_unsubscribe TEXT;
ALTER TABLE mail_messages ADD COLUMN list_unsubscribe_post TEXT;
ALTER TABLE mail_messages ADD COLUMN list_id TEXT;

-- Lists the user unsubscribed from, by account. `list` is 'id:' and the List-Id, or
-- 'from:' and the sender's address for mail without one.
-- method: one_click | email | website
CREATE TABLE mail_unsubscribed (
    connection_id TEXT NOT NULL,
    list          TEXT NOT NULL,
    method        TEXT NOT NULL,
    at            INTEGER NOT NULL,
    PRIMARY KEY (connection_id, list)
) STRICT;
