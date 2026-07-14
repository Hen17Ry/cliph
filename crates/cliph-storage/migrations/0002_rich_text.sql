BEGIN IMMEDIATE;

ALTER TABLE clipboard_items
ADD COLUMN html_text TEXT;

PRAGMA user_version = 2;

COMMIT;
