BEGIN IMMEDIATE;

ALTER TABLE clipboard_items
ADD COLUMN classification TEXT NOT NULL DEFAULT 'plain_text';

ALTER TABLE clipboard_items
ADD COLUMN classification_subtype TEXT;

ALTER TABLE clipboard_items
ADD COLUMN classification_confidence INTEGER NOT NULL DEFAULT 100;

CREATE TABLE clipboard_item_mime_types (
    item_id    INTEGER NOT NULL,
    mime_type  TEXT NOT NULL,

    PRIMARY KEY (item_id, mime_type),

    FOREIGN KEY (item_id)
        REFERENCES clipboard_items(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_clipboard_item_mime_types_mime
ON clipboard_item_mime_types (mime_type);

UPDATE clipboard_items
SET
    classification = CASE
        WHEN kind = 'image' THEN 'image'
        WHEN html_text IS NOT NULL
             AND TRIM(html_text) <> '' THEN 'rich_text'
        ELSE 'plain_text'
    END,
    classification_subtype = CASE
        WHEN kind = 'image' THEN 'PNG'
        WHEN html_text IS NOT NULL
             AND TRIM(html_text) <> '' THEN 'HTML'
        ELSE NULL
    END,
    classification_confidence = 100;

INSERT OR IGNORE INTO clipboard_item_mime_types (
    item_id,
    mime_type
)
SELECT
    id,
    'text/plain'
FROM clipboard_items
WHERE kind = 'text';

INSERT OR IGNORE INTO clipboard_item_mime_types (
    item_id,
    mime_type
)
SELECT
    id,
    'text/html'
FROM clipboard_items
WHERE kind = 'text'
  AND html_text IS NOT NULL
  AND TRIM(html_text) <> '';

INSERT OR IGNORE INTO clipboard_item_mime_types (
    item_id,
    mime_type
)
SELECT
    id,
    'image/png'
FROM clipboard_items
WHERE kind = 'image';

PRAGMA user_version = 4;

COMMIT;
