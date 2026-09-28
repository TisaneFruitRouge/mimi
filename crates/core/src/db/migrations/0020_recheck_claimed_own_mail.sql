-- Mail outside the Sent folder whose From line names the user was never checked for
-- instructions aimed at the assistant: anyone can write that From line. Clearing the
-- flag makes the next sync check it (store::backfill_suspicious).
UPDATE mail_messages SET suspicious = NULL WHERE outgoing = 1 AND folder != 'sent';
