-- How a smart folder looks: an icon and a colour, chosen from fixed sets
-- (`mail::folders::ICONS`, `COLORS`).
ALTER TABLE mail_folders ADD COLUMN icon TEXT NOT NULL DEFAULT 'sparkles';
ALTER TABLE mail_folders ADD COLUMN color TEXT NOT NULL DEFAULT 'violet';
