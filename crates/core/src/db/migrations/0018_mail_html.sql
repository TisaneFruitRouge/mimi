-- The HTML version of a message, for showing it as it was sent (`mail::render`):
-- already made safe (no scripts, forms, hidden text or remote content other than image
-- addresses) and with its inline pictures included. '' when the message has none;
-- NULL until known (mail stored before this column existed, or too large to copy),
-- then filled in from the server the first time the message is shown.
-- Only for display: the assistant always reads the plain-text `body`.
ALTER TABLE mail_messages ADD COLUMN html TEXT;
