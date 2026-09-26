-- Tool use per assistant message, as a JSON array of protocol `Action`s.
ALTER TABLE messages ADD COLUMN actions TEXT NOT NULL DEFAULT '[]';
