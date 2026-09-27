-- Smart folders: the user names a folder and says what goes in it; their chosen sorter
-- (their model or Jev) files conversations into it. Labels only: nothing changes on
-- the mail server.
CREATE TABLE mail_folders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    -- What goes in it, in the user's words. Given to the sorter as is.
    description TEXT NOT NULL,
    created_at  INTEGER NOT NULL
) STRICT;

-- One row per conversation checked against a folder: in it or not, and who decided.
-- A user's decision is never overwritten; changing a folder's description forgets the
-- sorter's decisions so they're made again.
CREATE TABLE mail_folder_threads (
    folder_id INTEGER NOT NULL REFERENCES mail_folders (id) ON DELETE CASCADE,
    thread_id INTEGER NOT NULL REFERENCES mail_threads (id) ON DELETE CASCADE,
    member    INTEGER NOT NULL,
    -- auto | user
    source    TEXT NOT NULL,
    PRIMARY KEY (folder_id, thread_id)
) STRICT;

CREATE INDEX mail_folder_threads_thread ON mail_folder_threads (thread_id);
