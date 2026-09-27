-- Which of the user's addresses an incoming message arrived at (an alias, a catch-all
-- address, a +tag, or the account's own). NULL for sent mail, and for mail stored
-- before this column existed until the sync fills it in from the recipients.
ALTER TABLE mail_messages ADD COLUMN received_on TEXT;

CREATE INDEX mail_messages_received_on ON mail_messages (connection_id, received_on);
