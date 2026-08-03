use std::collections::HashMap;

use chat2db_contract::{
    AssignDatasourceNamespaceRequest, CreateWorkspaceNamespaceRequest, MoveWorkspaceNodeRequest,
    UpdateWorkspaceNamespaceRequest, WorkspaceDatasourceGroup, WorkspaceDatasourceList,
    WorkspaceNamespace, WorkspaceNodeKind, WorkspaceNodeRef, WorkspaceTree, WorkspaceTreeNode,
};
use chat2db_storage::{
    WorkspaceNodeKind as StoredWorkspaceNodeKind, WorkspaceNodeLocator, WorkspaceNodeRecord,
};

use crate::{AppError, Application, storage_call};

impl Application {
    /// Returns the persisted local datasource workspace tree.
    ///
    /// # Errors
    ///
    /// Returns availability, storage, or persisted-tree integrity failures.
    pub async fn workspace_tree(&self) -> Result<WorkspaceTree, AppError> {
        let storage = self.require_storage()?;
        let records = storage_call(move || storage.list_workspace_nodes()).await?;
        build_tree(records)
    }

    /// Creates a root or child namespace at the end of its sibling list.
    ///
    /// # Errors
    ///
    /// Returns validation, parent-not-found, availability, or storage failures.
    pub async fn create_workspace_namespace(
        &self,
        request: CreateWorkspaceNamespaceRequest,
    ) -> Result<WorkspaceNamespace, AppError> {
        let parent_id = parse_optional_namespace_id(request.parent_id.as_deref())?;
        let storage = self.require_storage()?;
        let record =
            storage_call(move || storage.create_workspace_namespace(&request.name, parent_id))
                .await?;
        Ok(WorkspaceNamespace {
            id: record.id.to_string(),
            name: record.name,
            parent_id: record.parent_id.map(|id| id.to_string()),
        })
    }

    /// Renames one persisted namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, namespace-not-found, availability, or storage failures.
    pub async fn update_workspace_namespace(
        &self,
        request: UpdateWorkspaceNamespaceRequest,
    ) -> Result<WorkspaceNamespace, AppError> {
        let id = parse_namespace_id(&request.id)?;
        let storage = self.require_storage()?;
        let record =
            storage_call(move || storage.update_workspace_namespace(id, &request.name)).await?;
        Ok(WorkspaceNamespace {
            id: record.id.to_string(),
            name: record.name,
            parent_id: record.parent_id.map(|id| id.to_string()),
        })
    }

    /// Deletes a namespace and promotes its direct children into its parent.
    ///
    /// # Errors
    ///
    /// Returns validation, namespace-not-found, availability, or storage failures.
    pub async fn delete_workspace_namespace(&self, id: &str) -> Result<(), AppError> {
        let id = parse_namespace_id(id)?;
        let storage = self.require_storage()?;
        storage_call(move || storage.delete_workspace_namespace(id)).await
    }

    /// Applies Community-compatible namespace/datasource tree movement.
    ///
    /// # Errors
    ///
    /// Returns invalid-position, missing-node, cycle, availability, or storage failures.
    pub async fn move_workspace_node(
        &self,
        request: MoveWorkspaceNodeRequest,
    ) -> Result<(), AppError> {
        let drag = node_locator(request.drag_node)?;
        let target = node_locator(request.drop_to_node)?;
        let storage = self.require_storage()?;
        storage_call(move || storage.move_workspace_node(&drag, &target, request.drop_position))
            .await
    }

    /// Assigns a datasource to the end of one namespace or the root.
    ///
    /// # Errors
    ///
    /// Returns datasource/namespace-not-found, validation, availability, or storage failures.
    pub async fn assign_datasource_namespace(
        &self,
        request: AssignDatasourceNamespaceRequest,
    ) -> Result<(), AppError> {
        let namespace_id = parse_optional_namespace_id(request.namespace_id.as_deref())?;
        let storage = self.require_storage()?;
        storage_call(move || {
            storage.assign_datasource_namespace(&request.datasource_id, namespace_id)
        })
        .await
    }

    /// Returns direct datasource membership for root and every namespace.
    ///
    /// # Errors
    ///
    /// Returns availability, storage, or persisted-tree integrity failures.
    pub async fn workspace_datasource_list(&self) -> Result<WorkspaceDatasourceList, AppError> {
        let storage = self.require_storage()?;
        let records = storage_call(move || storage.list_workspace_nodes()).await?;
        let mut datasource_ids = HashMap::<Option<i64>, Vec<String>>::new();
        let mut namespaces = Vec::<i64>::new();
        for record in records {
            match record.kind {
                StoredWorkspaceNodeKind::Namespace => {
                    namespaces.push(parse_namespace_id(&record.id)?);
                }
                StoredWorkspaceNodeKind::DataSource => datasource_ids
                    .entry(record.parent_namespace_id)
                    .or_default()
                    .push(record.id),
            }
        }
        let mut groups = Vec::with_capacity(namespaces.len() + 1);
        groups.push(WorkspaceDatasourceGroup {
            namespace_id: None,
            datasource_ids: datasource_ids.remove(&None).unwrap_or_default(),
        });
        groups.extend(namespaces.into_iter().map(|namespace_id| {
            WorkspaceDatasourceGroup {
                namespace_id: Some(namespace_id.to_string()),
                datasource_ids: datasource_ids
                    .remove(&Some(namespace_id))
                    .unwrap_or_default(),
            }
        }));
        if !datasource_ids.is_empty() {
            return Err(AppError::internal());
        }
        Ok(WorkspaceDatasourceList { groups })
    }
}

fn build_tree(records: Vec<WorkspaceNodeRecord>) -> Result<WorkspaceTree, AppError> {
    let mut children = HashMap::<Option<i64>, Vec<WorkspaceNodeRecord>>::new();
    for record in records {
        children
            .entry(record.parent_namespace_id)
            .or_default()
            .push(record);
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.id.cmp(&right.id))
        });
    }
    let items = build_children(None, &mut children, 0)?;
    if children.values().any(|records| !records.is_empty()) {
        return Err(AppError::internal());
    }
    Ok(WorkspaceTree { items })
}

fn build_children(
    parent: Option<i64>,
    children: &mut HashMap<Option<i64>, Vec<WorkspaceNodeRecord>>,
    depth: usize,
) -> Result<Vec<WorkspaceTreeNode>, AppError> {
    if depth > 1_024 {
        return Err(AppError::internal());
    }
    let records = children.remove(&parent).unwrap_or_default();
    records
        .into_iter()
        .map(|record| match record.kind {
            StoredWorkspaceNodeKind::Namespace => {
                let namespace_id = parse_namespace_id(&record.id)?;
                Ok(WorkspaceTreeNode {
                    id: record.id.clone(),
                    node_type: WorkspaceNodeKind::Namespace,
                    name: record.name,
                    datasource_id: None,
                    namespace_id: Some(record.id),
                    children: build_children(Some(namespace_id), children, depth + 1)?,
                })
            }
            StoredWorkspaceNodeKind::DataSource => Ok(WorkspaceTreeNode {
                id: record.id.clone(),
                node_type: WorkspaceNodeKind::DataSource,
                name: record.name,
                datasource_id: Some(record.id),
                namespace_id: None,
                children: Vec::new(),
            }),
        })
        .collect()
}

fn node_locator(reference: WorkspaceNodeRef) -> Result<WorkspaceNodeLocator, AppError> {
    let kind = match reference.node_type {
        WorkspaceNodeKind::Namespace => {
            parse_namespace_id(&reference.id)?;
            StoredWorkspaceNodeKind::Namespace
        }
        WorkspaceNodeKind::DataSource => {
            if reference.id.trim().is_empty() {
                return Err(AppError::invalid(
                    "invalid_workspace_operation",
                    "datasource id cannot be empty",
                ));
            }
            StoredWorkspaceNodeKind::DataSource
        }
    };
    Ok(WorkspaceNodeLocator {
        id: reference.id,
        kind,
    })
}

fn parse_optional_namespace_id(value: Option<&str>) -> Result<Option<i64>, AppError> {
    value.map(parse_namespace_id).transpose()
}

fn parse_namespace_id(value: &str) -> Result<i64, AppError> {
    value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            AppError::invalid(
                "invalid_workspace_operation",
                "namespace id must be a positive integer",
            )
        })
}

#[cfg(test)]
mod tests {
    use chat2db_storage::{WorkspaceNodeKind, WorkspaceNodeRecord};

    use super::build_tree;

    #[test]
    fn tree_builder_combines_namespaces_and_datasources_without_secret_fields() {
        let tree = build_tree(vec![
            WorkspaceNodeRecord {
                id: "1".to_owned(),
                kind: WorkspaceNodeKind::Namespace,
                name: "Development".to_owned(),
                parent_namespace_id: None,
                position: 0,
            },
            WorkspaceNodeRecord {
                id: "mysql-local".to_owned(),
                kind: WorkspaceNodeKind::DataSource,
                name: "Local MySQL".to_owned(),
                parent_namespace_id: Some(1),
                position: 0,
            },
        ])
        .expect("tree builds");
        assert_eq!(tree.items.len(), 1);
        assert_eq!(tree.items[0].children.len(), 1);
        let json = serde_json::to_string(&tree).expect("tree serializes");
        assert!(json.contains("mysql-local"));
        for forbidden in ["password", "jdbcUrl", "secretRef"] {
            assert!(!json.contains(forbidden));
        }
    }
}
