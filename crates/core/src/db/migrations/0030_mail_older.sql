-- Older mail found on the server by a search (`mail::older`): kept until this moment
-- (ms), then forgotten. NULL for mail copied by sync, which ages out of its window as
-- usual. Kept mail is left out of the mailboxes, counts, sorting, smart folders and the
-- flag sweep's UID range; search and opening it work as for any conversation.
ALTER TABLE mail_messages ADD COLUMN kept_until INTEGER;
CREATE INDEX mail_messages_kept ON mail_messages (kept_until) WHERE kept_until IS NOT NULL;
