use std::fmt::{Debug, Formatter};

/// Database-neutral administration operation exposed by a native driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdministrationAction {
    CreatePrincipal,
    AlterCredential,
    LockPrincipal,
    UnlockPrincipal,
    DropPrincipal,
    GrantPrivileges,
    RevokePrivileges,
}

/// Identifies a database principal without imposing one database's identity model.
///
/// `MySQL` uses `name@qualifier` where the qualifier is the host. Databases whose
/// principals have no qualifier leave it unset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrincipalRef {
    pub(crate) name: String,
    pub(crate) qualifier: Option<String>,
}

/// Database-neutral privilege target category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivilegeScope {
    Global,
    Database,
    #[allow(
        dead_code,
        reason = "reserved for PostgreSQL and other schema-scoped administration drivers"
    )]
    Schema,
    Table,
}

/// Target of a grant or revoke operation.
///
/// Optional path segments let each driver validate the path required by its
/// own privilege model without leaking that model into the SPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrivilegeTarget {
    pub(crate) scope: PrivilegeScope,
    pub(crate) database_name: Option<String>,
    pub(crate) schema_name: Option<String>,
    pub(crate) object_name: Option<String>,
}

/// Input for previewing or executing one administration operation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AdministrationCommand {
    pub(crate) datasource_id: String,
    pub(crate) principal: PrincipalRef,
    pub(crate) action: AdministrationAction,
    pub(crate) target: Option<PrivilegeTarget>,
    pub(crate) privileges: Vec<String>,
    pub(crate) grant_option: bool,
    pub(crate) credential: Option<String>,
    pub(crate) preview_token: Option<String>,
}

impl Debug for AdministrationCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdministrationCommand")
            .field("datasource_id", &self.datasource_id)
            .field("principal", &self.principal)
            .field("action", &self.action)
            .field("target", &self.target)
            .field("privileges", &self.privileges)
            .field("grant_option", &self.grant_option)
            .field(
                "credential",
                &self.credential.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "preview_token",
                &self.preview_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Input for listing the grants assigned to one principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrincipalGrantsRequest {
    pub(crate) datasource_id: String,
    pub(crate) principal: PrincipalRef,
}

/// Server and permission capabilities for native administration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdministrationCapability {
    pub(crate) database_type: String,
    pub(crate) product_name: String,
    pub(crate) product_version: Option<String>,
    pub(crate) current_principal: Option<String>,
    pub(crate) connection_principal: Option<String>,
    pub(crate) principal_list_readable: bool,
    pub(crate) principal_lock_supported: bool,
    pub(crate) editable_privileges: Vec<String>,
    pub(crate) message: Option<String>,
}

/// One principal projected by a native administration driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Principal {
    pub(crate) name: String,
    pub(crate) qualifier: Option<String>,
    pub(crate) display_name: String,
    pub(crate) authentication_method: Option<String>,
    pub(crate) locked: Option<bool>,
}

/// Stable principal collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrincipalList {
    pub(crate) items: Vec<Principal>,
}

/// Stable collection of grants returned by a native administration driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrincipalGrantList {
    pub(crate) items: Vec<String>,
}

/// Masked SQL preview and authorization token for one administration operation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AdministrationPreview {
    pub(crate) action: AdministrationAction,
    pub(crate) sql: String,
    pub(crate) preview_token: String,
}

impl Debug for AdministrationPreview {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdministrationPreview")
            .field("action", &self.action)
            .field("sql", &"[REDACTED]")
            .field("preview_token", &"[REDACTED]")
            .finish()
    }
}

/// Result of executing one preview-authorized administration operation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AdministrationExecution {
    pub(crate) action: AdministrationAction,
    pub(crate) sql: String,
    pub(crate) success: bool,
    pub(crate) message: Option<String>,
    pub(crate) failure_code: Option<String>,
    pub(crate) error_code: Option<u16>,
    pub(crate) sql_state: Option<String>,
}

impl Debug for AdministrationExecution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdministrationExecution")
            .field("action", &self.action)
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
    use super::{
        AdministrationAction, AdministrationCommand, AdministrationExecution,
        AdministrationPreview, PrincipalRef,
    };

    #[test]
    fn sensitive_administration_values_are_redacted_from_debug_output() {
        let command = AdministrationCommand {
            datasource_id: "42".to_owned(),
            principal: PrincipalRef {
                name: "reader".to_owned(),
                qualifier: Some("%".to_owned()),
            },
            action: AdministrationAction::CreatePrincipal,
            target: None,
            privileges: Vec::new(),
            grant_option: false,
            credential: Some("plain-secret".to_owned()),
            preview_token: Some("sensitive-token".to_owned()),
        };
        let command_debug = format!("{command:?}");
        assert!(!command_debug.contains("plain-secret"));
        assert!(!command_debug.contains("sensitive-token"));

        let preview = AdministrationPreview {
            action: AdministrationAction::CreatePrincipal,
            sql: "CREATE USER reader IDENTIFIED BY plain-secret".to_owned(),
            preview_token: "sensitive-token".to_owned(),
        };
        let preview_debug = format!("{preview:?}");
        assert!(!preview_debug.contains("CREATE USER"));
        assert!(!preview_debug.contains("sensitive-token"));

        let execution = AdministrationExecution {
            action: AdministrationAction::CreatePrincipal,
            sql: preview.sql,
            success: false,
            message: Some("near plain-secret".to_owned()),
            failure_code: Some("driver.execute_failed".to_owned()),
            error_code: Some(1064),
            sql_state: Some("42000".to_owned()),
        };
        let execution_debug = format!("{execution:?}");
        assert!(!execution_debug.contains("CREATE USER"));
        assert!(!execution_debug.contains("plain-secret"));
    }
}
