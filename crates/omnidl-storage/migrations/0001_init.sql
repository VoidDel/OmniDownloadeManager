-- Tasks and their segments (docs/development-plan.md §4.6). Sizes and offsets are bytes; times
-- are Unix milliseconds. Enumerations are stored as their snake_case names.

CREATE TABLE tasks (
    id            TEXT PRIMARY KEY,          -- UUID
    kind          TEXT NOT NULL,             -- http | ftp | torrent
    source        TEXT NOT NULL,             -- URL as entered
    final_url     TEXT,                      -- after redirects
    save_dir      TEXT NOT NULL,
    file_name     TEXT NOT NULL,             -- final name; the partial file adds ".odm"
    status        TEXT NOT NULL,
    prev_status   TEXT,                      -- status before it went to the recycle bin
    total_bytes   INTEGER,                   -- NULL when the server does not say
    downloaded    INTEGER NOT NULL DEFAULT 0,
    uploaded      INTEGER NOT NULL DEFAULT 0,
    category      TEXT NOT NULL DEFAULT 'other',
    priority      INTEGER NOT NULL DEFAULT 1,
    sort_order    INTEGER NOT NULL,
    options_json  TEXT NOT NULL,
    etag          TEXT,
    last_modified TEXT,
    resumable     INTEGER NOT NULL DEFAULT 0,
    error_json    TEXT,
    created_at    INTEGER NOT NULL,
    started_at    INTEGER,
    completed_at  INTEGER
);

-- `end_pos` is inclusive and NULL while the length is unknown (single segment).
CREATE TABLE segments (
    task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_pos   INTEGER NOT NULL,
    end_pos     INTEGER,
    downloaded  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (task_id, idx)
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_sort ON tasks(sort_order);
