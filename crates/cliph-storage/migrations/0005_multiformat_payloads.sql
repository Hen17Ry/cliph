BEGIN IMMEDIATE;

CREATE TABLE clipboard_item_format_payloads (
    item_id    INTEGER NOT NULL,
    mime_type  TEXT NOT NULL,
    data       BLOB NOT NULL,
    byte_size  INTEGER NOT NULL,

    PRIMARY KEY (item_id, mime_type),

    FOREIGN KEY (item_id)
        REFERENCES clipboard_items(id)
        ON DELETE CASCADE,

    CHECK (byte_size >= 0)
);

CREATE INDEX idx_clipboard_item_format_payloads_mime
ON clipboard_item_format_payloads (mime_type);

INSERT OR IGNORE INTO clipboard_item_format_payloads (
    item_id,
    mime_type,
    data,
    byte_size
)
SELECT
    id,
    'text/html',
    CAST(html_text AS BLOB),
    LENGTH(CAST(html_text AS BLOB))
FROM clipboard_items
WHERE kind = 'text'
  AND html_text IS NOT NULL
  AND TRIM(html_text) <> '';

PRAGMA user_version = 5;

COMMIT;
