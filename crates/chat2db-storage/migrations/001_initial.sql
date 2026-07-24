BEGIN IMMEDIATE;

CREATE TABLE datasources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    driver_id TEXT NOT NULL,
    secret_ref TEXT,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE secret_cleanup_queue (
    secret_ref TEXT PRIMARY KEY NOT NULL,
    enqueued_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE retained_results (
    id TEXT PRIMARY KEY NOT NULL,
    format_version INTEGER NOT NULL CHECK (format_version >= 1),
    state TEXT NOT NULL CHECK (state IN ('writing', 'complete')),
    schema_frame_length INTEGER NOT NULL CHECK (schema_frame_length >= 5),
    schema_sha256 BLOB NOT NULL CHECK (length(schema_sha256) = 32),
    committed_length INTEGER NOT NULL CHECK (committed_length >= schema_frame_length),
    row_count INTEGER NOT NULL CHECK (row_count >= 0),
    truncated_by_max_rows INTEGER NOT NULL DEFAULT 0 CHECK (truncated_by_max_rows IN (0, 1)),
    truncated_by_max_result_bytes INTEGER NOT NULL DEFAULT 0 CHECK (truncated_by_max_result_bytes IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX retained_results_expiry_idx
    ON retained_results (expires_at_ms);

CREATE TABLE result_chunks (
    result_id TEXT NOT NULL REFERENCES retained_results(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    file_offset INTEGER NOT NULL CHECK (file_offset >= 5),
    frame_length INTEGER NOT NULL CHECK (frame_length >= 5),
    start_row INTEGER NOT NULL CHECK (start_row >= 0),
    end_row_exclusive INTEGER NOT NULL CHECK (end_row_exclusive > start_row),
    row_count INTEGER NOT NULL CHECK (row_count > 0),
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    PRIMARY KEY (result_id, ordinal),
    UNIQUE (result_id, start_row)
) STRICT;

CREATE INDEX result_chunks_page_idx
    ON result_chunks (result_id, start_row);

PRAGMA user_version = 1;
COMMIT;
