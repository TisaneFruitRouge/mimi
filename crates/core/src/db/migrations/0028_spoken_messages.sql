-- The user said the message out loud: its words were transcribed from the microphone or
-- a voice message (the recording itself is never kept).
ALTER TABLE messages ADD COLUMN spoken INTEGER NOT NULL DEFAULT 0;
