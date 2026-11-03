-- Matrix (connections/matrix/rooms.rs): the rooms the assistant keeps besides its owner's
-- chat, and who counts as known for sending messages on its own. Everything belongs to
-- one connection, so removing the connection removes all of it.

-- Rooms the assistant stays in. 'group': the owner invited it in; 'joined': a public
-- group it joined to post there, with the user's OK; 'direct': a chat it opened with one
-- person (user_id) to message them.
CREATE TABLE matrix_rooms (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    room_id       TEXT NOT NULL,
    why           TEXT NOT NULL CHECK (why IN ('group', 'joined', 'direct')),
    user_id       TEXT,
    name          TEXT,
    created_at    INTEGER NOT NULL,
    PRIMARY KEY (connection_id, room_id)
) STRICT;

-- People (user ids) and groups (room ids) the assistant has messaged for the user, and
-- groups the owner invited it into. Kept when it leaves a room: they stay known.
CREATE TABLE matrix_known (
    connection_id TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    target        TEXT NOT NULL,
    why           TEXT NOT NULL CHECK (why IN ('invited', 'messaged')),
    created_at    INTEGER NOT NULL,
    PRIMARY KEY (connection_id, target)
) STRICT;
