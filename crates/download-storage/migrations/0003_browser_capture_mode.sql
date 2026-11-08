ALTER TABLE browser_preferences
    ADD COLUMN capture_mode TEXT NOT NULL DEFAULT 'automatic'
    CHECK (capture_mode IN ('off', 'ask', 'automatic'));

UPDATE browser_preferences
SET capture_mode = CASE
    WHEN capture_enabled = 0 THEN 'off'
    WHEN automatic_interception = 1 THEN 'automatic'
    ELSE 'ask'
END;
