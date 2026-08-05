//! Adapts retained public API contracts to the database-neutral native driver SPI.

use chat2db_contract::{
    CommunityAccount, CommunityAccountAction, CommunityAccountCapability,
    CommunityAccountCommandRequest, CommunityAccountExecution, CommunityAccountGrantList,
    CommunityAccountGrantsRequest, CommunityAccountList, CommunityAccountPreview,
    CommunityAccountPrivilegeScope, CommunitySchemaDiffEndpoint, CommunitySchemaDiffRequest,
    CommunitySchemaDiffSql,
};

use crate::{
    AppError, Application,
    native_administration_types::{
        AdministrationAction, AdministrationCapability, AdministrationCommand,
        AdministrationExecution, AdministrationPreview, Principal, PrincipalGrantList,
        PrincipalGrantsRequest, PrincipalList, PrincipalRef, PrivilegeScope, PrivilegeTarget,
    },
    native_schema_diff_types::{SchemaDiffEndpoint, SchemaDiffRequest, SchemaDiffSql},
};

impl Application {
    /// Returns the retained account-capability response through the selected native driver.
    ///
    /// # Errors
    ///
    /// Returns datasource-resolution, capability, connection, query, or cleanup failures.
    pub async fn mysql_account_capability(
        &self,
        datasource_id: &str,
    ) -> Result<CommunityAccountCapability, AppError> {
        let driver = self
            .require_native_driver_for_datasource(datasource_id)
            .await?;
        let administration = driver
            .administration()
            .ok_or_else(administration_unavailable)?;
        administration
            .administration_capability(self, datasource_id)
            .await
            .map(community_account_capability)
    }

    /// Lists retained account rows through the selected native driver.
    ///
    /// # Errors
    ///
    /// Returns datasource-resolution, capability, connection, query, or cleanup failures.
    pub async fn list_mysql_accounts(
        &self,
        datasource_id: &str,
    ) -> Result<CommunityAccountList, AppError> {
        let driver = self
            .require_native_driver_for_datasource(datasource_id)
            .await?;
        let administration = driver
            .administration()
            .ok_or_else(administration_unavailable)?;
        administration
            .list_principals(self, datasource_id)
            .await
            .map(community_account_list)
    }

    /// Returns retained grant rows through the selected native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource-resolution, capability, query, or cleanup failures.
    pub async fn mysql_account_grants(
        &self,
        request: &CommunityAccountGrantsRequest,
    ) -> Result<CommunityAccountGrantList, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        let administration = driver
            .administration()
            .ok_or_else(administration_unavailable)?;
        administration
            .principal_grants(self, &principal_grants_request(request))
            .await
            .map(community_account_grants)
    }

    /// Builds the retained `MySQL` account preview through the native dialect capability.
    ///
    /// # Errors
    ///
    /// Returns capability or account-command validation failures.
    pub fn preview_mysql_account(
        &self,
        request: &CommunityAccountCommandRequest,
    ) -> Result<CommunityAccountPreview, AppError> {
        let driver = self
            .native_driver_for_database_type("MYSQL")
            .ok_or_else(administration_unavailable)?;
        let administration = driver
            .administration()
            .ok_or_else(administration_unavailable)?;
        administration
            .preview_administration(self, &administration_command(request))
            .map(community_account_preview)
    }

    /// Executes the retained `MySQL` account command through the selected native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, datasource, connection, or cleanup failures.
    pub async fn execute_mysql_account(
        &self,
        request: &CommunityAccountCommandRequest,
    ) -> Result<CommunityAccountExecution, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        let administration = driver
            .administration()
            .ok_or_else(administration_unavailable)?;
        administration
            .execute_administration(self, &administration_command(request))
            .await
            .map(community_account_execution)
    }

    /// Builds the retained schema-diff response through the source datasource's native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, driver-selection, metadata, resource-limit, or cleanup failures.
    pub async fn preview_mysql_schema_diff(
        &self,
        request: &CommunitySchemaDiffRequest,
    ) -> Result<CommunitySchemaDiffSql, AppError> {
        let request = schema_diff_request(request);
        validate_schema_diff_selection(&request)?;

        let source_driver = self
            .require_native_driver_for_datasource(&request.source.datasource_id)
            .await?;
        let target_driver = self
            .require_native_driver_for_datasource(&request.target.datasource_id)
            .await?;
        if !source_driver.id().eq_ignore_ascii_case(target_driver.id()) {
            return Err(AppError::invalid(
                "invalid_community_schema_diff_request",
                "Source and target datasources must use the same native driver",
            ));
        }
        let schema_diff = source_driver
            .schema_diff()
            .ok_or_else(schema_diff_unavailable)?;
        schema_diff
            .preview_schema_diff(self, &request)
            .await
            .map(community_schema_diff_sql)
            .map_err(community_schema_diff_error)
    }
}

fn administration_unavailable() -> AppError {
    AppError::invalid(
        "native_administration_capability_not_available",
        "The native Rust driver does not implement database administration",
    )
}

fn schema_diff_unavailable() -> AppError {
    AppError::invalid(
        "native_schema_diff_capability_not_available",
        "The native Rust driver does not implement schema comparison",
    )
}

fn administration_action(action: CommunityAccountAction) -> AdministrationAction {
    match action {
        CommunityAccountAction::CreateUser => AdministrationAction::CreatePrincipal,
        CommunityAccountAction::AlterPassword => AdministrationAction::AlterCredential,
        CommunityAccountAction::LockAccount => AdministrationAction::LockPrincipal,
        CommunityAccountAction::UnlockAccount => AdministrationAction::UnlockPrincipal,
        CommunityAccountAction::DropUser => AdministrationAction::DropPrincipal,
        CommunityAccountAction::GrantPrivilege => AdministrationAction::GrantPrivileges,
        CommunityAccountAction::RevokePrivilege => AdministrationAction::RevokePrivileges,
    }
}

fn community_account_action(action: AdministrationAction) -> CommunityAccountAction {
    match action {
        AdministrationAction::CreatePrincipal => CommunityAccountAction::CreateUser,
        AdministrationAction::AlterCredential => CommunityAccountAction::AlterPassword,
        AdministrationAction::LockPrincipal => CommunityAccountAction::LockAccount,
        AdministrationAction::UnlockPrincipal => CommunityAccountAction::UnlockAccount,
        AdministrationAction::DropPrincipal => CommunityAccountAction::DropUser,
        AdministrationAction::GrantPrivileges => CommunityAccountAction::GrantPrivilege,
        AdministrationAction::RevokePrivileges => CommunityAccountAction::RevokePrivilege,
    }
}

fn principal(user: &str, host: &str) -> PrincipalRef {
    PrincipalRef {
        name: user.to_owned(),
        qualifier: Some(host.to_owned()),
    }
}

fn principal_grants_request(request: &CommunityAccountGrantsRequest) -> PrincipalGrantsRequest {
    PrincipalGrantsRequest {
        datasource_id: request.datasource_id.clone(),
        principal: principal(&request.user, &request.host),
    }
}

fn administration_command(request: &CommunityAccountCommandRequest) -> AdministrationCommand {
    AdministrationCommand {
        datasource_id: request.datasource_id.clone(),
        principal: principal(&request.user, &request.host),
        action: administration_action(request.action_type),
        target: request.scope.map(|scope| PrivilegeTarget {
            scope: match scope {
                CommunityAccountPrivilegeScope::Global => PrivilegeScope::Global,
                CommunityAccountPrivilegeScope::Database => PrivilegeScope::Database,
                CommunityAccountPrivilegeScope::Table => PrivilegeScope::Table,
            },
            database_name: request.database_name.clone(),
            schema_name: None,
            object_name: request.table_name.clone(),
        }),
        privileges: request.privileges.clone(),
        grant_option: request.grant_option,
        credential: request.password.clone(),
        preview_token: request.preview_token.clone(),
    }
}

fn community_account_capability(
    capability: AdministrationCapability,
) -> CommunityAccountCapability {
    CommunityAccountCapability {
        db_type: capability.database_type,
        product_name: capability.product_name,
        product_version: capability.product_version,
        current_user: capability.current_principal,
        connection_user: capability.connection_principal,
        account_list_readable: capability.principal_list_readable,
        account_lock_supported: capability.principal_lock_supported,
        editable_privileges: capability.editable_privileges,
        message: capability.message,
    }
}

fn community_account(principal: Principal) -> CommunityAccount {
    CommunityAccount {
        user: principal.name,
        host: principal.qualifier.unwrap_or_default(),
        display_name: principal.display_name,
        authentication_plugin: principal.authentication_method,
        locked: principal.locked,
    }
}

fn community_account_list(accounts: PrincipalList) -> CommunityAccountList {
    CommunityAccountList {
        items: accounts.items.into_iter().map(community_account).collect(),
    }
}

fn community_account_grants(grants: PrincipalGrantList) -> CommunityAccountGrantList {
    CommunityAccountGrantList {
        items: grants.items,
    }
}

fn community_account_preview(preview: AdministrationPreview) -> CommunityAccountPreview {
    CommunityAccountPreview {
        action_type: community_account_action(preview.action),
        sql: preview.sql,
        preview_token: preview.preview_token,
    }
}

fn community_account_execution(execution: AdministrationExecution) -> CommunityAccountExecution {
    CommunityAccountExecution {
        action_type: community_account_action(execution.action),
        sql: execution.sql,
        success: execution.success,
        message: execution.message,
        failure_code: execution.failure_code,
        error_code: execution.error_code,
        sql_state: execution.sql_state,
    }
}

fn schema_diff_endpoint(endpoint: &CommunitySchemaDiffEndpoint) -> SchemaDiffEndpoint {
    SchemaDiffEndpoint {
        datasource_id: endpoint.datasource_id.clone(),
        database_name: endpoint.database_name.clone(),
        schema_name: endpoint.schema_name.clone(),
    }
}

fn schema_diff_request(request: &CommunitySchemaDiffRequest) -> SchemaDiffRequest {
    SchemaDiffRequest {
        source: schema_diff_endpoint(&request.source),
        target: schema_diff_endpoint(&request.target),
    }
}

fn validate_schema_diff_selection(request: &SchemaDiffRequest) -> Result<(), AppError> {
    for (role, endpoint) in [("source", &request.source), ("target", &request.target)] {
        if endpoint.datasource_id.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_community_schema_diff_request",
                format!("The {role} datasource id is required"),
            ));
        }
        if endpoint.database_name.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_community_schema_diff_request",
                format!("The {role} database name is required"),
            ));
        }
        if endpoint.database_name.contains('\0') {
            return Err(AppError::invalid(
                "invalid_community_schema_diff_request",
                format!("The {role} database name cannot contain NUL"),
            ));
        }
    }
    Ok(())
}

fn community_schema_diff_sql(sql: SchemaDiffSql) -> CommunitySchemaDiffSql {
    CommunitySchemaDiffSql::new(sql.into_inner())
}

fn community_schema_diff_error(error: AppError) -> AppError {
    let api = error.api_error();
    if api.code == "invalid_schema_diff_request" {
        AppError::invalid("invalid_community_schema_diff_request", api.message)
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{CommunityAccountAction, CommunityAccountCommandRequest};

    use super::{administration_command, community_account_action};

    #[test]
    fn account_actions_round_trip_across_the_compatibility_boundary() {
        for action in [
            CommunityAccountAction::CreateUser,
            CommunityAccountAction::AlterPassword,
            CommunityAccountAction::LockAccount,
            CommunityAccountAction::UnlockAccount,
            CommunityAccountAction::DropUser,
            CommunityAccountAction::GrantPrivilege,
            CommunityAccountAction::RevokePrivilege,
        ] {
            let request = CommunityAccountCommandRequest {
                datasource_id: "datasource-1".to_owned(),
                user: "reader".to_owned(),
                host: "%".to_owned(),
                action_type: action,
                scope: None,
                database_name: None,
                table_name: None,
                privileges: Vec::new(),
                grant_option: false,
                password: None,
                preview_token: None,
            };
            assert_eq!(
                community_account_action(administration_command(&request).action),
                action
            );
        }
    }
}
