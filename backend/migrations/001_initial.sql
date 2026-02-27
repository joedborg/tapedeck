-- Initial schema for tapedeck

CREATE TABLE IF NOT EXISTS users (
    id          TEXT PRIMARY KEY NOT NULL,
    username    TEXT NOT NULL UNIQUE,
    password    TEXT NOT NULL,   -- argon2 hash
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS queue_items (
    id              TEXT PRIMARY KEY NOT NULL,
    pid             TEXT NOT NULL,          -- BBC PID (e.g. b0001234)
    title           TEXT NOT NULL,
    series          TEXT,
    episode         TEXT,
    channel         TEXT,
    media_type      TEXT NOT NULL DEFAULT 'tv',   -- tv | radio
    thumbnail_url   TEXT,
    added_at        TEXT NOT NULL DEFAULT (datetime('now')),
    scheduled_at    TEXT,                   -- NULL = download immediately
    priority        INTEGER NOT NULL DEFAULT 5,  -- 1 (high) – 10 (low)
    status          TEXT NOT NULL DEFAULT 'queued',  -- queued | downloading | done | failed | cancelled
    started_at      TEXT,
    completed_at    TEXT,
    error           TEXT,
    output_path     TEXT,
    progress        REAL NOT NULL DEFAULT 0.0,
    speed           TEXT,
    eta             TEXT,
    file_size       INTEGER,
    quality         TEXT NOT NULL DEFAULT 'best',
    subtitles       INTEGER NOT NULL DEFAULT 1,
    metadata        TEXT NOT NULL DEFAULT '{}',  -- JSON blob of extra PID info
    user_id         TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_queue_status   ON queue_items(status);
CREATE INDEX IF NOT EXISTS idx_queue_added_at ON queue_items(added_at);
CREATE INDEX IF NOT EXISTS idx_queue_pid      ON queue_items(pid);

CREATE TABLE IF NOT EXISTS settings (
    key     TEXT PRIMARY KEY NOT NULL,
    value   TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Defaults
INSERT OR IGNORE INTO settings(key, value) VALUES
    ('output_dir',            '/downloads'),
    ('max_concurrent',        '5'),
    ('max_download_retries',  '5'),
    ('default_quality',       'best'),
    ('subtitles',             'true'),
    ('tvmode',                'best'),
    ('radiomode',             'best'),
    ('proxy',                 ''),
    ('get_iplayer_path',      'get_iplayer'),
    ('ffmpeg_path',           'ffmpeg');
