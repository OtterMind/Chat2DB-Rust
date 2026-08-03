BEGIN IMMEDIATE;

CREATE TABLE workspace_namespaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
) STRICT;

CREATE TABLE workspace_nodes (
    node_key TEXT PRIMARY KEY NOT NULL,
    node_type TEXT NOT NULL CHECK (node_type IN ('NAMESPACE', 'DATA_SOURCE')),
    namespace_id INTEGER UNIQUE REFERENCES workspace_namespaces(id) ON DELETE CASCADE,
    datasource_id TEXT UNIQUE REFERENCES datasources(id) ON DELETE CASCADE,
    parent_namespace_id INTEGER REFERENCES workspace_namespaces(id) ON DELETE SET NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    created_at_ms INTEGER NOT NULL,
    CHECK (
        (node_type = 'NAMESPACE' AND namespace_id IS NOT NULL AND datasource_id IS NULL)
        OR
        (node_type = 'DATA_SOURCE' AND namespace_id IS NULL AND datasource_id IS NOT NULL)
    )
) STRICT;

CREATE INDEX workspace_nodes_parent_position_idx
    ON workspace_nodes (parent_namespace_id, position, node_key);

INSERT INTO workspace_nodes (
    node_key, node_type, namespace_id, datasource_id,
    parent_namespace_id, position, created_at_ms
)
SELECT
    'datasource:' || id,
    'DATA_SOURCE',
    NULL,
    id,
    NULL,
    ROW_NUMBER() OVER (ORDER BY created_at_ms, id) - 1,
    created_at_ms
FROM datasources;

CREATE TRIGGER workspace_datasource_insert
AFTER INSERT ON datasources
BEGIN
    INSERT INTO workspace_nodes (
        node_key, node_type, namespace_id, datasource_id,
        parent_namespace_id, position, created_at_ms
    ) VALUES (
        'datasource:' || NEW.id,
        'DATA_SOURCE',
        NULL,
        NEW.id,
        NULL,
        COALESCE(
            (SELECT MAX(position) + 1
             FROM workspace_nodes
             WHERE parent_namespace_id IS NULL),
            0
        ),
        NEW.created_at_ms
    );
END;

PRAGMA user_version = 5;
COMMIT;
