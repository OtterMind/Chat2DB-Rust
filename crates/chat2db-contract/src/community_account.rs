use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// `MySQL` account action exposed by the retained Community account UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommunityAccountAction {
    CreateUser,
    AlterPassword,
    LockAccount,
    UnlockAccount,
    DropUser,
    GrantPrivilege,
    RevokePrivilege,
}

/// `MySQL` privilege scope exposed by the retained Community account UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommunityAccountPrivilegeScope {
    Global,
    Database,
    Table,
}

/// `MySQL` privilege accepted by Community account grant and revoke operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommunityMysqlPrivilege {
    Select,
    Insert,
    Update,
    Delete,
    Create,
    Drop,
    Alter,
    Index,
    References,
    Execute,
    ShowView,
    Trigger,
    Event,
    CreateTemporaryTables,
}

impl CommunityMysqlPrivilege {
    /// Complete privilege allowlist in the order presented by Community.
    pub const ALL: [Self; 14] = [
        Self::Select,
        Self::Insert,
        Self::Update,
        Self::Delete,
        Self::Create,
        Self::Drop,
        Self::Alter,
        Self::Index,
        Self::References,
        Self::Execute,
        Self::ShowView,
        Self::Trigger,
        Self::Event,
        Self::CreateTemporaryTables,
    ];

    /// Community wire value used by capability responses and command payloads.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Select => "SELECT",
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
            Self::Create => "CREATE",
            Self::Drop => "DROP",
            Self::Alter => "ALTER",
            Self::Index => "INDEX",
            Self::References => "REFERENCES",
            Self::Execute => "EXECUTE",
            Self::ShowView => "SHOW_VIEW",
            Self::Trigger => "TRIGGER",
            Self::Event => "EVENT",
            Self::CreateTemporaryTables => "CREATE_TEMPORARY_TABLES",
        }
    }
}

/// Request for grants belonging to one `MySQL` account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountGrantsRequest {
    #[serde(rename = "dataSourceId", alias = "datasourceId")]
    pub datasource_id: String,
    pub user: String,
    pub host: String,
}

/// Preview or execution request for one `MySQL` account operation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountCommandRequest {
    #[serde(rename = "dataSourceId", alias = "datasourceId")]
    pub datasource_id: String,
    pub user: String,
    pub host: String,
    pub action_type: CommunityAccountAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<CommunityAccountPrivilegeScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    #[serde(default)]
    pub privileges: Vec<String>,
    #[serde(default)]
    pub grant_option: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(write_only)]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_token: Option<String>,
}

impl Debug for CommunityAccountCommandRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommunityAccountCommandRequest")
            .field("datasource_id", &self.datasource_id)
            .field("user", &self.user)
            .field("host", &self.host)
            .field("action_type", &self.action_type)
            .field("scope", &self.scope)
            .field("database_name", &self.database_name)
            .field("table_name", &self.table_name)
            .field("privileges", &self.privileges)
            .field("grant_option", &self.grant_option)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field(
                "preview_token",
                &self.preview_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// `MySQL` server and permission capabilities for account administration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountCapability {
    pub db_type: String,
    pub product_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_user: Option<String>,
    pub account_list_readable: bool,
    pub account_lock_supported: bool,
    pub editable_privileges: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// One `MySQL` account projected for the retained Community account tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccount {
    pub user: String,
    pub host: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication_plugin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

/// Stable `MySQL` account collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountList {
    pub items: Vec<CommunityAccount>,
}

/// Stable `SHOW GRANTS` collection for one `MySQL` account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountGrantList {
    pub items: Vec<String>,
}

/// Masked SQL preview and authorization token for one account operation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountPreview {
    pub action_type: CommunityAccountAction,
    pub sql: String,
    pub preview_token: String,
}

impl Debug for CommunityAccountPreview {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommunityAccountPreview")
            .field("action_type", &self.action_type)
            .field("sql", &"[REDACTED]")
            .field("preview_token", &"[REDACTED]")
            .finish()
    }
}

/// Result of executing one preview-authorized `MySQL` account operation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityAccountExecution {
    pub action_type: CommunityAccountAction,
    pub sql: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql_state: Option<String>,
}

impl Debug for CommunityAccountExecution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommunityAccountExecution")
            .field("action_type", &self.action_type)
            .field("sql", &"[REDACTED]")
            .field("success", &self.success)
            .field("message", &self.message.as_ref().map(|_| "[REDACTED]"))
            .field("failure_code", &self.failure_code)
            .field("error_code", &self.error_code)
            .field("sql_state", &self.sql_state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CommunityAccountAction, CommunityAccountCommandRequest, CommunityAccountExecution,
        CommunityAccountPreview, CommunityAccountPrivilegeScope, CommunityMysqlPrivilege,
    };

    #[test]
    fn command_wire_shape_matches_the_retained_frontend() {
        let request = command("pa'ss\\word");

        assert_eq!(
            serde_json::to_value(&request).expect("account command must serialize"),
            json!({
                "dataSourceId": "42",
                "user": "reader",
                "host": "%",
                "actionType": "GRANT_PRIVILEGE",
                "scope": "TABLE",
                "databaseName": "inventory",
                "tableName": "orders",
                "privileges": ["SELECT", "SHOW_VIEW"],
                "grantOption": true,
                "password": "pa'ss\\word",
                "previewToken": "sensitive-preview-token-value"
            })
        );
    }

    #[test]
    fn debug_output_never_exposes_password_tokens_or_sql() {
        let password = "plain-secret";
        let command_debug = format!("{:?}", command(password));
        assert!(!command_debug.contains(password));
        assert!(!command_debug.contains("sensitive-preview-token-value"));
        assert!(command_debug.contains("[REDACTED]"));

        let preview = CommunityAccountPreview {
            action_type: CommunityAccountAction::CreateUser,
            sql: "CREATE USER 'reader'@'%' IDENTIFIED BY 'plain-secret'".to_owned(),
            preview_token: "sensitive-preview-token-value".to_owned(),
        };
        let preview_debug = format!("{preview:?}");
        assert!(!preview_debug.contains("CREATE USER"));
        assert!(!preview_debug.contains("plain-secret"));
        assert!(!preview_debug.contains("sensitive-preview-token-value"));

        let execution = CommunityAccountExecution {
            action_type: CommunityAccountAction::CreateUser,
            sql: preview.sql,
            success: false,
            message: Some("near plain-secret".to_owned()),
            failure_code: Some("mysql.account.executeFailed".to_owned()),
            error_code: Some(1064),
            sql_state: Some("42000".to_owned()),
        };
        let execution_debug = format!("{execution:?}");
        assert!(!execution_debug.contains("CREATE USER"));
        assert!(!execution_debug.contains("plain-secret"));
    }

    #[test]
    fn privilege_allowlist_is_complete_and_stable() {
        assert_eq!(CommunityMysqlPrivilege::ALL.len(), 14);
        assert_eq!(CommunityMysqlPrivilege::ALL[0].wire_name(), "SELECT");
        assert_eq!(CommunityMysqlPrivilege::ALL[10].wire_name(), "SHOW_VIEW");
        assert_eq!(
            CommunityMysqlPrivilege::ALL[13].wire_name(),
            "CREATE_TEMPORARY_TABLES"
        );
    }

    fn command(password: &str) -> CommunityAccountCommandRequest {
        CommunityAccountCommandRequest {
            datasource_id: "42".to_owned(),
            user: "reader".to_owned(),
            host: "%".to_owned(),
            action_type: CommunityAccountAction::GrantPrivilege,
            scope: Some(CommunityAccountPrivilegeScope::Table),
            database_name: Some("inventory".to_owned()),
            table_name: Some("orders".to_owned()),
            privileges: vec!["SELECT".to_owned(), "SHOW_VIEW".to_owned()],
            grant_option: true,
            password: Some(password.to_owned()),
            preview_token: Some("sensitive-preview-token-value".to_owned()),
        }
    }
}
