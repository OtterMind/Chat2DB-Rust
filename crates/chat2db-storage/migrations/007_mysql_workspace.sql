BEGIN IMMEDIATE;

CREATE TABLE mysql_pinned_tables (
    datasource_id TEXT NOT NULL REFERENCES datasources(id) ON DELETE CASCADE,
    database_name TEXT NOT NULL,
    schema_name TEXT NOT NULL,
    table_name TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (datasource_id, database_name, schema_name, table_name)
) STRICT;

CREATE TABLE mysql_er_positions (
    datasource_id TEXT NOT NULL REFERENCES datasources(id) ON DELETE CASCADE,
    database_name TEXT NOT NULL,
    schema_name TEXT NOT NULL,
    position TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (datasource_id, database_name, schema_name)
) STRICT;

PRAGMA user_version = 7;
COMMIT;
