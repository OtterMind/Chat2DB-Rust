BEGIN IMMEDIATE;

CREATE TABLE transfer_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    datasource_id TEXT NOT NULL,
    database_name TEXT NOT NULL,
    schema_name TEXT NOT NULL,
    table_name TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('import_file', 'export_sql', 'export_file')),
    status TEXT NOT NULL CHECK (
        status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled', 'interrupted')
    ),
    task_name TEXT NOT NULL,
    progress_current INTEGER NOT NULL DEFAULT 0 CHECK (progress_current >= 0),
    progress_total INTEGER CHECK (progress_total IS NULL OR progress_total >= 0),
    progress_description TEXT NOT NULL DEFAULT '',
    info_log TEXT NOT NULL DEFAULT '',
    error_log TEXT NOT NULL DEFAULT '',
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    finished_at_ms INTEGER CHECK (finished_at_ms IS NULL OR finished_at_ms >= created_at_ms)
) STRICT;

CREATE INDEX transfer_tasks_created_idx
    ON transfer_tasks (created_at_ms DESC, id DESC);

CREATE INDEX transfer_tasks_status_idx
    ON transfer_tasks (status, updated_at_ms DESC, id DESC);

CREATE TABLE transfer_artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    task_id INTEGER REFERENCES transfer_tasks(id) ON DELETE CASCADE,
    storage_name TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    media_type TEXT NOT NULL,
    format TEXT NOT NULL,
    byte_count INTEGER NOT NULL CHECK (byte_count >= 0),
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER CHECK (expires_at_ms IS NULL OR expires_at_ms >= created_at_ms)
) STRICT;

CREATE UNIQUE INDEX transfer_artifacts_task_idx
    ON transfer_artifacts (task_id) WHERE task_id IS NOT NULL;

CREATE INDEX transfer_artifacts_expiry_idx
    ON transfer_artifacts (expires_at_ms) WHERE expires_at_ms IS NOT NULL;

PRAGMA user_version = 6;
COMMIT;
