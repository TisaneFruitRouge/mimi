-- Conversation numbers are never reused, so a stale reference (an open window, a
-- mention) can't reach a different conversation after one is deleted. SQLite reuses
-- the highest rowid once it's gone; AUTOINCREMENT on this allocator doesn't.
CREATE TABLE mail_thread_ids (id INTEGER PRIMARY KEY AUTOINCREMENT);
INSERT INTO mail_thread_ids (id) SELECT id FROM (SELECT max(id) AS id FROM mail_threads) WHERE id IS NOT NULL;
DELETE FROM mail_thread_ids;
