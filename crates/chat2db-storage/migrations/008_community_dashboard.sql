BEGIN IMMEDIATE;

CREATE TABLE community_charts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    gmt_create_ms INTEGER NOT NULL CHECK (gmt_create_ms >= 0),
    gmt_modified_ms INTEGER NOT NULL CHECK (gmt_modified_ms >= gmt_create_ms),
    name TEXT CHECK (name IS NULL OR length(CAST(name AS BLOB)) <= 1024),
    description TEXT CHECK (description IS NULL OR length(CAST(description AS BLOB)) <= 1048576),
    schema_text TEXT CHECK (schema_text IS NULL OR length(CAST(schema_text AS BLOB)) <= 16777216),
    data_source_id INTEGER,
    data_source_name TEXT CHECK (data_source_name IS NULL OR length(CAST(data_source_name AS BLOB)) <= 1024),
    schema_name TEXT CHECK (schema_name IS NULL OR length(CAST(schema_name AS BLOB)) <= 1024),
    chart_type TEXT CHECK (chart_type IS NULL OR length(CAST(chart_type AS BLOB)) <= 256),
    database_name TEXT CHECK (database_name IS NULL OR length(CAST(database_name AS BLOB)) <= 1024),
    ddl TEXT CHECK (ddl IS NULL OR length(CAST(ddl AS BLOB)) <= 16777216),
    deleted TEXT CHECK (deleted IS NULL OR length(CAST(deleted AS BLOB)) <= 32),
    user_id INTEGER,
    chart_schema_json TEXT CHECK (
        chart_schema_json IS NULL OR (
            json_valid(chart_schema_json)
            AND length(CAST(chart_schema_json AS BLOB)) <= 16777216
        )
    ),
    meta_data_json TEXT CHECK (
        meta_data_json IS NULL OR (
            json_valid(meta_data_json)
            AND length(CAST(meta_data_json AS BLOB)) <= 16777216
        )
    ),
    database_info_json TEXT CHECK (
        database_info_json IS NULL OR (
            json_valid(database_info_json)
            AND length(CAST(database_info_json AS BLOB)) <= 16777216
        )
    ),
    refresh_type TEXT CHECK (refresh_type IS NULL OR length(CAST(refresh_type AS BLOB)) <= 256),
    refresh_cycle_json TEXT CHECK (
        refresh_cycle_json IS NULL OR (
            json_valid(refresh_cycle_json)
            AND length(CAST(refresh_cycle_json AS BLOB)) <= 16777216
        )
    )
) STRICT;

CREATE TABLE community_dashboards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    gmt_create_ms INTEGER NOT NULL CHECK (gmt_create_ms >= 0),
    gmt_modified_ms INTEGER NOT NULL CHECK (gmt_modified_ms >= gmt_create_ms),
    name TEXT CHECK (name IS NULL OR length(CAST(name AS BLOB)) <= 1024),
    description TEXT CHECK (description IS NULL OR length(CAST(description AS BLOB)) <= 1048576),
    data_source_collection_id INTEGER,
    chart_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(chart_ids_json)
        AND json_type(chart_ids_json) = 'array'
        AND length(CAST(chart_ids_json AS BLOB)) <= 2097152
    ),
    schema_text TEXT CHECK (schema_text IS NULL OR length(CAST(schema_text AS BLOB)) <= 16777216),
    refresh_type TEXT CHECK (refresh_type IS NULL OR length(CAST(refresh_type AS BLOB)) <= 256),
    refresh_cycle_json TEXT CHECK (
        refresh_cycle_json IS NULL OR (
            json_valid(refresh_cycle_json)
            AND length(CAST(refresh_cycle_json AS BLOB)) <= 16777216
        )
    ),
    user_id INTEGER
) STRICT;

CREATE INDEX community_dashboards_modified_idx
    ON community_dashboards (gmt_modified_ms DESC, id DESC);

CREATE TRIGGER community_dashboards_delete_charts
AFTER DELETE ON community_dashboards
BEGIN
    DELETE FROM community_charts
    WHERE id IN (
        SELECT value
        FROM json_each(OLD.chart_ids_json)
        WHERE type = 'integer'
    );
END;

PRAGMA user_version = 8;
COMMIT;
