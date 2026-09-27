-- First-run onboarding is new. Anyone who already chose a model has been set up.
UPDATE settings
SET value = json_set(value, '$.onboarding_done', json('true'))
WHERE key = 'app' AND json_extract(value, '$.default_model') IS NOT NULL;
