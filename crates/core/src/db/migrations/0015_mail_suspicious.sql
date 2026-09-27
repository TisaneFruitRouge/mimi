-- Whether an incoming message contains instructions aimed at an AI assistant
-- (`mail::suspicious`). NULL for mail stored before this column existed, until the
-- sync checks it.
ALTER TABLE mail_messages ADD COLUMN suspicious INTEGER;
