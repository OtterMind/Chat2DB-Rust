BEGIN IMMEDIATE;

ALTER TABLE datasources ADD COLUMN identity_color TEXT
    CHECK (identity_color IS NULL OR length(CAST(identity_color AS BLOB)) <= 16);

ALTER TABLE transfer_tasks ADD COLUMN task_type TEXT
    CHECK (task_type IS NULL OR length(CAST(task_type AS BLOB)) <= 64);

ALTER TABLE transfer_tasks ADD COLUMN format TEXT
    CHECK (format IS NULL OR length(CAST(format AS BLOB)) <= 32);

CREATE TABLE transfer_task_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES transfer_tasks(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    level TEXT NOT NULL CHECK (level IN ('INFO', 'WARN', 'ERROR')),
    code TEXT NOT NULL DEFAULT '',
    stage TEXT,
    message TEXT NOT NULL,
    details_json TEXT,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (task_id, sequence)
) STRICT;

CREATE INDEX transfer_task_events_task_idx
    ON transfer_task_events (task_id, sequence);

PRAGMA user_version = 9;
COMMIT;
