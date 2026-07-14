PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE clipboard_items_v6 (
    id                         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind                       TEXT NOT NULL,
    plain_text                 TEXT NOT NULL DEFAULT '',
    html_text                  TEXT,
    content_hash               TEXT NOT NULL UNIQUE,
    created_at_ns              INTEGER NOT NULL,
    last_used_at_ns            INTEGER NOT NULL,
    is_pinned                  INTEGER NOT NULL DEFAULT 0,
    classification             TEXT NOT NULL DEFAULT 'plain_text',
    classification_subtype     TEXT,
    classification_confidence  INTEGER NOT NULL DEFAULT 100,

    CHECK (kind IN ('text', 'image', 'files')),
    CHECK (is_pinned IN (0, 1)),
    CHECK (
        classification IN (
            'plain_text',
            'rich_text',
            'code',
            'link',
            'image',
            'files',
            'table',
            'html',
            'rtf',
            'color',
            'unknown'
        )
    ),
    CHECK (
        classification_confidence >= 0
        AND classification_confidence <= 100
    )
);

INSERT INTO clipboard_items_v6 (
    id,
    kind,
    plain_text,
    html_text,
    content_hash,
    created_at_ns,
    last_used_at_ns,
    is_pinned,
    classification,
    classification_subtype,
    classification_confidence
)
SELECT
    id,
    kind,
    plain_text,
    html_text,
    content_hash,
    created_at_ns,
    last_used_at_ns,
    is_pinned,
    classification,
    classification_subtype,
    classification_confidence
FROM clipboard_items;

DROP TABLE clipboard_items;

ALTER TABLE clipboard_items_v6
RENAME TO clipboard_items;

CREATE INDEX idx_clipboard_items_recent
ON clipboard_items (
    is_pinned DESC,
    last_used_at_ns DESC,
    id DESC
);

CREATE TABLE file_collections (
    item_id    INTEGER PRIMARY KEY,
    operation  TEXT NOT NULL,

    FOREIGN KEY (item_id)
        REFERENCES clipboard_items(id)
        ON DELETE CASCADE,

    CHECK (operation IN ('copy', 'cut'))
);

CREATE TABLE file_entries (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id             INTEGER NOT NULL,
    position            INTEGER NOT NULL,
    uri                 TEXT NOT NULL,
    local_path          TEXT,
    display_name        TEXT NOT NULL,
    mime_type           TEXT,
    byte_size           INTEGER,
    is_directory        INTEGER NOT NULL DEFAULT 0,
    existed_at_capture  INTEGER NOT NULL DEFAULT 1,

    FOREIGN KEY (item_id)
        REFERENCES file_collections(item_id)
        ON DELETE CASCADE,

    UNIQUE (item_id, position),
    UNIQUE (item_id, uri),

    CHECK (position >= 0),
    CHECK (byte_size IS NULL OR byte_size >= 0),
    CHECK (is_directory IN (0, 1)),
    CHECK (existed_at_capture IN (0, 1))
);

CREATE INDEX idx_file_entries_item_position
ON file_entries (item_id, position);

CREATE INDEX idx_file_entries_uri
ON file_entries (uri);

PRAGMA user_version = 6;

COMMIT;

PRAGMA foreign_keys = ON;
