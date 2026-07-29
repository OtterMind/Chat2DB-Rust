BEGIN IMMEDIATE;

CREATE TABLE operation_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    data_source_id TEXT,
    data_source_name TEXT,
    connectable INTEGER CHECK (connectable IS NULL OR connectable IN (0, 1)),
    database_name TEXT,
    database_type TEXT,
    ddl TEXT NOT NULL,
    status TEXT NOT NULL,
    operation_rows INTEGER CHECK (operation_rows IS NULL OR operation_rows >= 0),
    use_time INTEGER CHECK (use_time IS NULL OR use_time >= 0),
    extend_info TEXT,
    schema_name TEXT,
    organization_id INTEGER,
    user_name TEXT,
    more INTEGER NOT NULL DEFAULT 0 CHECK (more IN (0, 1)),
    operation_type TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
) STRICT;

CREATE INDEX operation_logs_created_idx
    ON operation_logs (created_at_ms DESC, id DESC);

CREATE INDEX operation_logs_scope_idx
    ON operation_logs (
        operation_type, data_source_id, database_name, schema_name,
        created_at_ms DESC, id DESC
    );

PRAGMA user_version = 4;
COMMIT;
