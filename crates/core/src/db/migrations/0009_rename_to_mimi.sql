-- The project was renamed from Hearth to Mimi. Keep a custom assistant name, but move
-- the old default along with the rename.
UPDATE settings
SET value = json_set(value, '$.assistant_name', 'Mimi')
WHERE key = 'app' AND json_extract(value, '$.assistant_name') = 'Hearth';
