BEGIN IMMEDIATE;

ALTER TABLE clipboard_items
RENAME TO clipboard_items_old;

CREATE TABLE clipboard_items (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    kind             TEXT NOT NULL,
    plain_text       TEXT NOT NULL DEFAULT '',
    html_text        TEXT,
    content_hash     TEXT NOT NULL UNIQUE,
    created_at_ns    INTEGER NOT NULL,
    last_used_at_ns  INTEGER NOT NULL,
    is_pinned        INTEGER NOT NULL DEFAULT 0,

    CHECK (kind IN ('text', 'image')),
    CHECK (is_pinned IN (0, 1))
);

INSERT INTO clipboard_items (
    id,
    kind,
    plain_text,
    html_text,
    content_hash,
    created_at_ns,
    last_used_at_ns,
    is_pinned
)
SELECT
    id,
    kind,
    plain_text,
    html_text,
    content_hash,
    created_at_ns,
    last_used_at_ns,
    is_pinned
FROM clipboard_items_old;

DROP TABLE clipboard_items_old;

CREATE INDEX idx_clipboard_items_recent
ON clipboard_items (
    is_pinned DESC,
    last_used_at_ns DESC,
    id DESC
);

CREATE TABLE image_payloads (
    item_id        INTEGER PRIMARY KEY,
    relative_path  TEXT NOT NULL UNIQUE,
    mime_type      TEXT NOT NULL,
    width          INTEGER NOT NULL,
    height         INTEGER NOT NULL,
    byte_size      INTEGER NOT NULL,

    FOREIGN KEY (item_id)
        REFERENCES clipboard_items(id)
        ON DELETE CASCADE,

    CHECK (width > 0),
    CHECK (height > 0),
    CHECK (byte_size > 0)
);

PRAGMA user_version = 3;

COMMIT;
