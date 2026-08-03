use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Stable kind discriminator for one node in the local datasource workspace tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceNodeKind {
    /// A user-created grouping node.
    Namespace,
    /// A persisted datasource.
    DataSource,
}

/// Disambiguated reference to a workspace node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceNodeRef {
    /// Namespace decimal id or opaque datasource id.
    pub id: String,
    /// Node category used to resolve the id safely.
    #[serde(rename = "type")]
    pub node_type: WorkspaceNodeKind,
}

/// Secret-free node returned by the local workspace tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTreeNode {
    /// Namespace decimal id or opaque datasource id.
    pub id: String,
    /// Node category.
    #[serde(rename = "type")]
    pub node_type: WorkspaceNodeKind,
    /// Current display name.
    pub name: String,
    /// Opaque datasource id for datasource nodes only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datasource_id: Option<String>,
    /// Decimal namespace id for namespace nodes only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_id: Option<String>,
    /// Ordered direct children. Datasource nodes always return an empty list.
    pub children: Vec<Self>,
}

/// Ordered root nodes of the local datasource workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTree {
    /// Root namespace and datasource nodes.
    pub items: Vec<WorkspaceTreeNode>,
}

/// Request to create one root or child namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceNamespaceRequest {
    /// User-visible namespace name.
    pub name: String,
    /// Optional decimal id of the parent namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

/// Request to rename one namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkspaceNamespaceRequest {
    /// Decimal namespace id.
    pub id: String,
    /// Replacement display name.
    pub name: String,
}

/// Persisted namespace metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceNamespace {
    /// Decimal namespace id.
    pub id: String,
    /// User-visible namespace name.
    pub name: String,
    /// Optional decimal id of the parent namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

/// Community-compatible drag-and-drop operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveWorkspaceNodeRequest {
    /// Node being moved.
    pub drag_node: WorkspaceNodeRef,
    /// Node receiving or anchoring the move.
    pub drop_to_node: WorkspaceNodeRef,
    /// `0` inserts as first child, `2` as last child, `-1` before, and `1` after the target.
    pub drop_position: i8,
}

/// Explicit datasource-to-namespace assignment used by the compatibility API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignDatasourceNamespaceRequest {
    /// Opaque datasource id.
    pub datasource_id: String,
    /// Destination namespace, or `None` for the root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_id: Option<String>,
}

/// Datasource ids directly assigned to one namespace or to the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDatasourceGroup {
    /// Namespace decimal id, or `None` for root datasources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_id: Option<String>,
    /// Ordered datasource ids directly owned by the namespace.
    pub datasource_ids: Vec<String>,
}

/// Stable namespace-to-datasource mapping used by the retained Community client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDatasourceList {
    /// Root and namespace groups in stable tree order.
    pub groups: Vec<WorkspaceDatasourceGroup>,
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceNodeKind, WorkspaceTreeNode};

    #[test]
    fn workspace_nodes_are_disambiguated_and_secret_free() {
        let node = WorkspaceTreeNode {
            id: "datasource-1".to_owned(),
            node_type: WorkspaceNodeKind::DataSource,
            name: "Local MySQL".to_owned(),
            datasource_id: Some("datasource-1".to_owned()),
            namespace_id: None,
            children: Vec::new(),
        };
        let json = serde_json::to_value(node).expect("node serializes");
        assert_eq!(json["type"], "DATA_SOURCE");
        for forbidden in ["password", "jdbcUrl", "properties", "secretRef"] {
            assert!(!json.to_string().contains(forbidden));
        }
    }
}
