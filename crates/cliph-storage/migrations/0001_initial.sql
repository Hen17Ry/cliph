BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS clipboard_items (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    kind             TEXT NOT NULL,
    plain_text       TEXT NOT NULL,
    content_hash     TEXT NOT NULL UNIQUE,
    created_at_ns    INTEGER NOT NULL,
    last_used_at_ns  INTEGER NOT NULL,
    is_pinned        INTEGER NOT NULL DEFAULT 0,

    CHECK (kind IN ('text')),
    CHECK (is_pinned IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_clipboard_items_recent
ON clipboard_items (
    is_pinned DESC,
    last_used_at_ns DESC,
    id DESC
);

PRAGMA user_version = 1;

COMMIT;
