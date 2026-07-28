BEGIN IMMEDIATE;

CREATE TABLE saved_consoles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    data_source_id TEXT,
    data_source_name TEXT,
    database_name TEXT,
    schema_name TEXT,
    database_type TEXT,
    ddl TEXT NOT NULL,
    status TEXT NOT NULL,
    tab_opened TEXT NOT NULL CHECK (tab_opened IN ('y', 'n')),
    operation_type TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
) STRICT;

CREATE INDEX saved_consoles_created_idx
    ON saved_consoles (created_at_ms, id);

CREATE INDEX saved_consoles_updated_idx
    ON saved_consoles (updated_at_ms DESC, id DESC);

CREATE INDEX saved_consoles_open_scope_idx
    ON saved_consoles (
        tab_opened, data_source_id, database_name, schema_name, created_at_ms, id
    );

CREATE INDEX saved_consoles_status_idx
    ON saved_consoles (status, updated_at_ms DESC, id DESC);

PRAGMA user_version = 3;
COMMIT;
