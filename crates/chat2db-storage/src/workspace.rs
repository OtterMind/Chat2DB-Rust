use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{Storage, StorageError, now_millis};

const MAX_NAMESPACE_NAME_BYTES: usize = 512;

/// Durable workspace node discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceNodeKind {
    /// A user-created namespace.
    Namespace,
    /// A datasource owned by the workspace.
    DataSource,
}

/// Disambiguated storage-level workspace node reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceNodeLocator {
    /// Namespace decimal id or opaque datasource id.
    pub id: String,
    /// Node category.
    pub kind: WorkspaceNodeKind,
}

/// Flat durable workspace node used to build transport-specific trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceNodeRecord {
    /// Namespace decimal id or opaque datasource id.
    pub id: String,
    /// Node category.
    pub kind: WorkspaceNodeKind,
    /// Current display name.
    pub name: String,
    /// Direct parent namespace, or root when absent.
    pub parent_namespace_id: Option<i64>,
    /// Stable zero-based order among direct siblings.
    pub position: u32,
}

/// Persisted namespace metadata and placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceNamespaceRecord {
    /// Positive `SQLite` namespace id.
    pub id: i64,
    /// Current display name.
    pub name: String,
    /// Direct parent namespace, or root when absent.
    pub parent_id: Option<i64>,
}

impl Storage {
    /// Lists every namespace and datasource node in deterministic tree order.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn list_workspace_nodes(&self) -> Result<Vec<WorkspaceNodeRecord>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT n.node_type, n.namespace_id, n.datasource_id,
                    COALESCE(ns.name, ds.name), n.parent_namespace_id, n.position
             FROM workspace_nodes n
             LEFT JOIN workspace_namespaces ns ON ns.id = n.namespace_id
             LEFT JOIN datasources ds ON ds.id = n.datasource_id
             ORDER BY n.parent_namespace_id IS NOT NULL,
                      n.parent_namespace_id, n.position, n.node_key",
        )?;
        let rows = statement.query_map([], decode_workspace_node)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Creates a namespace at the end of the selected parent's children.
    ///
    /// # Errors
    ///
    /// Returns validation, parent-not-found, or `SQLite` failures.
    pub fn create_workspace_namespace(
        &self,
        name: &str,
        parent_id: Option<i64>,
    ) -> Result<WorkspaceNamespaceRecord, StorageError> {
        let name = validate_namespace_name(name)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_parent_namespace(&transaction, parent_id)?;
        let timestamp = now_millis()?;
        transaction.execute(
            "INSERT INTO workspace_namespaces (name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?2)",
            params![name, timestamp],
        )?;
        let id = transaction.last_insert_rowid();
        let position = next_position(&transaction, parent_id)?;
        transaction.execute(
            "INSERT INTO workspace_nodes (
                node_key, node_type, namespace_id, datasource_id,
                parent_namespace_id, position, created_at_ms
             ) VALUES (?1, 'NAMESPACE', ?2, NULL, ?3, ?4, ?5)",
            params![namespace_key(id), id, parent_id, position, timestamp],
        )?;
        transaction.commit()?;
        Ok(WorkspaceNamespaceRecord {
            id,
            name,
            parent_id,
        })
    }

    /// Renames one namespace without changing placement.
    ///
    /// # Errors
    ///
    /// Returns validation, namespace-not-found, or `SQLite` failures.
    pub fn update_workspace_namespace(
        &self,
        id: i64,
        name: &str,
    ) -> Result<WorkspaceNamespaceRecord, StorageError> {
        validate_namespace_id(id)?;
        let name = validate_namespace_name(name)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE workspace_namespaces SET name = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![name, now_millis()?, id],
        )?;
        if changed != 1 {
            return Err(StorageError::WorkspaceNamespaceNotFound(id.to_string()));
        }
        let parent_id = connection.query_row(
            "SELECT parent_namespace_id FROM workspace_nodes WHERE namespace_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(WorkspaceNamespaceRecord {
            id,
            name,
            parent_id,
        })
    }

    /// Deletes one namespace and promotes its ordered children into its parent.
    ///
    /// # Errors
    ///
    /// Returns namespace-not-found, integrity, or `SQLite` failures.
    pub fn delete_workspace_namespace(&self, id: i64) -> Result<(), StorageError> {
        validate_namespace_id(id)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted = load_node(
            &transaction,
            &WorkspaceNodeLocator {
                id: id.to_string(),
                kind: WorkspaceNodeKind::Namespace,
            },
        )?;
        let parent = deleted.parent_namespace_id;
        let siblings = list_node_keys(&transaction, parent, Some(&namespace_key(id)))?;
        let children = list_node_keys(&transaction, Some(id), None)?;
        let insertion = siblings
            .iter()
            .position(|sibling| sibling.position > deleted.position)
            .unwrap_or(siblings.len());
        let mut ordered = Vec::with_capacity(siblings.len() + children.len());
        ordered.extend(siblings[..insertion].iter().map(|node| node.key.clone()));
        ordered.extend(children.iter().map(|node| node.key.clone()));
        ordered.extend(siblings[insertion..].iter().map(|node| node.key.clone()));

        transaction.execute(
            "UPDATE workspace_nodes SET parent_namespace_id = ?1
             WHERE parent_namespace_id = ?2",
            params![parent, id],
        )?;
        let changed =
            transaction.execute("DELETE FROM workspace_namespaces WHERE id = ?1", [id])?;
        if changed != 1 {
            return Err(StorageError::WorkspaceNamespaceNotFound(id.to_string()));
        }
        apply_order(&transaction, parent, &ordered)?;
        transaction.commit()?;
        Ok(())
    }

    /// Applies Community's `before`, `after`, `first child`, and `last child` tree movement.
    ///
    /// # Errors
    ///
    /// Returns invalid-position, missing-node, cycle, or `SQLite` failures.
    pub fn move_workspace_node(
        &self,
        drag: &WorkspaceNodeLocator,
        target: &WorkspaceNodeLocator,
        drop_position: i8,
    ) -> Result<(), StorageError> {
        if !matches!(drop_position, -1..=2) {
            return Err(StorageError::InvalidWorkspace(
                "drop position must be -1, 0, 1, or 2",
            ));
        }
        if drag == target {
            return Err(StorageError::InvalidWorkspace(
                "a workspace node cannot be dropped onto itself",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let drag_record = load_node(&transaction, drag)?;
        let target_record = load_node(&transaction, target)?;
        let drag_key = node_key(drag)?;
        let target_key = node_key(target)?;

        let (new_parent, mut ordered, insertion) = if matches!(drop_position, 0 | 2) {
            if target_record.kind != WorkspaceNodeKind::Namespace {
                return Err(StorageError::InvalidWorkspace(
                    "only namespaces can receive child nodes",
                ));
            }
            let namespace_id = parse_namespace_id(&target_record.id)?;
            let mut children = list_node_keys(&transaction, Some(namespace_id), None)?;
            children.retain(|node| node.key != drag_key);
            let insertion = if drop_position == 0 {
                0
            } else {
                children.len()
            };
            (Some(namespace_id), children, insertion)
        } else {
            let parent = target_record.parent_namespace_id;
            let mut siblings = list_node_keys(&transaction, parent, None)?;
            siblings.retain(|node| node.key != drag_key);
            let target_index = siblings
                .iter()
                .position(|node| node.key == target_key)
                .ok_or_else(|| {
                    StorageError::Integrity(
                        "workspace drop target disappeared during a transaction".to_owned(),
                    )
                })?;
            let insertion = if drop_position < 0 {
                target_index
            } else {
                target_index + 1
            };
            (parent, siblings, insertion)
        };

        if drag_record.kind == WorkspaceNodeKind::Namespace {
            let drag_namespace_id = parse_namespace_id(&drag_record.id)?;
            reject_namespace_cycle(&transaction, drag_namespace_id, new_parent)?;
        }

        ordered.insert(
            insertion,
            OrderedNode {
                key: drag_key.clone(),
                position: 0,
            },
        );
        transaction.execute(
            "UPDATE workspace_nodes SET parent_namespace_id = ?1 WHERE node_key = ?2",
            params![new_parent, drag_key],
        )?;
        apply_order(
            &transaction,
            new_parent,
            &ordered
                .iter()
                .map(|node| node.key.clone())
                .collect::<Vec<_>>(),
        )?;
        if drag_record.parent_namespace_id != new_parent {
            normalize_parent(&transaction, drag_record.parent_namespace_id)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Moves one datasource to the end of a namespace or the root.
    ///
    /// # Errors
    ///
    /// Returns datasource/namespace-not-found or `SQLite` failures.
    pub fn assign_datasource_namespace(
        &self,
        datasource_id: &str,
        namespace_id: Option<i64>,
    ) -> Result<(), StorageError> {
        if datasource_id.trim().is_empty() {
            return Err(StorageError::InvalidWorkspace(
                "datasource id cannot be empty",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_parent_namespace(&transaction, namespace_id)?;
        let locator = WorkspaceNodeLocator {
            id: datasource_id.to_owned(),
            kind: WorkspaceNodeKind::DataSource,
        };
        let current = load_node(&transaction, &locator)?;
        let key = node_key(&locator)?;
        transaction.execute(
            "UPDATE workspace_nodes SET parent_namespace_id = ?1 WHERE node_key = ?2",
            params![namespace_id, key],
        )?;
        let mut destination = list_node_keys(&transaction, namespace_id, None)?;
        destination.retain(|node| node.key != key);
        destination.push(OrderedNode { key, position: 0 });
        apply_order(
            &transaction,
            namespace_id,
            &destination
                .iter()
                .map(|node| node.key.clone())
                .collect::<Vec<_>>(),
        )?;
        if current.parent_namespace_id != namespace_id {
            normalize_parent(&transaction, current.parent_namespace_id)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

#[derive(Debug)]
struct OrderedNode {
    key: String,
    position: u32,
}

fn decode_workspace_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceNodeRecord> {
    let node_type: String = row.get(0)?;
    let namespace_id: Option<i64> = row.get(1)?;
    let datasource_id: Option<String> = row.get(2)?;
    let kind = match node_type.as_str() {
        "NAMESPACE" => WorkspaceNodeKind::Namespace,
        "DATA_SOURCE" => WorkspaceNodeKind::DataSource,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                "invalid workspace node type".into(),
            ));
        }
    };
    let id = match kind {
        WorkspaceNodeKind::Namespace => namespace_id.map(|id| id.to_string()),
        WorkspaceNodeKind::DataSource => datasource_id,
    }
    .ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Null,
            "workspace node identity is missing".into(),
        )
    })?;
    let position: i64 = row.get(5)?;
    Ok(WorkspaceNodeRecord {
        id,
        kind,
        name: row.get(3)?,
        parent_namespace_id: row.get(4)?,
        position: u32::try_from(position)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, position))?,
    })
}

fn load_node(
    connection: &Connection,
    locator: &WorkspaceNodeLocator,
) -> Result<WorkspaceNodeRecord, StorageError> {
    let key = node_key(locator)?;
    connection
        .query_row(
            "SELECT n.node_type, n.namespace_id, n.datasource_id,
                    COALESCE(ns.name, ds.name), n.parent_namespace_id, n.position
             FROM workspace_nodes n
             LEFT JOIN workspace_namespaces ns ON ns.id = n.namespace_id
             LEFT JOIN datasources ds ON ds.id = n.datasource_id
             WHERE n.node_key = ?1",
            [key],
            decode_workspace_node,
        )
        .optional()?
        .ok_or_else(|| StorageError::WorkspaceNodeNotFound(locator.id.clone()))
}

fn list_node_keys(
    connection: &Connection,
    parent: Option<i64>,
    excluded_key: Option<&str>,
) -> Result<Vec<OrderedNode>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT node_key, position FROM workspace_nodes
         WHERE parent_namespace_id IS ?1 AND (?2 IS NULL OR node_key <> ?2)
         ORDER BY position, node_key",
    )?;
    let rows = statement.query_map(params![parent, excluded_key], |row| {
        let position: i64 = row.get(1)?;
        Ok(OrderedNode {
            key: row.get(0)?,
            position: u32::try_from(position)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, position))?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn apply_order(
    transaction: &Transaction<'_>,
    parent: Option<i64>,
    ordered_keys: &[String],
) -> Result<(), StorageError> {
    for (position, key) in ordered_keys.iter().enumerate() {
        transaction.execute(
            "UPDATE workspace_nodes
             SET parent_namespace_id = ?1, position = ?2
             WHERE node_key = ?3",
            params![
                parent,
                i64::try_from(position)
                    .map_err(|_| StorageError::NumericRange("workspace node position"))?,
                key,
            ],
        )?;
    }
    Ok(())
}

fn normalize_parent(
    transaction: &Transaction<'_>,
    parent: Option<i64>,
) -> Result<(), StorageError> {
    let keys = list_node_keys(transaction, parent, None)?
        .into_iter()
        .map(|node| node.key)
        .collect::<Vec<_>>();
    apply_order(transaction, parent, &keys)
}

fn next_position(connection: &Connection, parent: Option<i64>) -> Result<i64, StorageError> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0)
             FROM workspace_nodes WHERE parent_namespace_id IS ?1",
            [parent],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn validate_parent_namespace(
    connection: &Connection,
    parent: Option<i64>,
) -> Result<(), StorageError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    validate_namespace_id(parent)?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspace_namespaces WHERE id = ?1)",
        [parent],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(StorageError::WorkspaceNamespaceNotFound(parent.to_string()))
    }
}

fn reject_namespace_cycle(
    connection: &Connection,
    dragged_namespace: i64,
    mut candidate_parent: Option<i64>,
) -> Result<(), StorageError> {
    let mut visited = 0_usize;
    while let Some(parent) = candidate_parent {
        if parent == dragged_namespace {
            return Err(StorageError::InvalidWorkspace(
                "a namespace cannot be moved into its own descendant",
            ));
        }
        candidate_parent = connection
            .query_row(
                "SELECT parent_namespace_id FROM workspace_nodes WHERE namespace_id = ?1",
                [parent],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        visited += 1;
        if visited > 1_024 {
            return Err(StorageError::Integrity(
                "workspace namespace ancestry is cyclic".to_owned(),
            ));
        }
    }
    Ok(())
}

fn node_key(locator: &WorkspaceNodeLocator) -> Result<String, StorageError> {
    match locator.kind {
        WorkspaceNodeKind::Namespace => {
            let id = parse_namespace_id(&locator.id)?;
            Ok(namespace_key(id))
        }
        WorkspaceNodeKind::DataSource => {
            if locator.id.trim().is_empty() || locator.id.len() > 512 {
                return Err(StorageError::InvalidWorkspace(
                    "datasource id must be non-empty and at most 512 UTF-8 bytes",
                ));
            }
            Ok(format!("datasource:{}", locator.id))
        }
    }
}

fn namespace_key(id: i64) -> String {
    format!("namespace:{id}")
}

fn parse_namespace_id(id: &str) -> Result<i64, StorageError> {
    let id = id
        .parse::<i64>()
        .map_err(|_| StorageError::InvalidWorkspace("namespace id must be a positive integer"))?;
    validate_namespace_id(id)?;
    Ok(id)
}

fn validate_namespace_id(id: i64) -> Result<(), StorageError> {
    if id <= 0 {
        return Err(StorageError::InvalidWorkspace(
            "namespace id must be a positive integer",
        ));
    }
    Ok(())
}

fn validate_namespace_name(name: &str) -> Result<String, StorageError> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_NAMESPACE_NAME_BYTES {
        return Err(StorageError::InvalidWorkspace(
            "namespace name must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::{WorkspaceNodeKind, WorkspaceNodeLocator};
    use crate::{
        CreateDatasource, SecretRef, SecretValue, SecretVault, SecretVaultError, Storage,
        StorageError,
    };

    #[derive(Debug)]
    struct EmptyVault;

    impl SecretVault for EmptyVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            _reference: &SecretRef,
            _value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(None)
        }

        fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
            Ok(())
        }
    }

    fn open(directory: &TempDir) -> Storage {
        Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens")
    }

    fn namespace(id: i64) -> WorkspaceNodeLocator {
        WorkspaceNodeLocator {
            id: id.to_string(),
            kind: WorkspaceNodeKind::Namespace,
        }
    }

    fn datasource(id: &str) -> WorkspaceNodeLocator {
        WorkspaceNodeLocator {
            id: id.to_owned(),
            kind: WorkspaceNodeKind::DataSource,
        }
    }

    #[test]
    fn namespace_tree_survives_restart_and_delete_promotes_children() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let datasource = storage
            .create_datasource(
                CreateDatasource {
                    name: "Local MySQL".to_owned(),
                    driver_id: "mysql".to_owned(),
                },
                None,
            )
            .expect("datasource creates");
        let root = storage
            .create_workspace_namespace("Root", None)
            .expect("root namespace creates");
        let child = storage
            .create_workspace_namespace("Child", Some(root.id))
            .expect("child namespace creates");
        storage
            .assign_datasource_namespace(&datasource.id, Some(child.id))
            .expect("datasource moves");
        drop(storage);

        let reopened = open(&directory);
        let before = reopened
            .list_workspace_nodes()
            .expect("workspace nodes list");
        assert!(before.iter().any(|node| {
            node.kind == WorkspaceNodeKind::DataSource
                && node.id == datasource.id
                && node.parent_namespace_id == Some(child.id)
        }));
        reopened
            .delete_workspace_namespace(root.id)
            .expect("root namespace deletes");
        let after = reopened
            .list_workspace_nodes()
            .expect("workspace nodes relist");
        assert!(after.iter().any(|node| {
            node.kind == WorkspaceNodeKind::Namespace
                && node.id == child.id.to_string()
                && node.parent_namespace_id.is_none()
        }));
        assert!(after.iter().any(|node| {
            node.kind == WorkspaceNodeKind::DataSource
                && node.id == datasource.id
                && node.parent_namespace_id == Some(child.id)
        }));
    }

    #[test]
    fn workspace_reorder_supports_siblings_and_first_or_last_child() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let first = storage
            .create_workspace_namespace("First", None)
            .expect("first creates");
        let second = storage
            .create_workspace_namespace("Second", None)
            .expect("second creates");
        storage
            .move_workspace_node(&namespace(second.id), &namespace(first.id), -1)
            .expect("second moves before first");
        let roots = storage
            .list_workspace_nodes()
            .expect("nodes list")
            .into_iter()
            .filter(|node| node.parent_namespace_id.is_none())
            .collect::<Vec<_>>();
        assert_eq!(roots[0].id, second.id.to_string());
        assert_eq!(roots[1].id, first.id.to_string());

        storage
            .move_workspace_node(&namespace(first.id), &namespace(second.id), 0)
            .expect("first moves inside second");
        let moved = storage
            .list_workspace_nodes()
            .expect("nodes relist")
            .into_iter()
            .find(|node| node.id == first.id.to_string())
            .expect("first remains");
        assert_eq!(moved.parent_namespace_id, Some(second.id));
        assert_eq!(moved.position, 0);

        let first_datasource = storage
            .create_datasource(
                CreateDatasource {
                    name: "First datasource".to_owned(),
                    driver_id: "mysql".to_owned(),
                },
                None,
            )
            .expect("first datasource creates");
        let last_datasource = storage
            .create_datasource(
                CreateDatasource {
                    name: "Last datasource".to_owned(),
                    driver_id: "mysql".to_owned(),
                },
                None,
            )
            .expect("last datasource creates");
        storage
            .assign_datasource_namespace(&first_datasource.id, Some(second.id))
            .expect("first datasource moves inside second");
        storage
            .move_workspace_node(&datasource(&last_datasource.id), &namespace(second.id), 2)
            .expect("last datasource appends inside second");
        let children = storage
            .list_workspace_nodes()
            .expect("nodes relist after append")
            .into_iter()
            .filter(|node| node.parent_namespace_id == Some(second.id))
            .collect::<Vec<_>>();
        assert_eq!(children[0].id, first.id.to_string());
        assert_eq!(children[1].id, first_datasource.id);
        assert_eq!(children[2].id, last_datasource.id);
    }

    #[test]
    fn namespace_cycles_and_unknown_datasources_are_rejected() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let root = storage
            .create_workspace_namespace("Root", None)
            .expect("root creates");
        let child = storage
            .create_workspace_namespace("Child", Some(root.id))
            .expect("child creates");
        let cycle = storage
            .move_workspace_node(&namespace(root.id), &namespace(child.id), 0)
            .expect_err("cycle rejected");
        assert!(matches!(cycle, StorageError::InvalidWorkspace(_)));

        let missing = storage
            .assign_datasource_namespace("missing", None)
            .expect_err("unknown datasource rejected");
        assert!(matches!(missing, StorageError::WorkspaceNodeNotFound(_)));
        assert_eq!(datasource("missing").kind, WorkspaceNodeKind::DataSource);
    }
}
