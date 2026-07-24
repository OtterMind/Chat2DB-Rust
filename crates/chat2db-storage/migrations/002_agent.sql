BEGIN IMMEDIATE;

CREATE TABLE provider_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('openai_compatible', 'anthropic', 'gemini')),
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    context_window_tokens INTEGER NOT NULL CHECK (context_window_tokens > 0),
    max_output_tokens INTEGER NOT NULL CHECK (max_output_tokens > 0),
    secret_ref TEXT,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    provider_id TEXT NOT NULL REFERENCES provider_profiles(id) ON DELETE RESTRICT,
    datasource_id TEXT REFERENCES datasources(id) ON DELETE SET NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX agent_sessions_provider_idx ON agent_sessions (provider_id);
CREATE INDEX agent_sessions_datasource_idx ON agent_sessions (datasource_id);

CREATE TABLE agent_messages (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES agent_runs(id) ON DELETE SET NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool', 'summary')),
    summary_through_ordinal INTEGER CHECK (summary_through_ordinal >= 0),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    content_bytes INTEGER NOT NULL CHECK (
        content_bytes > 0 AND content_bytes = length(CAST(content_json AS BLOB))
    ),
    created_at_ms INTEGER NOT NULL,
    CHECK (
        (role = 'summary' AND summary_through_ordinal IS NOT NULL)
        OR (role <> 'summary' AND summary_through_ordinal IS NULL)
    ),
    UNIQUE (session_id, ordinal)
) STRICT;

CREATE INDEX agent_messages_run_idx ON agent_messages (run_id, ordinal);

CREATE TABLE agent_runs (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    sql_permission_mode TEXT NOT NULL CHECK (
        sql_permission_mode IN ('read_only', 'ask_before_write')
    ),
    status TEXT NOT NULL CHECK (
        status IN (
            'running', 'waiting_permission', 'completed', 'failed', 'cancelled'
        )
    ),
    last_sequence INTEGER NOT NULL DEFAULT 1 CHECK (last_sequence >= 1),
    model_rounds INTEGER NOT NULL DEFAULT 0 CHECK (model_rounds >= 0),
    tool_calls INTEGER NOT NULL DEFAULT 0 CHECK (tool_calls >= 0),
    input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
    message_id TEXT REFERENCES agent_messages(id) ON DELETE SET NULL,
    error_code TEXT,
    error_message TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    write_in_flight_tool_call_id TEXT,
    write_in_flight_arguments_sha256 BLOB,
    compaction_count INTEGER NOT NULL DEFAULT 0 CHECK (compaction_count >= 0),
    compacted_through_ordinal INTEGER CHECK (compacted_through_ordinal >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    started_at_ms INTEGER,
    finished_at_ms INTEGER,
    CHECK (
        (write_in_flight_tool_call_id IS NULL AND write_in_flight_arguments_sha256 IS NULL)
        OR (
            write_in_flight_tool_call_id IS NOT NULL
            AND write_in_flight_arguments_sha256 IS NOT NULL
            AND length(write_in_flight_arguments_sha256) = 32
        )
    ),
    UNIQUE (id, session_id)
) STRICT;

CREATE INDEX agent_runs_session_idx ON agent_runs (session_id, created_at_ms, id);
CREATE UNIQUE INDEX agent_runs_active_session_idx
    ON agent_runs (session_id) WHERE status IN ('running', 'waiting_permission');

CREATE TABLE tool_permissions (
    id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_sha256 BLOB NOT NULL CHECK (length(arguments_sha256) = 32),
    summary TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'approved', 'denied', 'consumed', 'expired', 'revoked')
    ),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    expires_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER,
    UNIQUE (run_id, tool_call_id)
) STRICT;

CREATE INDEX tool_permissions_expiry_idx ON tool_permissions (expires_at_ms, status);
CREATE UNIQUE INDEX tool_permissions_active_run_idx
    ON tool_permissions (run_id) WHERE status IN ('pending', 'approved');

CREATE TABLE agent_result_handles (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    result_id TEXT NOT NULL REFERENCES retained_results(id) ON DELETE CASCADE,
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    FOREIGN KEY (run_id, session_id) REFERENCES agent_runs(id, session_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX agent_result_handles_expiry_idx ON agent_result_handles (expires_at_ms);
CREATE INDEX agent_result_handles_owner_idx
    ON agent_result_handles (session_id, run_id, created_at_ms, id);

PRAGMA user_version = 2;
COMMIT;
