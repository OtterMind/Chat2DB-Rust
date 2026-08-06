use std::{
    collections::HashMap,
    future::Future,
    sync::Mutex as StdMutex,
    time::{Duration, Instant},
};

use chat2db_contract::{ApiError, DatasourceConnection};
use mysql_async::{Conn, Error as MysqlError, prelude::Queryable};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    AppError, AppErrorKind, Application,
    native_administration_types::{
        AdministrationAction, AdministrationCapability, AdministrationCommand,
        AdministrationExecution, AdministrationPreview, Principal, PrincipalGrantList,
        PrincipalGrantsRequest, PrincipalList, PrincipalRef, PrivilegeScope,
    },
    native_mysql::{finish_connection, open_resolved_connection, resolve_native_connection},
};

const ACCOUNT_QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const ACCOUNT_EXECUTE_FAILED: &str = "mysql.account.executeFailed";
const ACCOUNT_EXECUTE_OUTCOME_UNKNOWN: &str = "mysql.account.outcomeUnknown";
const ACCOUNT_LIST_UNAVAILABLE: &str = "mysql.account.listUnavailable";
const ACCOUNT_GRANTS_UNAVAILABLE: &str = "mysql.account.grantsUnavailable";
const ACCOUNT_PREVIEW_TOKEN_MISMATCH: &str = "mysql.account.previewTokenMismatch";
const ACCOUNT_PREVIEW_UNAVAILABLE: &str = "mysql.account.previewUnavailable";
const MASKED_PASSWORD_LITERAL: &str = "'******'";
const ACCOUNT_PREVIEW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PENDING_ACCOUNT_PREVIEWS: usize = 256;

const SELECT_CURRENT_ACCOUNT: &str = "SELECT VERSION(), CURRENT_USER()";
const PROBE_ACCOUNT_LIST: &str = "SELECT User, Host FROM mysql.user LIMIT 1";
const PROBE_ACCOUNT_LOCK: &str = "SELECT account_locked FROM mysql.user LIMIT 1";
const SELECT_ACCOUNTS: &str = "SELECT User, Host, plugin FROM mysql.user ORDER BY User, Host";
const SELECT_ACCOUNTS_WITH_LOCK: &str =
    "SELECT User, Host, plugin, account_locked FROM mysql.user ORDER BY User, Host";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MysqlPrivilege {
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

impl MysqlPrivilege {
    const ALL: [Self; 14] = [
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

    const fn wire_name(self) -> &'static str {
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

enum AccountQueryFailure {
    Timeout,
    Mysql(MysqlError),
}

#[derive(Default)]
pub(crate) struct AccountPreviewRegistry {
    pending: StdMutex<HashMap<[u8; 32], AccountPreviewBinding>>,
}

struct AccountPreviewBinding {
    datasource_id: String,
    sql_sha256: [u8; 32],
    expires_at: Instant,
}

impl AccountPreviewRegistry {
    fn issue(&self, datasource_id: &str, sql: &str) -> Result<String, AppError> {
        let now = Instant::now();
        let mut pending = self.pending.lock().map_err(|_| {
            AppError::unavailable(
                ACCOUNT_PREVIEW_UNAVAILABLE,
                "MySQL account preview authorization is unavailable",
            )
        })?;
        pending.retain(|_, binding| binding.expires_at > now);
        if pending.len() >= MAX_PENDING_ACCOUNT_PREVIEWS {
            return Err(AppError::unavailable(
                ACCOUNT_PREVIEW_UNAVAILABLE,
                "Too many MySQL account previews are pending",
            ));
        }

        let sql_sha256 = sha256(sql.as_bytes());
        loop {
            let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            let token_sha256 = sha256(token.as_bytes());
            if pending.contains_key(&token_sha256) {
                continue;
            }
            pending.insert(
                token_sha256,
                AccountPreviewBinding {
                    datasource_id: datasource_id.to_owned(),
                    sql_sha256,
                    expires_at: now + ACCOUNT_PREVIEW_TTL,
                },
            );
            return Ok(token);
        }
    }

    fn consume(&self, token: &str, datasource_id: &str, sql: &str) -> bool {
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
        let token_sha256 = sha256(token.as_bytes());
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let Some(binding) = pending.remove(&token_sha256) else {
            return false;
        };
        binding.expires_at > Instant::now()
            && binding.datasource_id == datasource_id
            && binding.sql_sha256 == sha256(sql.as_bytes())
    }

    fn authorizes(&self, token: &str, datasource_id: &str, sql: &str) -> bool {
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
        let token_sha256 = sha256(token.as_bytes());
        let Ok(pending) = self.pending.lock() else {
            return false;
        };
        let Some(binding) = pending.get(&token_sha256) else {
            return false;
        };
        binding.expires_at > Instant::now()
            && binding.datasource_id == datasource_id
            && binding.sql_sha256 == sha256(sql.as_bytes())
    }
}

/// Returns `MySQL` account-administration capability for one datasource.
///
/// # Errors
///
/// Returns datasource, secret, driver, connection, or cleanup errors.
pub(crate) async fn mysql_account_capability(
    application: &Application,
    datasource_id: &str,
) -> Result<AdministrationCapability, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let connection_user = configured_connection_user(&resolved.connection);
    let mut conn = open_resolved_connection(&resolved).await?;

    let account_list_readable = match timed_query(conn.query_drop(PROBE_ACCOUNT_LIST)).await {
        Ok(()) => true,
        Err(AccountQueryFailure::Mysql(_)) => false,
        Err(AccountQueryFailure::Timeout) => {
            return finish_connection(
                conn,
                Ok(capability_with_message(
                    connection_user,
                    false,
                    false,
                    "The MySQL account capability query timed out",
                )),
            )
            .await;
        }
    };
    let account_lock_supported = match timed_query(conn.query_drop(PROBE_ACCOUNT_LOCK)).await {
        Ok(()) => true,
        Err(AccountQueryFailure::Mysql(_)) => false,
        Err(AccountQueryFailure::Timeout) => {
            return finish_connection(
                conn,
                Ok(capability_with_message(
                    connection_user,
                    account_list_readable,
                    false,
                    "The MySQL account capability query timed out",
                )),
            )
            .await;
        }
    };

    let (product_version, current_user, message) =
        match timed_query(conn.query_first::<(String, String), _>(SELECT_CURRENT_ACCOUNT)).await {
            Ok(Some((version, current_user))) => (Some(version), Some(current_user), None),
            Ok(None) => (None, None, None),
            Err(AccountQueryFailure::Timeout) => (
                None,
                None,
                Some("The MySQL account capability query timed out".to_owned()),
            ),
            Err(AccountQueryFailure::Mysql(error)) => {
                (None, None, Some(safe_query_message(&error)))
            }
        };
    finish_connection(
        conn,
        Ok(AdministrationCapability {
            database_type: "MYSQL".to_owned(),
            product_name: "MySQL".to_owned(),
            product_version,
            current_principal: current_user,
            connection_principal: connection_user,
            principal_list_readable: account_list_readable,
            principal_lock_supported: account_lock_supported,
            editable_privileges: MysqlPrivilege::ALL
                .into_iter()
                .map(|privilege| privilege.wire_name().to_owned())
                .collect(),
            message,
        }),
    )
    .await
}

/// Lists `MySQL` accounts in stable user and host order.
///
/// # Errors
///
/// Returns datasource, connection, permission, query, or cleanup errors.
pub(crate) async fn list_mysql_accounts(
    application: &Application,
    datasource_id: &str,
) -> Result<PrincipalList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let with_lock = timed_query(
        conn.query::<(String, String, Option<String>, Option<String>), _>(
            SELECT_ACCOUNTS_WITH_LOCK,
        ),
    )
    .await;
    let result = match with_lock {
        Ok(rows) => Ok(PrincipalList {
            items: rows
                .into_iter()
                .map(|(user, host, plugin, locked)| account(user, host, plugin, locked.as_deref()))
                .collect(),
        }),
        Err(AccountQueryFailure::Timeout) => Err(account_query_unavailable(
            ACCOUNT_LIST_UNAVAILABLE,
            "The MySQL account list query timed out",
        )),
        Err(AccountQueryFailure::Mysql(_)) => {
            match timed_query(conn.query::<(String, String, Option<String>), _>(SELECT_ACCOUNTS))
                .await
            {
                Ok(rows) => Ok(PrincipalList {
                    items: rows
                        .into_iter()
                        .map(|(user, host, plugin)| account(user, host, plugin, None))
                        .collect(),
                }),
                Err(_) => Err(account_query_unavailable(
                    ACCOUNT_LIST_UNAVAILABLE,
                    "The MySQL account list is unavailable",
                )),
            }
        }
    };
    finish_connection(conn, result).await
}

/// Returns `SHOW GRANTS` rows for one `MySQL` account.
///
/// # Errors
///
/// Returns validation, datasource, connection, permission, query, or cleanup errors.
pub(crate) async fn mysql_account_grants(
    application: &Application,
    request: &PrincipalGrantsRequest,
) -> Result<PrincipalGrantList, AppError> {
    let account = account_literal(&request.principal)?;
    let resolved = resolve_native_connection(application, &request.datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let sql = format!("SHOW GRANTS FOR {account}");
    let result = match timed_query(query_account_grants(&mut conn, &sql)).await {
        Ok(items) => Ok(PrincipalGrantList { items }),
        Err(_) => Err(account_query_unavailable(
            ACCOUNT_GRANTS_UNAVAILABLE,
            "The MySQL grants are unavailable",
        )),
    };
    finish_connection(conn, result).await
}

/// Builds a masked account-operation preview without opening `MySQL` or starting Java.
///
/// # Errors
///
/// Returns a field-specific account validation error.
pub(crate) fn preview_mysql_account(
    application: &Application,
    request: &AdministrationCommand,
) -> Result<AdministrationPreview, AppError> {
    preview_account(&application.inner.account_previews, request)
}

/// Executes one preview-authorized `MySQL` account operation through `mysql_async`.
///
/// SQL execution errors are returned in [`AdministrationExecution`]. Datasource,
/// validation, preview-token, connection, read-only, and cleanup failures remain errors.
///
/// # Errors
///
/// Returns validation, token, datasource, connection, read-only, or cleanup errors.
pub(crate) async fn execute_mysql_account(
    application: &Application,
    request: &AdministrationCommand,
) -> Result<AdministrationExecution, AppError> {
    let execution_sql =
        authorize_account_preview(&application.inner.account_previews, request, true)?;
    let preview_sql = build_account_sql(request, true)?;

    let resolved = resolve_native_connection(application, &request.datasource_id).await?;
    if resolved.connection.read_only {
        return Err(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "datasource_read_only",
                "The datasource connection is configured as read-only",
            ),
        ));
    }
    let mut conn = open_resolved_connection(&resolved).await?;
    let query_result = timed_query(execute_account_sql(&mut conn, &execution_sql)).await;
    drop(execution_sql);

    let response = match query_result {
        Ok(()) => AdministrationExecution {
            action: request.action,
            sql: preview_sql.clone(),
            success: true,
            message: Some("OK".to_owned()),
            failure_code: None,
            error_code: None,
            sql_state: None,
        },
        Err(AccountQueryFailure::Mysql(MysqlError::Server(server))) => {
            AdministrationExecution {
                action: request.action,
                sql: preview_sql.clone(),
                success: false,
                message: Some(redact_password(
                    &server.message,
                    request.credential.as_deref(),
                )),
                failure_code: Some(ACCOUNT_EXECUTE_FAILED.to_owned()),
                error_code: Some(server.code),
                sql_state: Some(server.state),
            }
        }
        Err(AccountQueryFailure::Mysql(_)) => account_outcome_unknown(
            request.action,
            preview_sql.clone(),
            "The MySQL connection ended after dispatch; the account-operation outcome is unknown and must not be retried blindly".to_owned(),
        ),
        Err(AccountQueryFailure::Timeout) => account_outcome_unknown(
            request.action,
            preview_sql,
            "The MySQL account operation timed out after dispatch; its outcome is unknown and must not be retried blindly".to_owned(),
        ),
    };
    if let Err(error) = finish_connection(conn, Ok(())).await {
        tracing::warn!(
            error = %error,
            "native MySQL account connection cleanup failed after a settled operation"
        );
    }
    Ok(response)
}

fn account_outcome_unknown(
    action: AdministrationAction,
    sql: String,
    message: String,
) -> AdministrationExecution {
    AdministrationExecution {
        action,
        sql,
        success: false,
        message: Some(message),
        failure_code: Some(ACCOUNT_EXECUTE_OUTCOME_UNKNOWN.to_owned()),
        error_code: None,
        sql_state: None,
    }
}

fn preview_account(
    registry: &AccountPreviewRegistry,
    request: &AdministrationCommand,
) -> Result<AdministrationPreview, AppError> {
    let execution_sql = build_account_sql(request, false)?;
    let sql = build_account_sql(request, true)?;
    let preview_token = registry.issue(&request.datasource_id, &execution_sql)?;
    drop(execution_sql);
    Ok(AdministrationPreview {
        action: request.action,
        sql,
        preview_token,
    })
}

fn authorize_account_preview(
    registry: &AccountPreviewRegistry,
    request: &AdministrationCommand,
    consume: bool,
) -> Result<String, AppError> {
    let execution_sql = build_account_sql(request, false)?;
    let supplied_token = request.preview_token.as_deref().unwrap_or_default();
    let authorized = if consume {
        registry.consume(supplied_token, &request.datasource_id, &execution_sql)
    } else {
        registry.authorizes(supplied_token, &request.datasource_id, &execution_sql)
    };
    if !authorized {
        return Err(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                ACCOUNT_PREVIEW_TOKEN_MISMATCH,
                "The MySQL account preview token does not match this operation",
            ),
        ));
    }
    Ok(execution_sql)
}

fn build_account_sql(
    request: &AdministrationCommand,
    mask_sensitive: bool,
) -> Result<String, AppError> {
    let account = account_literal(&request.principal)?;
    match request.action {
        AdministrationAction::CreatePrincipal => Ok(format!(
            "CREATE USER {account} IDENTIFIED BY {}",
            password_literal(request, mask_sensitive)?
        )),
        AdministrationAction::AlterCredential => Ok(format!(
            "ALTER USER {account} IDENTIFIED BY {}",
            password_literal(request, mask_sensitive)?
        )),
        AdministrationAction::LockPrincipal => Ok(format!("ALTER USER {account} ACCOUNT LOCK")),
        AdministrationAction::UnlockPrincipal => Ok(format!("ALTER USER {account} ACCOUNT UNLOCK")),
        AdministrationAction::DropPrincipal => Ok(format!("DROP USER {account}")),
        AdministrationAction::GrantPrivileges => {
            let privileges = privilege_list(&request.privileges)?;
            let scope = privilege_scope(request)?;
            let grant_option = if request.grant_option {
                " WITH GRANT OPTION"
            } else {
                ""
            };
            Ok(format!(
                "GRANT {privileges} ON {scope} TO {account}{grant_option}"
            ))
        }
        AdministrationAction::RevokePrivileges => {
            let privileges = privilege_list(&request.privileges)?;
            let scope = privilege_scope(request)?;
            Ok(format!("REVOKE {privileges} ON {scope} FROM {account}"))
        }
    }
}

fn password_literal(
    request: &AdministrationCommand,
    mask_sensitive: bool,
) -> Result<String, AppError> {
    let password = request
        .credential
        .as_deref()
        .filter(|value| !is_blank(value));
    if password.is_none() {
        return Err(account_validation_error(
            "mysql.account.passwordRequired",
            "A non-blank password is required",
        ));
    }
    if mask_sensitive {
        Ok(MASKED_PASSWORD_LITERAL.to_owned())
    } else {
        Ok(string_literal(password.unwrap_or_default()))
    }
}

fn account_literal(principal: &PrincipalRef) -> Result<String, AppError> {
    let user = principal.name.as_str();
    let host = principal.qualifier.as_deref().unwrap_or_default();
    validate_account_part(
        user,
        "mysql.account.userRequired",
        "A non-blank MySQL account user is required",
    )?;
    validate_account_part(
        host,
        "mysql.account.hostRequired",
        "A non-blank MySQL account host is required",
    )?;
    Ok(format!("{}@{}", string_literal(user), string_literal(host)))
}

fn validate_account_part(
    value: &str,
    code: &'static str,
    message: &'static str,
) -> Result<(), AppError> {
    if is_blank(value) {
        return Err(account_validation_error(code, message));
    }
    if value.contains('\0') {
        return Err(account_validation_error(
            "mysql.account.invalidAccountName",
            "MySQL account user and host names cannot contain NUL",
        ));
    }
    Ok(())
}

fn privilege_scope(request: &AdministrationCommand) -> Result<String, AppError> {
    let Some(target) = request.target.as_ref() else {
        return Err(account_validation_error(
            "mysql.account.scopeRequired",
            "A privilege scope is required",
        ));
    };
    match target.scope {
        PrivilegeScope::Global => Ok("*.*".to_owned()),
        PrivilegeScope::Database => {
            let database = required_identifier(
                target.database_name.as_deref(),
                "mysql.account.databaseRequired",
                "A database name is required for database privileges",
            )?;
            Ok(format!("{database}.*"))
        }
        PrivilegeScope::Table => {
            let database = required_identifier(
                target.database_name.as_deref(),
                "mysql.account.databaseRequired",
                "A database name is required for table privileges",
            )?;
            let table = required_identifier(
                target.object_name.as_deref(),
                "mysql.account.tableRequired",
                "A table name is required for table privileges",
            )?;
            Ok(format!("{database}.{table}"))
        }
        PrivilegeScope::Schema => Err(account_validation_error(
            "mysql.account.scopeUnsupported",
            "MySQL does not support schema-scoped account privileges",
        )),
    }
}

fn required_identifier(
    value: Option<&str>,
    code: &'static str,
    message: &'static str,
) -> Result<String, AppError> {
    let value = value.filter(|value| !is_blank(value));
    match value {
        Some(value) => Ok(identifier(value)),
        None => Err(account_validation_error(code, message)),
    }
}

fn identifier(value: &str) -> String {
    format!("`{}`", value.replace('`', "``"))
}

fn string_literal(value: &str) -> String {
    let mut literal = String::with_capacity(value.len() + 2);
    literal.push('\'');
    for character in value.chars() {
        match character {
            '\'' => literal.push_str("''"),
            _ => literal.push(character),
        }
    }
    literal.push('\'');
    literal
}

async fn query_account_grants(conn: &mut Conn, sql: &str) -> Result<Vec<String>, MysqlError> {
    enforce_mode_independent_account_literals(conn).await?;
    conn.query(sql).await
}

async fn execute_account_sql(conn: &mut Conn, sql: &str) -> Result<(), MysqlError> {
    enforce_mode_independent_account_literals(conn).await?;
    conn.query_drop(sql).await
}

async fn enforce_mode_independent_account_literals(conn: &mut Conn) -> Result<(), MysqlError> {
    let current = conn
        .query_first::<String, _>("SELECT @@SESSION.sql_mode")
        .await?
        .unwrap_or_default();
    let Some(required) = sql_mode_with_no_backslash_escapes(&current) else {
        return Ok(());
    };
    conn.exec_drop("SET SESSION sql_mode = ?", (required,))
        .await
}

fn sql_mode_with_no_backslash_escapes(current: &str) -> Option<String> {
    if current
        .split(',')
        .any(|mode| mode.trim().eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
    {
        return None;
    }
    let current = current.trim();
    Some(if current.is_empty() {
        "NO_BACKSLASH_ESCAPES".to_owned()
    } else {
        format!("{current},NO_BACKSLASH_ESCAPES")
    })
}

fn privilege_list(privileges: &[String]) -> Result<String, AppError> {
    if privileges.is_empty() {
        return Err(account_validation_error(
            "mysql.account.privilegeRequired",
            "At least one MySQL privilege is required",
        ));
    }
    let mut accepted = Vec::new();
    for privilege in privileges {
        let privilege = parse_privilege(privilege)?;
        if !accepted.contains(&privilege) {
            accepted.push(privilege);
        }
    }
    if accepted.is_empty() {
        return Err(account_validation_error(
            "mysql.account.privilegeRequired",
            "At least one MySQL privilege is required",
        ));
    }
    Ok(accepted
        .into_iter()
        .map(privilege_sql_name)
        .collect::<Vec<_>>()
        .join(", "))
}

fn parse_privilege(value: &str) -> Result<MysqlPrivilege, AppError> {
    match value.trim().to_ascii_uppercase().as_str() {
        "SELECT" => Ok(MysqlPrivilege::Select),
        "INSERT" => Ok(MysqlPrivilege::Insert),
        "UPDATE" => Ok(MysqlPrivilege::Update),
        "DELETE" => Ok(MysqlPrivilege::Delete),
        "CREATE" => Ok(MysqlPrivilege::Create),
        "DROP" => Ok(MysqlPrivilege::Drop),
        "ALTER" => Ok(MysqlPrivilege::Alter),
        "INDEX" => Ok(MysqlPrivilege::Index),
        "REFERENCES" => Ok(MysqlPrivilege::References),
        "EXECUTE" => Ok(MysqlPrivilege::Execute),
        "SHOW_VIEW" => Ok(MysqlPrivilege::ShowView),
        "TRIGGER" => Ok(MysqlPrivilege::Trigger),
        "EVENT" => Ok(MysqlPrivilege::Event),
        "CREATE_TEMPORARY_TABLES" => Ok(MysqlPrivilege::CreateTemporaryTables),
        _ => Err(account_validation_error(
            "mysql.account.privilegeUnsupported",
            "The requested MySQL privilege is not supported",
        )),
    }
}

const fn privilege_sql_name(privilege: MysqlPrivilege) -> &'static str {
    match privilege {
        MysqlPrivilege::ShowView => "SHOW VIEW",
        MysqlPrivilege::CreateTemporaryTables => "CREATE TEMPORARY TABLES",
        other => other.wire_name(),
    }
}

fn sha256(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

fn account(
    user: String,
    host: String,
    authentication_plugin: Option<String>,
    locked: Option<&str>,
) -> Principal {
    Principal {
        display_name: format!("{user}@{host}"),
        name: user,
        qualifier: Some(host),
        authentication_method: authentication_plugin,
        locked: locked
            .and_then(|value| (!value.trim().is_empty()).then(|| value.eq_ignore_ascii_case("Y"))),
    }
}

fn configured_connection_user(connection: &DatasourceConnection) -> Option<String> {
    connection
        .properties
        .iter()
        .find(|property| {
            property.key.eq_ignore_ascii_case("user")
                || property.key.eq_ignore_ascii_case("username")
        })
        .map(|property| property.value.clone())
        .filter(|value| !value.trim().is_empty())
}

fn capability_with_message(
    connection_user: Option<String>,
    account_list_readable: bool,
    account_lock_supported: bool,
    message: &str,
) -> AdministrationCapability {
    AdministrationCapability {
        database_type: "MYSQL".to_owned(),
        product_name: "MySQL".to_owned(),
        product_version: None,
        current_principal: None,
        connection_principal: connection_user,
        principal_list_readable: account_list_readable,
        principal_lock_supported: account_lock_supported,
        editable_privileges: MysqlPrivilege::ALL
            .into_iter()
            .map(|privilege| privilege.wire_name().to_owned())
            .collect(),
        message: Some(message.to_owned()),
    }
}

async fn timed_query<T>(
    future: impl Future<Output = Result<T, MysqlError>>,
) -> Result<T, AccountQueryFailure> {
    tokio::time::timeout(ACCOUNT_QUERY_TIMEOUT, future)
        .await
        .map_err(|_| AccountQueryFailure::Timeout)?
        .map_err(AccountQueryFailure::Mysql)
}

fn safe_query_message(error: &MysqlError) -> String {
    match error {
        MysqlError::Server(server) => server.message.clone(),
        _ => "The MySQL connection ended before the capability query completed".to_owned(),
    }
}

fn redact_password(message: &str, password: Option<&str>) -> String {
    let Some(password) = password.filter(|password| !password.is_empty()) else {
        return message.to_owned();
    };
    let literal = string_literal(password);
    message
        .replace(&literal, "'[REDACTED]'")
        .replace(password, "[REDACTED]")
}

fn account_query_unavailable(code: &'static str, message: &'static str) -> AppError {
    AppError::new(AppErrorKind::InvalidRequest, ApiError::new(code, message))
}

fn account_validation_error(code: &'static str, message: &'static str) -> AppError {
    AppError::invalid(code, message)
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use crate::{
        Application,
        native_administration_types::{
            AdministrationAction, AdministrationCommand, PrincipalRef, PrivilegeScope,
            PrivilegeTarget,
        },
    };

    use super::{
        AccountPreviewRegistry, account_outcome_unknown, authorize_account_preview,
        build_account_sql, execute_mysql_account as execute_mysql_account_impl, preview_account,
        preview_mysql_account as preview_mysql_account_impl, redact_password,
        sql_mode_with_no_backslash_escapes, string_literal,
    };

    #[test]
    fn every_administration_action_matches_mysql_sql() {
        assert_eq!(
            sql(AdministrationAction::CreatePrincipal),
            "CREATE USER 'reader'@'%' IDENTIFIED BY 'pa''ss\\word'"
        );
        assert_eq!(
            sql(AdministrationAction::AlterCredential),
            "ALTER USER 'reader'@'%' IDENTIFIED BY 'pa''ss\\word'"
        );
        assert_eq!(
            sql(AdministrationAction::LockPrincipal),
            "ALTER USER 'reader'@'%' ACCOUNT LOCK"
        );
        assert_eq!(
            sql(AdministrationAction::UnlockPrincipal),
            "ALTER USER 'reader'@'%' ACCOUNT UNLOCK"
        );
        assert_eq!(
            sql(AdministrationAction::DropPrincipal),
            "DROP USER 'reader'@'%'"
        );
        assert_eq!(
            sql(AdministrationAction::GrantPrivileges),
            "GRANT SELECT, SHOW VIEW, CREATE TEMPORARY TABLES ON `odd``db`.`order``item` TO 'reader'@'%' WITH GRANT OPTION"
        );
        assert_eq!(
            sql(AdministrationAction::RevokePrivileges),
            "REVOKE SELECT, SHOW VIEW, CREATE TEMPORARY TABLES ON `odd``db`.`order``item` FROM 'reader'@'%'"
        );
    }

    #[test]
    fn scopes_and_account_literals_use_stable_mysql_escaping() {
        let mut request = command(AdministrationAction::GrantPrivileges);
        request.principal.name = "o'brien\\ops".to_owned();
        request.principal.qualifier = Some("local'host".to_owned());
        request.target.as_mut().expect("target").scope = PrivilegeScope::Global;
        assert_eq!(
            build_account_sql(&request, false).expect("global grant"),
            "GRANT SELECT, SHOW VIEW, CREATE TEMPORARY TABLES ON *.* TO 'o''brien\\ops'@'local''host' WITH GRANT OPTION"
        );

        request.target.as_mut().expect("target").scope = PrivilegeScope::Database;
        assert_eq!(
            build_account_sql(&request, false).expect("database grant"),
            "GRANT SELECT, SHOW VIEW, CREATE TEMPORARY TABLES ON `odd``db`.* TO 'o''brien\\ops'@'local''host' WITH GRANT OPTION"
        );
    }

    #[test]
    fn account_literal_mode_is_stable_from_default_and_no_backslash_modes() {
        assert_eq!(
            string_literal("o'brien\\ops"),
            "'o''brien\\ops'",
            "backslashes must remain data while quotes use SQL-standard doubling"
        );
        assert_eq!(
            sql_mode_with_no_backslash_escapes("STRICT_TRANS_TABLES"),
            Some("STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES".to_owned())
        );
        assert_eq!(
            sql_mode_with_no_backslash_escapes(""),
            Some("NO_BACKSLASH_ESCAPES".to_owned())
        );
        assert_eq!(
            sql_mode_with_no_backslash_escapes("STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES"),
            None
        );
        assert_eq!(
            sql_mode_with_no_backslash_escapes("no_backslash_escapes"),
            None
        );
    }

    #[test]
    fn preview_masks_password_and_issues_an_opaque_token() {
        let registry = AccountPreviewRegistry::default();
        let request = command(AdministrationAction::CreatePrincipal);
        let preview = preview_account(&registry, &request).expect("valid account preview");

        assert_eq!(
            preview.sql,
            "CREATE USER 'reader'@'%' IDENTIFIED BY '******'"
        );
        assert_eq!(preview.preview_token.len(), 64);
        assert!(
            preview
                .preview_token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_ne!(
            preview.preview_token,
            preview_account(&registry, &request)
                .expect("repeated preview")
                .preview_token,
            "repeating identical input must not produce a caller-computable token"
        );
        assert_ne!(
            preview.preview_token,
            preview_account(&registry, &command_with_credential("different"))
                .expect("second preview")
                .preview_token
        );
    }

    #[test]
    fn preview_tokens_are_datasource_bound_exact_and_single_use() {
        let registry = AccountPreviewRegistry::default();
        let request = command(AdministrationAction::DropPrincipal);
        let sql = build_account_sql(&request, false).expect("account SQL");

        let wrong_datasource =
            preview_account(&registry, &request).expect("wrong-datasource preview");
        assert!(!registry.consume(&wrong_datasource.preview_token, "other-datasource", &sql));
        assert!(!registry.consume(
            &wrong_datasource.preview_token,
            &request.datasource_id,
            &sql
        ));

        let wrong_sql = preview_account(&registry, &request).expect("wrong-SQL preview");
        assert!(!registry.consume(
            &wrong_sql.preview_token,
            &request.datasource_id,
            &format!("{sql} ")
        ));
        assert!(!registry.consume(&wrong_sql.preview_token, &request.datasource_id, &sql));

        let valid = preview_account(&registry, &request).expect("valid preview");
        assert!(registry.consume(&valid.preview_token, &request.datasource_id, &sql));
        assert!(!registry.consume(&valid.preview_token, &request.datasource_id, &sql));
    }

    #[test]
    fn registry_preflight_does_not_consume_the_preview_token() {
        let application = Application::new();
        let mut request = command(AdministrationAction::DropPrincipal);
        request.preview_token = Some(
            preview_mysql_account_impl(&application, &request)
                .expect("account preview")
                .preview_token,
        );

        authorize_account_preview(&application.inner.account_previews, &request, false)
            .expect("non-consuming preflight");
        authorize_account_preview(&application.inner.account_previews, &request, true)
            .expect("single execution authorization");
        let error = authorize_account_preview(&application.inner.account_previews, &request, true)
            .expect_err("execution authorization must remain single-use");
        assert_eq!(error.api_error().code, "mysql.account.previewTokenMismatch");
    }

    #[test]
    fn duplicate_privileges_are_removed_in_first_seen_order() {
        let mut request = command(AdministrationAction::GrantPrivileges);
        request.privileges = vec![
            "select".to_owned(),
            " SELECT ".to_owned(),
            "update".to_owned(),
        ];
        assert_eq!(
            build_account_sql(&request, false).expect("deduplicated grant"),
            "GRANT SELECT, UPDATE ON `odd``db`.`order``item` TO 'reader'@'%' WITH GRANT OPTION"
        );
    }

    #[test]
    fn invalid_fields_return_mysql_error_codes() {
        let mut request = command(AdministrationAction::CreatePrincipal);
        request.principal.name.clear();
        assert_code(&request, "mysql.account.userRequired");

        request.principal.name = "reader\0hidden".to_owned();
        assert_code(&request, "mysql.account.invalidAccountName");

        request.principal.name = "reader".to_owned();
        request.credential = Some("   ".to_owned());
        assert_code(&request, "mysql.account.passwordRequired");

        request = command(AdministrationAction::GrantPrivileges);
        request.target = None;
        assert_code(&request, "mysql.account.scopeRequired");

        request.target = Some(PrivilegeTarget {
            scope: PrivilegeScope::Database,
            database_name: None,
            schema_name: None,
            object_name: None,
        });
        assert_code(&request, "mysql.account.databaseRequired");

        request.target = Some(PrivilegeTarget {
            scope: PrivilegeScope::Table,
            database_name: Some("inventory".to_owned()),
            schema_name: None,
            object_name: None,
        });
        assert_code(&request, "mysql.account.tableRequired");

        request.target.as_mut().expect("target").object_name = Some("orders".to_owned());
        request.privileges = vec!["ROLE_ADMIN".to_owned()];
        assert_code(&request, "mysql.account.privilegeUnsupported");

        request.privileges = vec!["SELECT".to_owned()];
        request.target.as_mut().expect("target").scope = PrivilegeScope::Schema;
        assert_code(&request, "mysql.account.scopeUnsupported");
    }

    #[tokio::test]
    async fn token_mismatch_is_rejected_before_storage_or_mysql_access() {
        let mut request = command(AdministrationAction::DropPrincipal);
        request.preview_token = Some("not-the-preview-token".to_owned());

        let application = Application::new();
        let direct_error = execute_mysql_account_impl(&application, &request)
            .await
            .expect_err("direct token mismatch must fail before datasource resolution");
        assert_eq!(
            direct_error.api_error().code,
            "mysql.account.previewTokenMismatch"
        );
    }

    #[test]
    fn interrupted_account_writes_are_explicitly_non_retryable_unknown_outcomes() {
        let result = account_outcome_unknown(
            AdministrationAction::AlterCredential,
            "ALTER USER 'reader'@'%' IDENTIFIED BY '******'".to_owned(),
            "The outcome is unknown and must not be retried blindly".to_owned(),
        );

        assert!(!result.success);
        assert_eq!(
            result.failure_code.as_deref(),
            Some("mysql.account.outcomeUnknown")
        );
        assert!(
            result
                .message
                .as_deref()
                .is_some_and(|message| message.contains("must not be retried blindly"))
        );
    }

    #[test]
    fn server_messages_cannot_echo_the_password() {
        let password = "pa'ss\\word";
        let message = format!(
            "syntax near {} containing {password}",
            string_literal(password)
        );
        let redacted = redact_password(&message, Some(password));
        assert!(!redacted.contains(password));
        assert!(!redacted.contains(&string_literal(password)));
        assert!(redacted.contains("[REDACTED]"));
    }

    fn sql(action: AdministrationAction) -> String {
        build_account_sql(&command(action), false).expect("account SQL")
    }

    fn command(action: AdministrationAction) -> AdministrationCommand {
        AdministrationCommand {
            datasource_id: "42".to_owned(),
            principal: PrincipalRef {
                name: "reader".to_owned(),
                qualifier: Some("%".to_owned()),
            },
            action,
            target: Some(PrivilegeTarget {
                scope: PrivilegeScope::Table,
                database_name: Some("odd`db".to_owned()),
                schema_name: None,
                object_name: Some("order`item".to_owned()),
            }),
            privileges: vec![
                "SELECT".to_owned(),
                "SHOW_VIEW".to_owned(),
                "CREATE_TEMPORARY_TABLES".to_owned(),
            ],
            grant_option: true,
            credential: Some("pa'ss\\word".to_owned()),
            preview_token: None,
        }
    }

    fn command_with_credential(credential: &str) -> AdministrationCommand {
        let mut request = command(AdministrationAction::CreatePrincipal);
        request.credential = Some(credential.to_owned());
        request
    }

    fn assert_code(request: &AdministrationCommand, expected: &str) {
        let error = build_account_sql(request, false).expect_err("request must be invalid");
        assert_eq!(error.api_error().code, expected);
    }
}
