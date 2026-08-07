use std::{
    collections::BTreeMap,
    fmt::Write as _,
    future::Future,
    mem::size_of,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chat2db_contract::{
    ApiError, ColumnNullability, DatasourceConnection, JdbcValue, JdbcValueType, QueryLimits,
    ResultColumn, ResultMetadata, ResultRow, StartQueryRequest,
};
use chat2db_engine_protocol::wire;
use chat2db_storage::Storage;
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use oracle_rs::{
    Config, Connection, Error as OracleError, LobData, LobValue, OracleType,
    QueryResult as OracleQueryResult, Row as OracleRow, Value as OracleValue,
    config::ServiceMethod,
    statement::ColumnInfo,
    types::{OracleDate, OracleNumber, OracleTimestamp},
};
use prost::Message;
use sqlparser::{ast::Statement, dialect::OracleDialect, parser::Parser};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    AppError, AppErrorKind, Application,
    datasource_session::{ResolvedDatasourceConnection, resolve_datasource_connection},
    native_driver::{
        NativeConnectionDriver, NativeDialectDriver, NativeDriver, NativeMetadataDriver,
        NativeQueryDriver, NativeTableDriver,
    },
    native_driver_types::{
        BuiltSql, ColumnList, ColumnMetadata, CreateSchemaSqlRequest, DatabaseDefinition,
        DatabaseList, DatabaseMetadata, DmlAssignment, DmlColumn, DmlRow, DmlSqlRequest,
        DmlStatement, DmlTarget, DmlTemporalKind, DmlValue, EntityRelationColumn,
        EntityRelationForeignKey, EntityRelationTable, ForeignKeyList, ForeignKeyMetadata,
        FunctionList, FunctionMetadata, FunctionParameterList, FunctionParameterMetadata,
        IndexColumnMetadata, IndexList, IndexMetadata, ListColumnsRequest, ListDatabasesRequest,
        ListIndexesRequest, ListRoutinesRequest, ListSchemasRequest, ListTableKeysRequest,
        ListTablesRequest, ListTriggersRequest, ListViewsRequest, MetadataObjectRef, MetadataScope,
        NamespaceSqlOperation, NamespaceSqlRequest, NativeDriverDescriptor, PrimaryKeyList,
        PrimaryKeyMetadata, ProcedureList, ProcedureMetadata, ProcedureParameterList,
        ProcedureParameterMetadata, SchemaList, SchemaMetadata, TableList, TableMetadata,
        TablePreviewAccepted, TablePreviewRequest, TableRef, TriggerList, TriggerMetadata,
        ViewList,
    },
    operation::CancellationRequest,
    query::{
        DatabaseValue, DatabaseWriteError, NativeConsoleRequest, NativeConsoleResult,
        PreparedQuery, QueryExecutionOptions, QueryParameter, QueryTaskError, RetainedWriter,
    },
    ssh::{SshTunnel, SshTunnelIdentity},
};

const ORACLE_DATABASE_TYPE: &str = "ORACLE";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PORT: u16 = 1_521;
const DEFAULT_BATCH_ROWS: u32 = 256;
const DEFAULT_BATCH_BYTES: u32 = 256 * 1024;
const DEFAULT_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::DefaultResultBytes as u64;
const MAX_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::MaxResultBytes as u64;
const MAX_BATCH_ROWS: u32 = wire::JdbcProtocolLimit::MaxBatchRows as u32;
const MAX_BATCH_BYTES: u32 = wire::JdbcProtocolLimit::MaxBatchBytes as u32;
const MAX_COLUMNS: usize = wire::JdbcProtocolLimit::MaxColumns as usize;
const MAX_PARAMETERS: usize = wire::JdbcProtocolLimit::MaxParameters as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_METADATA_ROWS: usize = 100_000;
const MAX_METADATA_RESULT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CONSOLE_STATEMENTS: usize = 1_000;
const MAX_CONSOLE_PAGE_SIZE: u32 = 10_000;
const MAX_CONSOLE_RESULT_BYTES: u64 = DEFAULT_RESULT_BYTES;
const MAX_CONSOLE_ROWS: u64 = 1_000_000;
const MAX_TABLE_PREVIEW_ROWS: u32 = 1_000;
const FETCH_ROWS: u32 = 1;

const ORACLE_SYSTEM_SCHEMAS: &[&str] = &[
    "ANONYMOUS",
    "AUDSYS",
    "CTXSYS",
    "DBSNMP",
    "DIP",
    "DVF",
    "DVSYS",
    "GGSYS",
    "GSMADMIN_INTERNAL",
    "LBACSYS",
    "MDSYS",
    "OJVMSYS",
    "OLAPSYS",
    "ORDDATA",
    "ORDSYS",
    "OUTLN",
    "SYS",
    "SYSBACKUP",
    "SYSDG",
    "SYSKM",
    "SYSTEM",
    "WMSYS",
    "XDB",
];

pub(crate) struct OracleNativeDriver;

pub(crate) const ORACLE_DRIVER_DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
    id: "oracle",
    implementation: "oracle-rs",
    database_types: &["ORACLE"],
    compatibility_aliases: &[
        "oracle",
        "oracle-rs",
        "oracle.jdbc.OracleDriver",
        "oracle.jdbc.driver.OracleDriver",
    ],
};

impl NativeDriver for OracleNativeDriver {
    fn descriptor(&self) -> &'static NativeDriverDescriptor {
        &ORACLE_DRIVER_DESCRIPTOR
    }

    fn connection(&self) -> Option<&dyn NativeConnectionDriver> {
        Some(self)
    }

    fn query(&self) -> Option<&dyn NativeQueryDriver> {
        Some(self)
    }

    fn metadata(&self) -> Option<&dyn NativeMetadataDriver> {
        Some(self)
    }

    fn tables(&self) -> Option<&dyn NativeTableDriver> {
        Some(self)
    }

    fn dialect(&self) -> Option<&dyn NativeDialectDriver> {
        Some(self)
    }
}

impl NativeDialectDriver for OracleNativeDriver {
    fn build_create_schema(&self, request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
        build_oracle_create_schema(request)
    }

    fn build_namespace_sql(&self, request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
        build_oracle_namespace_sql(request)
    }

    fn build_dml(&self, request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
        build_oracle_dml(request)
    }
}

#[async_trait]
impl NativeConnectionDriver for OracleNativeDriver {
    async fn test_connection(&self, connection: &DatasourceConnection) -> Result<(), AppError> {
        test_connection(connection).await.map(|_| ())
    }

    async fn test_connection_with_local_port(
        &self,
        connection: &DatasourceConnection,
    ) -> Result<Option<u16>, AppError> {
        test_connection(connection).await
    }
}

#[async_trait]
impl NativeQueryDriver for OracleNativeDriver {
    fn is_read_candidate(&self, sql: &str) -> Result<bool, AppError> {
        is_read_candidate(sql)
    }

    fn validate_query(&self, query: &PreparedQuery) -> Result<(), AppError> {
        validate_query(query)
    }

    async fn execute_query_task(
        &self,
        application: &Application,
        operation_id: &str,
        cancellation: watch::Receiver<CancellationRequest>,
        query: PreparedQuery,
        storage: Storage,
        resolved: ResolvedDatasourceConnection,
    ) -> Result<ResultMetadata, QueryTaskError> {
        execute_query_task(
            application,
            operation_id,
            cancellation,
            query,
            storage,
            resolved,
        )
        .await
    }

    async fn execute_update(
        &self,
        resolved: ResolvedDatasourceConnection,
        sql: String,
        cancellation: CancellationToken,
    ) -> Result<u64, DatabaseWriteError> {
        execute_update(resolved, sql, cancellation).await
    }

    async fn execute_console(
        &self,
        application: &Application,
        request: NativeConsoleRequest,
        cancellation: watch::Receiver<CancellationRequest>,
        force_read_only: bool,
    ) -> Result<Vec<NativeConsoleResult>, AppError> {
        execute_console(application, request, cancellation, force_read_only).await
    }
}

#[async_trait]
impl NativeMetadataDriver for OracleNativeDriver {
    async fn list_schemas(
        &self,
        application: &Application,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError> {
        list_schemas(application, request).await
    }

    async fn list_databases(
        &self,
        application: &Application,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError> {
        list_databases(application, request).await
    }

    async fn list_tables(
        &self,
        application: &Application,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError> {
        list_tables(application, request).await
    }

    async fn list_columns(
        &self,
        application: &Application,
        request: ListColumnsRequest,
    ) -> Result<ColumnList, AppError> {
        list_columns(application, request).await
    }

    async fn list_indexes(
        &self,
        application: &Application,
        request: ListIndexesRequest,
    ) -> Result<IndexList, AppError> {
        list_indexes(application, request).await
    }

    async fn list_views(
        &self,
        application: &Application,
        request: ListViewsRequest,
    ) -> Result<ViewList, AppError> {
        list_views(application, request).await
    }

    async fn get_view(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TableMetadata, AppError> {
        get_view(application, request).await
    }

    async fn list_imported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        list_imported_keys(application, request).await
    }

    async fn list_exported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        list_exported_keys(application, request).await
    }

    async fn list_primary_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<PrimaryKeyList, AppError> {
        list_primary_keys(application, request).await
    }

    async fn list_functions(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<FunctionList, AppError> {
        list_functions(application, request).await
    }

    async fn get_function(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionMetadata, AppError> {
        get_function(application, request).await
    }

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionParameterList, AppError> {
        list_function_parameters(application, request).await
    }

    async fn list_procedures(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError> {
        list_procedures(application, request).await
    }

    async fn get_procedure(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureMetadata, AppError> {
        get_procedure(application, request).await
    }

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureParameterList, AppError> {
        list_procedure_parameters(application, request).await
    }

    async fn list_triggers(
        &self,
        application: &Application,
        request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError> {
        list_triggers(application, request).await
    }

    async fn get_trigger(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TriggerMetadata, AppError> {
        get_trigger(application, request).await
    }
}

#[async_trait]
impl NativeTableDriver for OracleNativeDriver {
    async fn load_er_tables(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Vec<EntityRelationTable>, AppError> {
        load_er_tables(application, datasource_id, database_name, schema_name).await
    }

    async fn validate_column_reorder(
        &self,
        _application: &Application,
        _datasource_id: &str,
        _database_name: &str,
        _table_name: &str,
        _column_names: &[String],
    ) -> Result<(), AppError> {
        Err(capability_not_supported("physical column reordering"))
    }

    async fn table_ddl(
        &self,
        application: &Application,
        datasource_id: &str,
        _database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        object_ddl(application, datasource_id, schema_name, table_name, "TABLE").await
    }

    async fn start_table_preview(
        &self,
        application: &Application,
        request: TablePreviewRequest,
        row_limit: u32,
    ) -> Result<TablePreviewAccepted, AppError> {
        start_table_preview(application, request, row_limit).await
    }
}

struct PreparedOracleConnection {
    config: Config,
    tunnel: Option<SshTunnel>,
}

struct ManagedOracleConnection {
    connection: Connection,
    tunnel: Option<SshTunnel>,
    local_port: Option<u16>,
}

impl ManagedOracleConnection {
    async fn close(self) -> Result<(), AppError> {
        let close_result = tokio::time::timeout(CLOSE_TIMEOUT, self.connection.close()).await;
        let connection_result = match close_result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(oracle_connection_error(&error)),
            Err(_) => Err(AppError::unavailable(
                "oracle_connection_close_timeout",
                "The Oracle connection could not be closed in time",
            )),
        };
        let tunnel_result = match self.tunnel {
            Some(tunnel) => match tokio::time::timeout(CLOSE_TIMEOUT, tunnel.close()).await {
                Ok(result) => result,
                Err(_) => Err(AppError::unavailable(
                    "oracle_ssh_tunnel_close_timeout",
                    "The Oracle SSH tunnel could not be closed in time",
                )),
            },
            None => Ok(()),
        };
        connection_result.and(tunnel_result)
    }

    async fn abandon(self) {
        self.connection.mark_closed();
        drop(self.connection);
        if let Some(tunnel) = self.tunnel
            && let Err(error) = tunnel.close().await
        {
            tracing::warn!(error = %error, "Oracle SSH tunnel cleanup failed after connection abandonment");
        }
    }
}

async fn test_connection(connection: &DatasourceConnection) -> Result<Option<u16>, AppError> {
    let managed = open_connection(connection, SshTunnelIdentity::Ephemeral).await?;
    let local_port = managed.local_port;
    let ping = tokio::time::timeout(OPERATION_TIMEOUT, managed.connection.ping()).await;
    match ping {
        Ok(Ok(())) => {
            managed.close().await?;
            Ok(local_port)
        }
        Ok(Err(error)) => {
            managed.abandon().await;
            Err(oracle_connection_error(&error))
        }
        Err(_) => {
            managed.abandon().await;
            Err(oracle_operation_timeout("connection test"))
        }
    }
}

async fn open_resolved_connection(
    resolved: &ResolvedDatasourceConnection,
) -> Result<ManagedOracleConnection, AppError> {
    open_connection(
        &resolved.connection,
        SshTunnelIdentity::Datasource {
            datasource_id: &resolved.datasource_id,
            revision: resolved.datasource_revision,
        },
    )
    .await
}

async fn open_connection(
    connection: &DatasourceConnection,
    identity: SshTunnelIdentity<'_>,
) -> Result<ManagedOracleConnection, AppError> {
    let prepared = prepare_connection(connection, identity).await?;
    let local_port = prepared.tunnel.as_ref().map(SshTunnel::local_port);
    let connect = Connection::connect_with_config(prepared.config);
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(Ok(connection)) => Ok(ManagedOracleConnection {
            connection,
            tunnel: prepared.tunnel,
            local_port,
        }),
        Ok(Err(error)) => {
            if let Some(tunnel) = prepared.tunnel
                && let Err(close_error) = tunnel.close().await
            {
                tracing::warn!(error = %close_error, "Oracle SSH tunnel cleanup failed after connection rejection");
            }
            Err(oracle_connection_error(&error))
        }
        Err(_) => {
            if let Some(tunnel) = prepared.tunnel
                && let Err(error) = tunnel.close().await
            {
                tracing::warn!(error = %error, "Oracle SSH tunnel cleanup failed after connection timeout");
            }
            Err(AppError::unavailable(
                "oracle_connection_timeout",
                "The Oracle server did not accept the connection in time",
            ))
        }
    }
}

async fn prepare_connection(
    connection: &DatasourceConnection,
    identity: SshTunnelIdentity<'_>,
) -> Result<PreparedOracleConnection, AppError> {
    let mut config = connection_config(connection)?;
    let Some(ssh) = connection.ssh.as_ref() else {
        return Ok(PreparedOracleConnection {
            config,
            tunnel: None,
        });
    };
    let tunnel = SshTunnel::open(identity, ssh, config.host.clone(), config.port).await?;
    apply_ssh_forward(&mut config, tunnel.local_port());
    Ok(PreparedOracleConnection {
        config,
        tunnel: Some(tunnel),
    })
}

fn connection_config(connection: &DatasourceConnection) -> Result<Config, AppError> {
    let (mut config, url_username, url_password, tls) = parse_oracle_url(&connection.jdbc_url)?;
    let username = connection_property(connection, &["user", "username"])?
        .or(url_username)
        .ok_or_else(|| invalid_connection_property("username"))?;
    let password = connection_property(connection, &["password"])?
        .or(url_password)
        .ok_or_else(|| invalid_connection_property("password"))?;
    validate_credential(&username, "username")?;
    validate_credential(&password, "password")?;
    config.set_username(username);
    config.set_password(password);
    if tls {
        ensure_rustls_crypto_provider()?;
        config = config
            .with_tls()
            .map_err(|error| oracle_connection_error(&error))?;
    }
    Ok(config)
}

fn ensure_rustls_crypto_provider() -> Result<(), AppError> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        Ok(())
    } else {
        Err(AppError::internal())
    }
}

fn apply_ssh_forward(config: &mut Config, local_port: u16) {
    let original_host = config.host.clone();
    if let Some(tls) = config.tls_config.as_mut() {
        tls.server_name.get_or_insert(original_host);
    }
    "127.0.0.1".clone_into(&mut config.host);
    config.port = local_port;
}

fn parse_oracle_url(
    jdbc_url: &str,
) -> Result<(Config, Option<String>, Option<String>, bool), AppError> {
    let value = jdbc_url.trim();
    if let Some(rest) = value.strip_prefix("jdbc:oracle:thin:@") {
        return parse_jdbc_oracle_target(rest);
    }
    if let Some(rest) = value.strip_prefix("jdbc:oracle:@") {
        return parse_jdbc_oracle_target(rest);
    }
    if value
        .get(..9)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("oracle://"))
    {
        return parse_oracle_url_target(value);
    }
    Err(invalid_connection_url())
}

fn parse_jdbc_oracle_target(
    target: &str,
) -> Result<(Config, Option<String>, Option<String>, bool), AppError> {
    let target = target.trim();
    if target.is_empty() || target.starts_with('(') {
        return Err(invalid_connection_url());
    }
    let (target, query) = target.split_once('?').map_or((target, ""), |parts| parts);
    let target = target.trim_start_matches('/');
    if target.is_empty() || (!target.contains('/') && target.matches(':').count() != 2) {
        return Err(invalid_connection_url());
    }
    let config = target
        .parse::<Config>()
        .map_err(|_| invalid_connection_url())?;
    validate_config_target(&config)?;
    Ok((config, None, None, tls_requested(query)?))
}

fn parse_oracle_url_target(
    value: &str,
) -> Result<(Config, Option<String>, Option<String>, bool), AppError> {
    let url = Url::parse(value).map_err(|_| invalid_connection_url())?;
    if url.fragment().is_some() {
        return Err(invalid_connection_url());
    }
    let host = url.host_str().ok_or_else(invalid_connection_url)?;
    if host.contains(':') {
        return Err(AppError::invalid(
            "invalid_oracle_connection",
            "IPv6 Oracle connection targets are not supported by the selected protocol driver",
        ));
    }
    let service = url.path().trim_matches('/');
    if service.is_empty() || service.contains('/') {
        return Err(invalid_connection_url());
    }
    validate_connection_component(host, "host", 255)?;
    validate_connection_component(service, "serviceName", MAX_IDENTIFIER_BYTES)?;
    let port = url.port().unwrap_or(DEFAULT_PORT);
    let mut sid = None;
    let mut tls = false;
    for (key, value) in url.query_pairs() {
        if key.eq_ignore_ascii_case("sid") {
            if sid.replace(value.into_owned()).is_some() {
                return Err(invalid_connection_property("sid"));
            }
        } else if key.eq_ignore_ascii_case("ssl") || key.eq_ignore_ascii_case("tcps") {
            tls = parse_bool(&value).ok_or_else(|| invalid_connection_property(&key))?;
        } else {
            return Err(invalid_connection_property(&key));
        }
    }
    let username = (!url.username().is_empty()).then(|| url.username().to_owned());
    let password = url.password().map(ToOwned::to_owned);
    let config = if let Some(sid) = sid {
        validate_connection_component(&sid, "sid", MAX_IDENTIFIER_BYTES)?;
        Config::with_sid(host, port, sid, "", "")
    } else {
        Config::new(host, port, service, "", "")
    };
    Ok((config, username, password, tls))
}

fn tls_requested(query: &str) -> Result<bool, AppError> {
    let mut tls = false;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key.eq_ignore_ascii_case("ssl") || key.eq_ignore_ascii_case("tcps") {
            tls = parse_bool(&value).ok_or_else(|| invalid_connection_property(&key))?;
        } else if !key.is_empty() {
            return Err(invalid_connection_property(&key));
        }
    }
    Ok(tls)
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.eq_ignore_ascii_case("true") || value == "1" {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") || value == "0" {
        Some(false)
    } else {
        None
    }
}

fn validate_config_target(config: &Config) -> Result<(), AppError> {
    validate_connection_component(&config.host, "host", 255)?;
    match &config.service {
        ServiceMethod::ServiceName(service) => {
            validate_connection_component(service, "serviceName", MAX_IDENTIFIER_BYTES)
        }
        ServiceMethod::Sid(sid) => validate_connection_component(sid, "sid", MAX_IDENTIFIER_BYTES),
    }
}

fn validate_connection_component(
    value: &str,
    field: &str,
    max_bytes: usize,
) -> Result<(), AppError> {
    if value.trim().is_empty()
        || value.len() > max_bytes
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(invalid_connection_property(field));
    }
    Ok(())
}

fn validate_credential(value: &str, field: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > 64 * 1024 || value.contains('\0') {
        return Err(invalid_connection_property(field));
    }
    Ok(())
}

fn connection_property(
    connection: &DatasourceConnection,
    keys: &[&str],
) -> Result<Option<String>, AppError> {
    let mut result = None;
    for property in &connection.properties {
        if keys
            .iter()
            .any(|key| property.key.eq_ignore_ascii_case(key))
            && result.replace(property.value.clone()).is_some()
        {
            return Err(invalid_connection_property(keys[0]));
        }
    }
    Ok(result)
}

fn invalid_connection_url() -> AppError {
    AppError::invalid(
        "invalid_oracle_connection",
        "A valid jdbc:oracle:thin:@host:port/service or oracle://host:port/service URL is required",
    )
}

fn invalid_connection_property(property: &str) -> AppError {
    AppError::invalid(
        "invalid_oracle_connection",
        format!("The Oracle connection property {property} is invalid"),
    )
}

fn oracle_connection_error(error: &OracleError) -> AppError {
    match error {
        OracleError::InvalidConnectionString(_) => invalid_connection_url(),
        OracleError::InvalidCredentials
        | OracleError::AuthenticationFailed(_)
        | OracleError::InvalidServiceName { .. }
        | OracleError::InvalidSid { .. }
        | OracleError::ConnectionRefused { .. }
        | OracleError::OracleError { .. }
        | OracleError::ServerError { .. } => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("oracle_connection_rejected", error.to_string()),
        ),
        OracleError::ProtocolVersionNotSupported(_, _)
        | OracleError::FeatureNotSupported(_)
        | OracleError::NativeNetworkEncryptionRequired => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new(
                "oracle_protocol_not_supported",
                "The native Oracle driver supports Oracle Database 12.1 or newer and does not support this server configuration",
            ),
        ),
        _ => AppError::unavailable(
            "oracle_connection_failed",
            "The Oracle server could not be reached or the protocol session ended unexpectedly",
        ),
    }
}

fn oracle_query_error(error: &OracleError) -> AppError {
    match error {
        OracleError::OracleError { .. }
        | OracleError::ServerError { .. }
        | OracleError::SqlError(_)
        | OracleError::NoDataFound => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("oracle_query_failed", error.to_string()),
        ),
        OracleError::FeatureNotSupported(_) => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("oracle_query_not_supported", error.to_string()),
        ),
        _ => oracle_connection_error(error),
    }
}

fn oracle_operation_timeout(operation: &str) -> AppError {
    AppError::unavailable(
        "oracle_operation_timeout",
        format!(
            "The Oracle {operation} did not complete in time; its dedicated connection was discarded"
        ),
    )
}

fn capability_not_supported(capability: &str) -> AppError {
    AppError::invalid(
        "native_driver_capability_not_supported",
        format!("The Oracle driver does not implement {capability}"),
    )
}

async fn resolve_native_connection(
    application: &Application,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    if application
        .native_driver_for_datasource_driver_id(&resolved.driver_id)
        .is_none_or(|driver| driver.descriptor().id != ORACLE_DRIVER_DESCRIPTOR.id)
    {
        return Err(AppError::invalid(
            "oracle_driver_mismatch",
            "The datasource is not configured with the native Oracle driver",
        ));
    }
    Ok(resolved)
}

pub(crate) fn is_read_candidate(sql: &str) -> Result<bool, AppError> {
    Ok(matches!(
        first_sql_keyword(sql)?.as_deref(),
        Some("SELECT" | "WITH")
    ))
}

pub(crate) fn validate_query(query: &PreparedQuery) -> Result<(), AppError> {
    validate_read_sql(&query.sql)?;
    let _ = oracle_query_parameters(&query.parameters)?;
    validate_query_options(query.options)
}

fn validate_read_sql(sql: &str) -> Result<(), AppError> {
    validate_sql_text(sql)?;
    if split_oracle_script(sql)?.len() != 1 {
        return Err(AppError::invalid(
            "oracle_native_query_unsupported",
            "Native Oracle read execution accepts exactly one statement",
        ));
    }
    if !is_read_candidate(sql)? {
        return Err(AppError::invalid(
            "oracle_native_query_unsupported",
            "Native Oracle read execution accepts SELECT or WITH queries",
        ));
    }
    let words = oracle_sql_words(sql)?;
    if words
        .windows(2)
        .any(|window| matches!(window, [first, second] if first == "FOR" && second == "UPDATE"))
    {
        return Err(AppError::invalid(
            "oracle_native_query_unsupported",
            "Native Oracle read execution does not accept SELECT FOR UPDATE",
        ));
    }
    if let Ok(statements) = Parser::parse_sql(&OracleDialect {}, sql)
        && !matches!(statements.as_slice(), [Statement::Query(_)])
    {
        return Err(AppError::invalid(
            "oracle_native_query_unsupported",
            "Native Oracle read execution accepts one query statement",
        ));
    }
    Ok(())
}

fn validate_sql_text(sql: &str) -> Result<(), AppError> {
    if sql.trim().is_empty() || sql.len() > MAX_SQL_BYTES || sql.contains('\0') {
        return Err(AppError::invalid(
            "invalid_query_request",
            format!("Oracle SQL must be non-empty and at most {MAX_SQL_BYTES} UTF-8 bytes"),
        ));
    }
    Ok(())
}

fn validate_query_options(options: QueryExecutionOptions) -> Result<(), AppError> {
    if options.target_batch_rows > MAX_BATCH_ROWS {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("batchRows must be at most {MAX_BATCH_ROWS}"),
        ));
    }
    if options.target_batch_bytes != 0
        && !(1_024..=MAX_BATCH_BYTES).contains(&options.target_batch_bytes)
    {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("batchBytes must be zero or between 1024 and {MAX_BATCH_BYTES}"),
        ));
    }
    if options.max_result_bytes > MAX_RESULT_BYTES {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("maxResultBytes must be at most {MAX_RESULT_BYTES}"),
        ));
    }
    Ok(())
}

fn oracle_query_parameters(parameters: &[QueryParameter]) -> Result<Vec<OracleValue>, AppError> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(AppError::invalid(
            "invalid_query_parameter_count",
            format!("Oracle queries accept at most {MAX_PARAMETERS} parameters"),
        ));
    }
    let mut ordered = parameters.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|parameter| parameter.position);
    ordered
        .into_iter()
        .enumerate()
        .map(|(index, parameter)| {
            let expected = u32::try_from(index + 1).map_err(|_| AppError::internal())?;
            if parameter.position != expected {
                return Err(AppError::invalid(
                    "invalid_query_parameter",
                    "Oracle parameter positions must be unique and contiguous from 1",
                ));
            }
            oracle_query_value(&parameter.value)
        })
        .collect()
}

fn oracle_query_value(value: &DatabaseValue) -> Result<OracleValue, AppError> {
    match value {
        DatabaseValue::Null => Ok(OracleValue::Null),
        DatabaseValue::Boolean(value) => Ok(OracleValue::Boolean(*value)),
        DatabaseValue::SignedInteger(value) => Ok(OracleValue::Integer(*value)),
        DatabaseValue::UnsignedInteger(value) => i64::try_from(*value)
            .map(OracleValue::Integer)
            .or_else(|_| oracle_decimal_value(&value.to_string())),
        DatabaseValue::Float32(value) => Ok(OracleValue::Float(f64::from(*value))),
        DatabaseValue::Float64(value) => Ok(OracleValue::Float(*value)),
        DatabaseValue::Decimal(value) => oracle_decimal_value(value),
        DatabaseValue::Text(value) => oracle_string_value(value, "text"),
        DatabaseValue::Binary(value) => {
            validate_scalar_bytes(value.len(), "binary")?;
            Ok(OracleValue::Bytes(value.clone()))
        }
        DatabaseValue::Date(value) => oracle_date_value(value),
        DatabaseValue::Time(value) => oracle_string_value(value, "time"),
        DatabaseValue::Timestamp(value) => oracle_timestamp_value(value),
        DatabaseValue::TimestampWithTimeZone(value) => oracle_timestamp_tz_value(value),
        DatabaseValue::Json(value) => {
            validate_scalar_bytes(value.len(), "JSON")?;
            serde_json::from_str(value)
                .map(OracleValue::Json)
                .map_err(|_| invalid_query_parameter("JSON"))
        }
        DatabaseValue::Uuid(value) => {
            validate_scalar_bytes(value.len(), "UUID")?;
            uuid::Uuid::parse_str(value).map_err(|_| invalid_query_parameter("UUID"))?;
            Ok(OracleValue::String(value.clone()))
        }
    }
}

fn oracle_decimal_value(value: &str) -> Result<OracleValue, AppError> {
    validate_scalar_bytes(value.len(), "decimal")?;
    oracle_rs::types::encode_oracle_number(value)
        .map_err(|_| invalid_query_parameter("decimal"))?;
    Ok(OracleValue::Number(OracleNumber::new(value)))
}

fn oracle_string_value(value: &str, label: &str) -> Result<OracleValue, AppError> {
    validate_scalar_bytes(value.len(), label)?;
    Ok(OracleValue::String(value.to_owned()))
}

fn oracle_date_value(value: &str) -> Result<OracleValue, AppError> {
    validate_scalar_bytes(value.len(), "date")?;
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| invalid_query_parameter("date"))?;
    Ok(OracleValue::Date(OracleDate::date(
        date.year(),
        u8::try_from(date.month()).map_err(|_| AppError::internal())?,
        u8::try_from(date.day()).map_err(|_| AppError::internal())?,
    )))
}

fn oracle_timestamp_value(value: &str) -> Result<OracleValue, AppError> {
    validate_scalar_bytes(value.len(), "timestamp")?;
    let value = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| invalid_query_parameter("timestamp"))?;
    Ok(OracleValue::Timestamp(OracleTimestamp::new(
        value.year(),
        u8::try_from(value.month()).map_err(|_| AppError::internal())?,
        u8::try_from(value.day()).map_err(|_| AppError::internal())?,
        u8::try_from(value.hour()).map_err(|_| AppError::internal())?,
        u8::try_from(value.minute()).map_err(|_| AppError::internal())?,
        u8::try_from(value.second()).map_err(|_| AppError::internal())?,
        value.nanosecond() / 1_000,
    )))
}

fn oracle_timestamp_tz_value(value: &str) -> Result<OracleValue, AppError> {
    validate_scalar_bytes(value.len(), "timestamp with time zone")?;
    let value = DateTime::parse_from_rfc3339(value)
        .map_err(|_| invalid_query_parameter("timestamp with time zone"))?;
    let offset_seconds = value.offset().local_minus_utc();
    let offset_hours = offset_seconds / 3_600;
    let offset_minutes = (offset_seconds % 3_600) / 60;
    Ok(OracleValue::Timestamp(OracleTimestamp::with_timezone(
        value.year(),
        u8::try_from(value.month()).map_err(|_| AppError::internal())?,
        u8::try_from(value.day()).map_err(|_| AppError::internal())?,
        u8::try_from(value.hour()).map_err(|_| AppError::internal())?,
        u8::try_from(value.minute()).map_err(|_| AppError::internal())?,
        u8::try_from(value.second()).map_err(|_| AppError::internal())?,
        value.nanosecond() / 1_000,
        i8::try_from(offset_hours)
            .map_err(|_| invalid_query_parameter("timestamp with time zone"))?,
        i8::try_from(offset_minutes)
            .map_err(|_| invalid_query_parameter("timestamp with time zone"))?,
    )))
}

fn validate_scalar_bytes(size: usize, label: &str) -> Result<(), AppError> {
    if size > MAX_SCALAR_BYTES {
        return Err(AppError::invalid(
            "invalid_query_parameter",
            format!("The Oracle {label} parameter exceeds {MAX_SCALAR_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn invalid_query_parameter(label: &str) -> AppError {
    AppError::invalid(
        "invalid_query_parameter",
        format!("The Oracle {label} parameter is invalid"),
    )
}

enum AwaitOutcome<T> {
    Completed(T),
    Cancelled(Option<String>),
}

async fn await_with_cancellation<F, T>(
    cancellation: &mut watch::Receiver<CancellationRequest>,
    future: F,
) -> AwaitOutcome<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(future);
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if changed.is_err() {
                    cancellation_open = false;
                    continue;
                }
                let request = { cancellation.borrow().clone() };
                if let CancellationRequest::Requested { reason } = request {
                    return AwaitOutcome::Cancelled(reason);
                }
            }
            output = &mut future => return AwaitOutcome::Completed(output),
        }
    }
}

async fn start_read_only_transaction(
    managed: &ManagedOracleConnection,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> AwaitOutcome<Result<(), AppError>> {
    // Autonomous transactions are independent of this transaction. Preventing side effects from
    // autonomous routines requires a database account whose grants do not permit those writes.
    match await_with_cancellation(
        cancellation,
        tokio::time::timeout(
            OPERATION_TIMEOUT,
            managed.connection.execute("SET TRANSACTION READ ONLY", &[]),
        ),
    )
    .await
    {
        AwaitOutcome::Completed(Ok(Ok(_))) => AwaitOutcome::Completed(Ok(())),
        AwaitOutcome::Completed(Ok(Err(error))) => {
            AwaitOutcome::Completed(Err(oracle_query_error(&error)))
        }
        AwaitOutcome::Completed(Err(_)) => {
            AwaitOutcome::Completed(Err(oracle_operation_timeout("read-only transaction setup")))
        }
        AwaitOutcome::Cancelled(reason) => AwaitOutcome::Cancelled(reason),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn execute_query_task(
    application: &Application,
    operation_id: &str,
    mut cancellation: watch::Receiver<CancellationRequest>,
    query: PreparedQuery,
    storage: Storage,
    resolved: ResolvedDatasourceConnection,
) -> Result<ResultMetadata, QueryTaskError> {
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(QueryTaskError::Cancelled(reason));
    }
    validate_query(&query)?;
    let parameters = oracle_query_parameters(&query.parameters)?;
    let open = open_resolved_connection(&resolved);
    let managed = match await_with_cancellation(&mut cancellation, open).await {
        AwaitOutcome::Completed(result) => result?,
        AwaitOutcome::Cancelled(reason) => return Err(QueryTaskError::Cancelled(reason)),
    };
    match start_read_only_transaction(&managed, &mut cancellation).await {
        AwaitOutcome::Completed(Ok(())) => {}
        AwaitOutcome::Completed(Err(error)) => {
            managed.abandon().await;
            return Err(error.into());
        }
        AwaitOutcome::Cancelled(reason) => {
            managed.abandon().await;
            return Err(QueryTaskError::Cancelled(reason));
        }
    }
    let mut page = match await_with_cancellation(
        &mut cancellation,
        managed.connection.query(&query.sql, &parameters),
    )
    .await
    {
        AwaitOutcome::Completed(Ok(result)) => result,
        AwaitOutcome::Completed(Err(error)) => {
            managed.abandon().await;
            return Err(oracle_query_error(&error).into());
        }
        AwaitOutcome::Cancelled(reason) => {
            managed.abandon().await;
            return Err(QueryTaskError::Cancelled(reason));
        }
    };
    let columns = page.columns.clone();
    if let Err(error) = validate_result_columns(&columns) {
        managed.abandon().await;
        return Err(error.into());
    }
    if columns.len() > MAX_COLUMNS {
        managed.abandon().await;
        return Err(resource_error(
            "oracle_result_too_wide",
            format!("Oracle returned more than {MAX_COLUMNS} columns"),
        )
        .into());
    }
    let schema = wire::QueryStarted {
        columns: columns
            .iter()
            .enumerate()
            .map(|(index, column)| oracle_column(index, column))
            .collect::<Result<_, _>>()?,
    };
    let mut writer = match RetainedWriter::begin(storage, schema, query.retention).await {
        Ok(writer) => writer,
        Err(error) => {
            managed.abandon().await;
            return Err(error.into());
        }
    };
    if let Err(error) = application.inner.operations.started(operation_id).await {
        abort_writer(&mut writer).await;
        managed.abandon().await;
        return Err(error.into());
    }

    let max_rows = query.options.max_rows;
    let max_result_bytes = if query.options.max_result_bytes == 0 {
        DEFAULT_RESULT_BYTES
    } else {
        query.options.max_result_bytes
    };
    let batch_rows = if query.options.target_batch_rows == 0 {
        DEFAULT_BATCH_ROWS
    } else {
        query.options.target_batch_rows
    };
    let batch_bytes = if query.options.target_batch_bytes == 0 {
        DEFAULT_BATCH_BYTES
    } else {
        query.options.target_batch_bytes
    };
    let cursor_id = page.cursor_id;
    let mut pending_rows = Vec::new();
    let mut pending_bytes = 0_u64;
    let mut row_count = 0_u64;
    let mut result_bytes = 0_u64;
    let mut truncated_by_max_rows = false;
    let mut truncated_by_max_result_bytes = false;
    let mut discard_connection = false;

    'pages: loop {
        let has_more = page.has_more_rows;
        for row in std::mem::take(&mut page.rows) {
            if max_rows != 0 && row_count >= max_rows {
                truncated_by_max_rows = true;
                discard_connection = true;
                break 'pages;
            }
            let converted = match await_with_cancellation(
                &mut cancellation,
                oracle_row(&managed.connection, row, &columns),
            )
            .await
            {
                AwaitOutcome::Completed(Ok(row)) => row,
                AwaitOutcome::Completed(Err(error)) => {
                    abort_writer(&mut writer).await;
                    managed.abandon().await;
                    return Err(error.into());
                }
                AwaitOutcome::Cancelled(reason) => {
                    abort_writer(&mut writer).await;
                    managed.abandon().await;
                    return Err(QueryTaskError::Cancelled(reason));
                }
            };
            let row_bytes = u64::try_from(converted.encoded_len())
                .map_err(|_| QueryTaskError::Failed(AppError::internal()))?;
            if result_bytes.saturating_add(row_bytes) > max_result_bytes {
                truncated_by_max_result_bytes = true;
                discard_connection = true;
                break 'pages;
            }
            let entry_bytes = row_batch_entry_bytes(&converted)?;
            let candidate_bytes = pending_bytes
                .saturating_add(if pending_rows.is_empty() {
                    row_batch_prefix_bytes(row_count)
                } else {
                    0
                })
                .saturating_add(entry_bytes);
            if !pending_rows.is_empty()
                && (pending_rows.len()
                    >= usize::try_from(batch_rows)
                        .map_err(|_| QueryTaskError::Failed(AppError::internal()))?
                    || candidate_bytes > u64::from(batch_bytes))
            {
                flush_rows(
                    application,
                    operation_id,
                    &mut writer,
                    &mut pending_rows,
                    row_count,
                )
                .await?;
                pending_bytes = 0;
            }
            if pending_rows.is_empty() {
                pending_bytes = row_batch_prefix_bytes(row_count);
            }
            pending_rows.push(converted);
            pending_bytes = pending_bytes.saturating_add(entry_bytes);
            row_count = row_count
                .checked_add(1)
                .ok_or_else(|| QueryTaskError::Failed(AppError::internal()))?;
            result_bytes = result_bytes
                .checked_add(row_bytes)
                .ok_or_else(|| QueryTaskError::Failed(AppError::internal()))?;
        }
        if !has_more {
            break;
        }
        page = match await_with_cancellation(
            &mut cancellation,
            managed
                .connection
                .fetch_more(cursor_id, &columns, FETCH_ROWS),
        )
        .await
        {
            AwaitOutcome::Completed(Ok(result)) => result,
            AwaitOutcome::Completed(Err(error)) => {
                abort_writer(&mut writer).await;
                managed.abandon().await;
                return Err(oracle_query_error(&error).into());
            }
            AwaitOutcome::Cancelled(reason) => {
                abort_writer(&mut writer).await;
                managed.abandon().await;
                return Err(QueryTaskError::Cancelled(reason));
            }
        };
    }

    if let Err(error) = flush_rows(
        application,
        operation_id,
        &mut writer,
        &mut pending_rows,
        row_count,
    )
    .await
    {
        abort_writer(&mut writer).await;
        managed.abandon().await;
        return Err(error);
    }
    let metadata = match writer
        .finish(wire::QueryCompleted {
            row_count,
            truncated_by_max_rows,
            truncated_by_max_result_bytes,
        })
        .await
    {
        Ok(metadata) => metadata,
        Err(error) => {
            abort_writer(&mut writer).await;
            managed.abandon().await;
            return Err(error.into());
        }
    };
    if discard_connection {
        managed.abandon().await;
    } else {
        finish_read_only_connection(managed).await;
    }
    Ok(metadata)
}

fn row_batch_prefix_bytes(start_row_offset: u64) -> u64 {
    if start_row_offset == 0 {
        0
    } else {
        1_u64.saturating_add(
            u64::try_from(prost::encoding::encoded_len_varint(start_row_offset))
                .unwrap_or(u64::MAX),
        )
    }
}

fn row_batch_entry_bytes(row: &wire::JdbcRow) -> Result<u64, QueryTaskError> {
    let row_bytes = row.encoded_len();
    let length_bytes = prost::encoding::length_delimiter_len(row_bytes);
    u64::try_from(
        1_usize
            .saturating_add(length_bytes)
            .saturating_add(row_bytes),
    )
    .map_err(|_| QueryTaskError::Failed(AppError::internal()))
}

async fn flush_rows(
    application: &Application,
    operation_id: &str,
    writer: &mut RetainedWriter,
    rows: &mut Vec<wire::JdbcRow>,
    row_count: u64,
) -> Result<(), QueryTaskError> {
    if rows.is_empty() {
        return Ok(());
    }
    let row_len = u64::try_from(rows.len()).map_err(|_| AppError::internal())?;
    let start_row_offset = row_count
        .checked_sub(row_len)
        .ok_or_else(AppError::internal)?;
    let batch = wire::RowBatch {
        start_row_offset,
        rows: std::mem::take(rows),
    };
    if batch.encoded_len() > usize::try_from(MAX_BATCH_BYTES).unwrap_or(usize::MAX) {
        return Err(resource_error(
            "oracle_result_batch_too_large",
            "One Oracle result row exceeds the retained-result batch limit",
        )
        .into());
    }
    let byte_count = writer.append(batch).await?;
    application
        .inner
        .operations
        .progress(operation_id, row_count, byte_count)
        .await?;
    Ok(())
}

async fn abort_writer(writer: &mut RetainedWriter) {
    if let Err(error) = writer.abort().await {
        tracing::warn!(error = %error, "Oracle retained-result cleanup failed");
    }
}

async fn finish_read_only_connection(managed: ManagedOracleConnection) {
    match tokio::time::timeout(OPERATION_TIMEOUT, managed.connection.rollback()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "Oracle read-only transaction rollback failed");
            managed.abandon().await;
            return;
        }
        Err(_) => {
            tracing::warn!("Oracle read-only transaction rollback timed out");
            managed.abandon().await;
            return;
        }
    }
    if let Err(error) = managed.close().await {
        tracing::warn!(error = %error, "Oracle connection cleanup failed");
    }
}

fn resource_error(code: impl Into<String>, message: impl Into<String>) -> AppError {
    AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new(code, message),
    )
}

fn oracle_column(index: usize, column: &ColumnInfo) -> Result<wire::JdbcColumn, AppError> {
    let ordinal = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(AppError::internal)?;
    let value_type = oracle_value_type(column);
    let precision = u32::try_from(column.precision)
        .ok()
        .filter(|value| *value > 0);
    let scale = (column.scale != 0).then(|| i32::from(column.scale));
    let display_size = (column.data_size > 0).then_some(column.data_size);
    Ok(wire::JdbcColumn {
        ordinal,
        name: column.name.clone(),
        label: column.name.clone(),
        jdbc_type: oracle_jdbc_type(column.oracle_type),
        jdbc_type_name: oracle_type_name(column.oracle_type).to_owned(),
        value_type: value_type as i32,
        nullability: if column.nullable {
            wire::ColumnNullability::Nullable as i32
        } else {
            wire::ColumnNullability::NoNulls as i32
        },
        precision,
        scale,
        display_size,
        signed: oracle_numeric_type(column.oracle_type).then_some(true),
        catalog_name: None,
        schema_name: column.type_schema.clone(),
        table_name: None,
    })
}

fn validate_result_columns(columns: &[ColumnInfo]) -> Result<(), AppError> {
    if let Some(column) = columns
        .iter()
        .find(|column| !oracle_result_type_supported(column.oracle_type))
    {
        return Err(result_type_not_supported(column.oracle_type));
    }
    Ok(())
}

const fn oracle_result_type_supported(oracle_type: OracleType) -> bool {
    !matches!(
        oracle_type,
        OracleType::BinaryFloat
            | OracleType::BinaryDouble
            | OracleType::Rowid
            | OracleType::Urowid
            | OracleType::Bfile
            | OracleType::Cursor
            | OracleType::Object
            | OracleType::Vector
            | OracleType::IntervalYm
            | OracleType::IntervalDs
    )
}

fn oracle_value_type(column: &ColumnInfo) -> wire::JdbcValueType {
    match column.oracle_type {
        OracleType::Number | OracleType::BinaryInteger
            if column.scale == 0 && (1..=18).contains(&column.precision) =>
        {
            wire::JdbcValueType::SignedInteger
        }
        OracleType::Number | OracleType::BinaryInteger => wire::JdbcValueType::Decimal,
        OracleType::BinaryFloat => wire::JdbcValueType::Float32,
        OracleType::BinaryDouble => wire::JdbcValueType::Float64,
        OracleType::Raw | OracleType::LongRaw | OracleType::Blob => wire::JdbcValueType::Binary,
        OracleType::Date | OracleType::Timestamp | OracleType::TimestampLtz => {
            wire::JdbcValueType::Timestamp
        }
        OracleType::TimestampTz => wire::JdbcValueType::TimestampWithTimeZone,
        OracleType::Json => wire::JdbcValueType::Json,
        OracleType::Boolean => wire::JdbcValueType::Boolean,
        OracleType::Varchar
        | OracleType::Char
        | OracleType::Long
        | OracleType::Clob
        | OracleType::Rowid
        | OracleType::Urowid => wire::JdbcValueType::Text,
        OracleType::Bfile
        | OracleType::Cursor
        | OracleType::Object
        | OracleType::Vector
        | OracleType::IntervalYm
        | OracleType::IntervalDs => wire::JdbcValueType::Opaque,
    }
}

const fn oracle_numeric_type(oracle_type: OracleType) -> bool {
    matches!(
        oracle_type,
        OracleType::Number
            | OracleType::BinaryInteger
            | OracleType::BinaryFloat
            | OracleType::BinaryDouble
    )
}

const fn oracle_jdbc_type(oracle_type: OracleType) -> i32 {
    match oracle_type {
        OracleType::Varchar => 12,
        OracleType::Number => 2,
        OracleType::BinaryInteger => 4,
        OracleType::Long => -1,
        OracleType::Rowid | OracleType::Urowid => -8,
        OracleType::Date | OracleType::Timestamp | OracleType::TimestampLtz => 93,
        OracleType::Raw => -3,
        OracleType::LongRaw => -4,
        OracleType::Char => 1,
        OracleType::BinaryFloat => 6,
        OracleType::BinaryDouble => 8,
        OracleType::Cursor => -10,
        OracleType::Object => 2_002,
        OracleType::Clob => 2_005,
        OracleType::Blob => 2_004,
        OracleType::Bfile => -13,
        OracleType::Json | OracleType::Vector | OracleType::IntervalYm | OracleType::IntervalDs => {
            1_111
        }
        OracleType::TimestampTz => 2_014,
        OracleType::Boolean => 16,
    }
}

const fn oracle_type_name(oracle_type: OracleType) -> &'static str {
    match oracle_type {
        OracleType::Varchar => "VARCHAR2",
        OracleType::Number => "NUMBER",
        OracleType::BinaryInteger => "BINARY_INTEGER",
        OracleType::Long => "LONG",
        OracleType::Rowid => "ROWID",
        OracleType::Date => "DATE",
        OracleType::Raw => "RAW",
        OracleType::LongRaw => "LONG RAW",
        OracleType::Char => "CHAR",
        OracleType::BinaryFloat => "BINARY_FLOAT",
        OracleType::BinaryDouble => "BINARY_DOUBLE",
        OracleType::Cursor => "REF CURSOR",
        OracleType::Object => "OBJECT",
        OracleType::Clob => "CLOB",
        OracleType::Blob => "BLOB",
        OracleType::Bfile => "BFILE",
        OracleType::Json => "JSON",
        OracleType::Vector => "VECTOR",
        OracleType::Timestamp => "TIMESTAMP",
        OracleType::TimestampTz => "TIMESTAMP WITH TIME ZONE",
        OracleType::IntervalYm => "INTERVAL YEAR TO MONTH",
        OracleType::IntervalDs => "INTERVAL DAY TO SECOND",
        OracleType::Urowid => "UROWID",
        OracleType::TimestampLtz => "TIMESTAMP WITH LOCAL TIME ZONE",
        OracleType::Boolean => "BOOLEAN",
    }
}

async fn oracle_row(
    connection: &Connection,
    row: OracleRow,
    columns: &[ColumnInfo],
) -> Result<wire::JdbcRow, AppError> {
    if row.len() != columns.len() {
        return Err(AppError::internal());
    }
    let mut values = Vec::with_capacity(columns.len());
    for (value, column) in row.into_values().into_iter().zip(columns) {
        values.push(oracle_wire_value(connection, value, column).await?);
    }
    Ok(wire::JdbcRow { values })
}

async fn oracle_wire_value(
    connection: &Connection,
    value: OracleValue,
    column: &ColumnInfo,
) -> Result<wire::JdbcValue, AppError> {
    use wire::jdbc_value::Value as WireValue;
    if matches!(value, OracleValue::Null | OracleValue::Lob(LobValue::Null)) {
        return Ok(wire_value(WireValue::NullValue(wire::JdbcNull {})));
    }
    let value_type = oracle_value_type(column);
    let value = match value_type {
        wire::JdbcValueType::Boolean => {
            WireValue::BooleanValue(oracle_bool(&value).ok_or_else(result_decode_error)?)
        }
        wire::JdbcValueType::SignedInteger => {
            WireValue::SignedIntegerValue(oracle_i64(&value).ok_or_else(result_decode_error)?)
        }
        wire::JdbcValueType::UnsignedInteger => WireValue::UnsignedIntegerValue(
            u64::try_from(oracle_i64(&value).ok_or_else(result_decode_error)?)
                .map_err(|_| result_decode_error())?,
        ),
        wire::JdbcValueType::Float32 => {
            WireValue::Float32Value(oracle_f32(&value).ok_or_else(result_decode_error)?)
        }
        wire::JdbcValueType::Float64 => {
            WireValue::Float64Value(oracle_f64(&value).ok_or_else(result_decode_error)?)
        }
        wire::JdbcValueType::Decimal => WireValue::DecimalValue(oracle_decimal_text(&value)?),
        wire::JdbcValueType::Text => {
            WireValue::TextValue(oracle_text(connection, value, column.oracle_type).await?)
        }
        wire::JdbcValueType::Binary => {
            WireValue::BinaryValue(oracle_binary(connection, value).await?)
        }
        wire::JdbcValueType::Date => {
            WireValue::DateValue(oracle_text(connection, value, column.oracle_type).await?)
        }
        wire::JdbcValueType::Time => {
            WireValue::TimeValue(oracle_text(connection, value, column.oracle_type).await?)
        }
        wire::JdbcValueType::Timestamp => {
            WireValue::TimestampValue(oracle_temporal_text(&value, false)?)
        }
        wire::JdbcValueType::TimestampWithTimeZone => {
            WireValue::TimestampWithTimeZoneValue(oracle_temporal_text(&value, true)?)
        }
        wire::JdbcValueType::Json => {
            let text = match value {
                OracleValue::Json(value) => value.to_string(),
                other => oracle_text(connection, other, column.oracle_type).await?,
            };
            validate_result_scalar(text.len())?;
            WireValue::JsonValue(text)
        }
        wire::JdbcValueType::Uuid => {
            WireValue::UuidValue(oracle_text(connection, value, column.oracle_type).await?)
        }
        wire::JdbcValueType::Opaque | wire::JdbcValueType::Unspecified => {
            return Err(result_type_not_supported(column.oracle_type));
        }
    };
    Ok(wire_value(value))
}

fn wire_value(value: wire::jdbc_value::Value) -> wire::JdbcValue {
    wire::JdbcValue { value: Some(value) }
}

fn oracle_bool(value: &OracleValue) -> Option<bool> {
    match value {
        OracleValue::Boolean(value) => Some(*value),
        OracleValue::Integer(value) => Some(*value != 0),
        OracleValue::Number(value) => value.to_i64().ok().map(|value| value != 0),
        OracleValue::String(value) if value == "1" || value.eq_ignore_ascii_case("true") => {
            Some(true)
        }
        OracleValue::String(value) if value == "0" || value.eq_ignore_ascii_case("false") => {
            Some(false)
        }
        OracleValue::String(value) if value.as_bytes() == [1, 1] => Some(true),
        OracleValue::String(value) if value.as_bytes() == [1, 0] => Some(false),
        _ => None,
    }
}

fn oracle_i64(value: &OracleValue) -> Option<i64> {
    match value {
        OracleValue::Integer(value) => Some(*value),
        OracleValue::Float(value) => value.to_string().parse().ok(),
        OracleValue::Number(value) => value.to_i64().ok(),
        OracleValue::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn oracle_f32(value: &OracleValue) -> Option<f32> {
    match value {
        OracleValue::Float(value) => value.to_string().parse().ok(),
        OracleValue::Integer(value) => value.to_string().parse().ok(),
        OracleValue::Number(value) => value.as_str().parse().ok(),
        OracleValue::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn oracle_f64(value: &OracleValue) -> Option<f64> {
    match value {
        OracleValue::Float(value) => Some(*value),
        OracleValue::Integer(value) => value.to_string().parse().ok(),
        OracleValue::Number(value) => value.to_f64().ok(),
        OracleValue::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn oracle_decimal_text(value: &OracleValue) -> Result<String, AppError> {
    let value = match value {
        OracleValue::Number(value) => value.as_str().to_owned(),
        OracleValue::Integer(value) => value.to_string(),
        OracleValue::Float(value) => value.to_string(),
        OracleValue::String(value) => value.clone(),
        _ => return Err(result_decode_error()),
    };
    validate_result_scalar(value.len())?;
    Ok(value)
}

async fn oracle_text(
    connection: &Connection,
    value: OracleValue,
    oracle_type: OracleType,
) -> Result<String, AppError> {
    let value = match value {
        OracleValue::String(value) => value,
        OracleValue::Bytes(value) => String::from_utf8(value).map_err(|_| result_decode_error())?,
        OracleValue::Integer(value) => value.to_string(),
        OracleValue::Float(value) => value.to_string(),
        OracleValue::Number(value) => value.as_str().to_owned(),
        OracleValue::Date(value) => format_oracle_date(value),
        OracleValue::Timestamp(value) if value.has_timezone() => format_oracle_timestamp_tz(value)?,
        OracleValue::Timestamp(value) => format_oracle_timestamp(value),
        OracleValue::RowId(value) => format!("{value}"),
        OracleValue::Boolean(value) => value.to_string(),
        OracleValue::Json(value) => value.to_string(),
        OracleValue::Lob(value) => oracle_lob_text(connection, value, oracle_type).await?,
        OracleValue::Null => return Err(result_decode_error()),
        _ => return Err(result_type_not_supported(oracle_type)),
    };
    validate_result_scalar(value.len())?;
    Ok(value)
}

async fn oracle_binary(connection: &Connection, value: OracleValue) -> Result<Vec<u8>, AppError> {
    let value = match value {
        OracleValue::Bytes(value) => value,
        OracleValue::String(value) => value.into_bytes(),
        OracleValue::Lob(LobValue::Inline(value)) => {
            validate_result_scalar(value.len())?;
            value.to_vec()
        }
        OracleValue::Lob(LobValue::Empty) => Vec::new(),
        OracleValue::Lob(LobValue::Locator(locator)) => {
            if locator.size() > u64::try_from(MAX_SCALAR_BYTES).unwrap_or(u64::MAX) {
                return Err(result_scalar_too_large());
            }
            match connection
                .read_lob(&locator)
                .await
                .map_err(|error| oracle_query_error(&error))?
            {
                LobData::Bytes(value) => {
                    validate_result_scalar(value.len())?;
                    value.to_vec()
                }
                LobData::String(value) => {
                    validate_result_scalar(value.len())?;
                    value.into_bytes()
                }
            }
        }
        OracleValue::Lob(LobValue::Null) | OracleValue::Null => {
            return Err(result_decode_error());
        }
        _ => return Err(result_decode_error()),
    };
    validate_result_scalar(value.len())?;
    Ok(value)
}

async fn oracle_lob_text(
    connection: &Connection,
    value: LobValue,
    oracle_type: OracleType,
) -> Result<String, AppError> {
    match value {
        LobValue::Inline(value) => {
            validate_result_scalar(value.len())?;
            String::from_utf8(value.to_vec()).map_err(|_| result_decode_error())
        }
        LobValue::Empty => Ok(String::new()),
        LobValue::Null => Err(result_decode_error()),
        LobValue::Locator(locator) => {
            if locator.size() > u64::try_from(MAX_SCALAR_BYTES).unwrap_or(u64::MAX) {
                return Err(result_scalar_too_large());
            }
            match connection
                .read_lob(&locator)
                .await
                .map_err(|error| oracle_query_error(&error))?
            {
                LobData::String(value) => {
                    validate_result_scalar(value.len())?;
                    Ok(value)
                }
                LobData::Bytes(value) if matches!(oracle_type, OracleType::Clob) => {
                    validate_result_scalar(value.len())?;
                    String::from_utf8(value.to_vec()).map_err(|_| result_decode_error())
                }
                LobData::Bytes(value) => {
                    let encoded_len = value.len().saturating_mul(4).div_ceil(3);
                    validate_result_scalar(encoded_len)?;
                    Ok(BASE64_STANDARD.encode(value))
                }
            }
        }
    }
}

fn oracle_temporal_text(value: &OracleValue, require_timezone: bool) -> Result<String, AppError> {
    let value = match value {
        OracleValue::Date(value) if !require_timezone => format_oracle_date(*value),
        OracleValue::Timestamp(value) if require_timezone => format_oracle_timestamp_tz(*value)?,
        OracleValue::Timestamp(value) => format_oracle_timestamp(*value),
        OracleValue::String(value) => value.clone(),
        _ => return Err(result_decode_error()),
    };
    validate_result_scalar(value.len())?;
    Ok(value)
}

fn format_oracle_date(value: OracleDate) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        value.year, value.month, value.day, value.hour, value.minute, value.second
    )
}

fn format_oracle_timestamp(value: OracleTimestamp) -> String {
    let mut result = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        value.year, value.month, value.day, value.hour, value.minute, value.second
    );
    if value.microsecond != 0 {
        let _ = write!(&mut result, ".{:06}", value.microsecond);
    }
    result
}

fn format_oracle_timestamp_tz(value: OracleTimestamp) -> Result<String, AppError> {
    let offset_minutes = i32::from(value.tz_minute_offset);
    let offset_hours = i32::from(value.tz_hour_offset);
    if offset_minutes.unsigned_abs() > 59
        || (offset_hours > 0 && offset_minutes < 0)
        || (offset_hours < 0 && offset_minutes > 0)
    {
        return Err(result_decode_error());
    }
    let offset_seconds = offset_hours
        .checked_mul(3_600)
        .and_then(|seconds| {
            offset_minutes
                .checked_mul(60)
                .and_then(|minutes| seconds.checked_add(minutes))
        })
        .filter(|seconds| (-12 * 3_600..=14 * 3_600).contains(seconds))
        .ok_or_else(result_decode_error)?;
    let offset = FixedOffset::east_opt(offset_seconds).ok_or_else(result_decode_error)?;
    let utc = NaiveDate::from_ymd_opt(value.year, u32::from(value.month), u32::from(value.day))
        .and_then(|date| {
            date.and_hms_micro_opt(
                u32::from(value.hour),
                u32::from(value.minute),
                u32::from(value.second),
                value.microsecond,
            )
        })
        .ok_or_else(result_decode_error)?;
    // Oracle's TSTZ wire value carries UTC fields plus the original numeric offset.
    let local = utc
        .checked_add_signed(chrono::TimeDelta::seconds(i64::from(offset_seconds)))
        .ok_or_else(result_decode_error)?;
    Ok(format!(
        "{}{}",
        local.format("%Y-%m-%dT%H:%M:%S%.f"),
        offset
    ))
}

fn oracle_display(value: &OracleValue) -> String {
    let display = value.to_string();
    if display.len() <= MAX_SCALAR_BYTES {
        display
    } else {
        "[value exceeds display limit]".to_owned()
    }
}

fn validate_result_scalar(size: usize) -> Result<(), AppError> {
    if size > MAX_SCALAR_BYTES {
        Err(result_scalar_too_large())
    } else {
        Ok(())
    }
}

fn result_scalar_too_large() -> AppError {
    resource_error(
        "oracle_scalar_too_large",
        format!("An Oracle value exceeds {MAX_SCALAR_BYTES} bytes"),
    )
}

fn result_decode_error() -> AppError {
    AppError::unavailable(
        "oracle_result_decode_failed",
        "An Oracle result value could not be decoded safely",
    )
}

fn result_type_not_supported(oracle_type: OracleType) -> AppError {
    AppError::invalid(
        "oracle_result_type_not_supported",
        format!(
            "The native Oracle driver does not support result type {}",
            oracle_type_name(oracle_type)
        ),
    )
}

fn console_column(index: usize, column: &ColumnInfo) -> Result<ResultColumn, AppError> {
    let column = oracle_column(index, column)?;
    let value_type = match wire::JdbcValueType::try_from(column.value_type) {
        Ok(wire::JdbcValueType::Boolean) => JdbcValueType::Boolean,
        Ok(wire::JdbcValueType::SignedInteger) => JdbcValueType::SignedInteger,
        Ok(wire::JdbcValueType::UnsignedInteger) => JdbcValueType::UnsignedInteger,
        Ok(wire::JdbcValueType::Float32) => JdbcValueType::Float32,
        Ok(wire::JdbcValueType::Float64) => JdbcValueType::Float64,
        Ok(wire::JdbcValueType::Decimal) => JdbcValueType::Decimal,
        Ok(wire::JdbcValueType::Text) => JdbcValueType::Text,
        Ok(wire::JdbcValueType::Binary) => JdbcValueType::Binary,
        Ok(wire::JdbcValueType::Date) => JdbcValueType::Date,
        Ok(wire::JdbcValueType::Time) => JdbcValueType::Time,
        Ok(wire::JdbcValueType::Timestamp) => JdbcValueType::Timestamp,
        Ok(wire::JdbcValueType::TimestampWithTimeZone) => JdbcValueType::TimestampWithTimeZone,
        Ok(wire::JdbcValueType::Json) => JdbcValueType::Json,
        Ok(wire::JdbcValueType::Uuid) => JdbcValueType::Uuid,
        Ok(wire::JdbcValueType::Opaque) => JdbcValueType::Opaque,
        Ok(wire::JdbcValueType::Unspecified) | Err(_) => return Err(AppError::internal()),
    };
    let nullability = match wire::ColumnNullability::try_from(column.nullability) {
        Ok(wire::ColumnNullability::Unknown) => ColumnNullability::Unknown,
        Ok(wire::ColumnNullability::NoNulls) => ColumnNullability::NoNulls,
        Ok(wire::ColumnNullability::Nullable) => ColumnNullability::Nullable,
        Err(_) => return Err(AppError::internal()),
    };
    Ok(ResultColumn {
        ordinal: column.ordinal,
        label: column.label,
        name: column.name,
        jdbc_type: column.jdbc_type,
        jdbc_type_name: column.jdbc_type_name,
        value_type,
        nullability,
        precision: column.precision,
        scale: column.scale,
        display_size: column.display_size,
        signed: column.signed,
        catalog_name: column.catalog_name,
        schema_name: column.schema_name,
        table_name: column.table_name,
    })
}

async fn console_row(
    connection: &Connection,
    row: OracleRow,
    columns: &[ColumnInfo],
) -> Result<ResultRow, AppError> {
    let row = oracle_row(connection, row, columns).await?;
    Ok(ResultRow {
        values: row
            .values
            .into_iter()
            .map(contract_value)
            .collect::<Result<_, _>>()?,
    })
}

fn contract_value(value: wire::JdbcValue) -> Result<JdbcValue, AppError> {
    use wire::jdbc_value::Value;
    match value.value.ok_or_else(AppError::internal)? {
        Value::NullValue(_) => Ok(JdbcValue::Null),
        Value::BooleanValue(value) => Ok(JdbcValue::Boolean { value }),
        Value::SignedIntegerValue(value) => Ok(JdbcValue::SignedInteger {
            value: value.to_string(),
        }),
        Value::UnsignedIntegerValue(value) => Ok(JdbcValue::UnsignedInteger {
            value: value.to_string(),
        }),
        Value::Float32Value(value) => Ok(JdbcValue::Float32 {
            value: value.to_string(),
        }),
        Value::Float64Value(value) => Ok(JdbcValue::Float64 {
            value: value.to_string(),
        }),
        Value::DecimalValue(value) => Ok(JdbcValue::Decimal { value }),
        Value::TextValue(value) => Ok(JdbcValue::Text { value }),
        Value::BinaryValue(value) => Ok(JdbcValue::Binary {
            value: BASE64_STANDARD.encode(value),
        }),
        Value::DateValue(value) => Ok(JdbcValue::Date { value }),
        Value::TimeValue(value) => Ok(JdbcValue::Time { value }),
        Value::TimestampValue(value) => Ok(JdbcValue::Timestamp { value }),
        Value::TimestampWithTimeZoneValue(value) => Ok(JdbcValue::TimestampWithTimeZone { value }),
        Value::JsonValue(value) => Ok(JdbcValue::Json { value }),
        Value::UuidValue(value) => Ok(JdbcValue::Uuid { value }),
        Value::OpaqueValue(value) => Ok(JdbcValue::Opaque {
            type_name: value.type_name,
            display_value: value.display_value,
        }),
    }
}

fn console_row_retained_bytes(row: &ResultRow) -> u64 {
    let mut bytes = u64::try_from(size_of::<ResultRow>()).unwrap_or(u64::MAX);
    bytes = bytes.saturating_add(
        u64::try_from(row.values.capacity())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(size_of::<JdbcValue>()).unwrap_or(u64::MAX)),
    );
    for value in &row.values {
        let value_bytes = match value {
            JdbcValue::Null | JdbcValue::Boolean { .. } => 0,
            JdbcValue::SignedInteger { value }
            | JdbcValue::UnsignedInteger { value }
            | JdbcValue::Float32 { value }
            | JdbcValue::Float64 { value }
            | JdbcValue::Decimal { value }
            | JdbcValue::Text { value }
            | JdbcValue::Binary { value }
            | JdbcValue::Date { value }
            | JdbcValue::Time { value }
            | JdbcValue::Timestamp { value }
            | JdbcValue::TimestampWithTimeZone { value }
            | JdbcValue::Json { value }
            | JdbcValue::Uuid { value } => value.capacity(),
            JdbcValue::Opaque {
                type_name,
                display_value,
            } => type_name
                .capacity()
                .saturating_add(display_value.capacity()),
        };
        bytes = bytes.saturating_add(u64::try_from(value_bytes).unwrap_or(u64::MAX));
    }
    bytes
}

fn reserve_console_result_bytes(total: &mut u64, row: &ResultRow) -> Result<(), AppError> {
    let next = total.saturating_add(console_row_retained_bytes(row));
    if next > MAX_CONSOLE_RESULT_BYTES {
        return Err(resource_error(
            "oracle_console_result_too_large",
            format!(
                "Oracle Console results are limited to {MAX_CONSOLE_RESULT_BYTES} retained bytes"
            ),
        ));
    }
    *total = next;
    Ok(())
}

pub(crate) async fn execute_update(
    resolved: ResolvedDatasourceConnection,
    sql: String,
    cancellation: CancellationToken,
) -> Result<u64, DatabaseWriteError> {
    if cancellation.is_cancelled() {
        return Err(DatabaseWriteError::not_started(oracle_write_cancelled(
            false,
        )));
    }
    let sql = validate_single_write_sql(&sql).map_err(DatabaseWriteError::not_started)?;
    if resolved.connection.read_only {
        return Err(DatabaseWriteError::not_started(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "datasource_read_only",
                "The datasource connection is configured as read-only",
            ),
        )));
    }
    let open = open_resolved_connection(&resolved);
    tokio::pin!(open);
    let managed = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            return Err(DatabaseWriteError::not_started(oracle_write_cancelled(false)));
        }
        result = &mut open => result.map_err(DatabaseWriteError::not_started)?,
    };
    if cancellation.is_cancelled() {
        managed.abandon().await;
        return Err(DatabaseWriteError::not_started(oracle_write_cancelled(
            false,
        )));
    }

    let Some(result) = await_with_token(&cancellation, managed.connection.execute(&sql, &[])).await
    else {
        managed.abandon().await;
        return Err(DatabaseWriteError::unknown(oracle_write_cancelled(true)));
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            managed.abandon().await;
            return Err(DatabaseWriteError::unknown(AppError::unavailable(
                "database_write_outcome_unknown",
                format!(
                    "Oracle reported an error after write dispatch; partial effects cannot be excluded, so do not retry blindly: {}",
                    safe_oracle_error(&error)
                ),
            )));
        }
    };
    let affected_rows = result.rows_affected;
    let Some(commit_result) = await_with_token(&cancellation, managed.connection.commit()).await
    else {
        managed.abandon().await;
        return Err(DatabaseWriteError::unknown(oracle_write_cancelled(true)));
    };
    if let Err(error) = commit_result {
        managed.abandon().await;
        return Err(DatabaseWriteError::unknown(AppError::unavailable(
            "database_write_outcome_unknown",
            format!(
                "The Oracle commit outcome is unknown; do not retry blindly: {}",
                safe_oracle_error(&error)
            ),
        )));
    }
    if let Err(error) = managed.close().await {
        tracing::warn!(error = %error, "Oracle write connection cleanup failed after commit");
    }
    Ok(affected_rows)
}

async fn await_with_token<F, T>(cancellation: &CancellationToken, future: F) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(future);
    tokio::select! {
        biased;
        () = cancellation.cancelled() => None,
        output = &mut future => Some(output),
    }
}

fn validate_single_write_sql(sql: &str) -> Result<String, AppError> {
    validate_sql_text(sql)?;
    let mut statements = split_oracle_script(sql)?;
    if statements.len() != 1 {
        return Err(AppError::invalid(
            "invalid_database_write",
            "Exactly one Oracle write statement is required",
        ));
    }
    let statement = statements.pop().expect("length checked above");
    let first = first_sql_keyword(&statement)?;
    if !matches!(
        first.as_deref(),
        Some(
            "INSERT"
                | "UPDATE"
                | "DELETE"
                | "MERGE"
                | "CREATE"
                | "ALTER"
                | "DROP"
                | "TRUNCATE"
                | "RENAME"
                | "GRANT"
                | "REVOKE"
                | "ANALYZE"
                | "CALL"
                | "BEGIN"
                | "DECLARE"
        )
    ) {
        return Err(AppError::invalid(
            "database_write_statement_required",
            "The confirmed Oracle write surface accepts one DML, DDL, grant, call, or PL/SQL statement",
        ));
    }
    Ok(statement)
}

fn oracle_write_cancelled(dispatched: bool) -> AppError {
    if dispatched {
        AppError::unavailable(
            "database_write_outcome_unknown",
            "The Oracle write was interrupted after dispatch; do not retry it blindly",
        )
    } else {
        AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "database_write_cancelled",
                "The Oracle write was cancelled before dispatch",
            ),
        )
    }
}

fn safe_oracle_error(error: &OracleError) -> String {
    match error {
        OracleError::OracleError { .. }
        | OracleError::ServerError { .. }
        | OracleError::SqlError(_) => error.to_string(),
        _ => "the protocol session ended unexpectedly".to_owned(),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn execute_console(
    application: &Application,
    request: NativeConsoleRequest,
    mut cancellation: watch::Receiver<CancellationRequest>,
    force_read_only: bool,
) -> Result<Vec<NativeConsoleResult>, AppError> {
    let (statements, page_offset, page_end) = prepare_console_statements(&request)?;
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(oracle_console_cancelled(reason));
    }
    if force_read_only {
        validate_read_only_statements(&statements, "agent read query")?;
    }
    let resolved = resolve_native_connection(application, &request.datasource_id).await?;
    if resolved.connection.read_only && !force_read_only {
        validate_read_only_statements(&statements, "read-only datasource")?;
    }
    let effective_read_only = force_read_only || resolved.connection.read_only;
    let managed =
        match await_with_cancellation(&mut cancellation, open_resolved_connection(&resolved)).await
        {
            AwaitOutcome::Completed(result) => result?,
            AwaitOutcome::Cancelled(reason) => return Err(oracle_console_cancelled(reason)),
        };
    if effective_read_only {
        match start_read_only_transaction(&managed, &mut cancellation).await {
            AwaitOutcome::Completed(Ok(())) => {}
            AwaitOutcome::Completed(Err(error)) => {
                managed.abandon().await;
                return Err(error);
            }
            AwaitOutcome::Cancelled(reason) => {
                managed.abandon().await;
                return Err(oracle_console_cancelled(reason));
            }
        }
    }

    let mut results = Vec::new();
    let mut retained_result_bytes = 0_u64;
    for (index, sql) in statements.into_iter().enumerate() {
        let statement_read_only = validate_read_sql(&sql).is_ok();
        let statement_sequence = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or_else(AppError::internal)?;
        let started = Instant::now();
        let execution =
            await_with_cancellation(&mut cancellation, managed.connection.execute(&sql, &[])).await;
        let mut query_result = match execution {
            AwaitOutcome::Completed(Ok(result)) => result,
            AwaitOutcome::Completed(Err(error)) if oracle_database_error(&error) => {
                let error = oracle_query_error(&error);
                results.push(console_failure_result(
                    statement_sequence,
                    sql,
                    &error,
                    elapsed_millis(started),
                ));
                if !request.error_continue {
                    break;
                }
                continue;
            }
            AwaitOutcome::Completed(Err(error)) => {
                managed.abandon().await;
                return Err(oracle_console_connection_error(statement_read_only, &error));
            }
            AwaitOutcome::Cancelled(reason) => {
                managed.abandon().await;
                return Err(oracle_console_interrupted(statement_read_only, reason));
            }
        };
        let columns = query_result.columns.clone();
        if let Err(error) = validate_result_columns(&columns) {
            managed.abandon().await;
            return Err(oracle_console_post_dispatch_error(
                statement_read_only,
                error,
            ));
        }
        if columns.len() > MAX_COLUMNS {
            managed.abandon().await;
            return Err(resource_error(
                "oracle_result_too_wide",
                format!("Oracle returned more than {MAX_COLUMNS} columns"),
            ));
        }
        let tabular = !columns.is_empty();
        if tabular {
            let retain = request.result_set_id.is_none_or(|selected| selected == 1);
            let converted_columns = if retain {
                columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| console_column(index, column))
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };
            let cursor_id = query_result.cursor_id;
            let mut row_count = 0_u64;
            let mut rows = Vec::new();
            'pages: loop {
                let has_more = query_result.has_more_rows;
                for row in std::mem::take(&mut query_result.rows) {
                    if row_count >= MAX_CONSOLE_ROWS {
                        managed.abandon().await;
                        return Err(resource_error(
                            "oracle_console_row_limit_exceeded",
                            format!("Oracle Console results cannot exceed {MAX_CONSOLE_ROWS} rows"),
                        ));
                    }
                    if retain && (page_offset..page_end).contains(&row_count) {
                        let row = match await_with_cancellation(
                            &mut cancellation,
                            console_row(&managed.connection, row, &columns),
                        )
                        .await
                        {
                            AwaitOutcome::Completed(Ok(row)) => row,
                            AwaitOutcome::Completed(Err(error)) => {
                                managed.abandon().await;
                                return Err(oracle_console_post_dispatch_error(
                                    statement_read_only,
                                    error,
                                ));
                            }
                            AwaitOutcome::Cancelled(reason) => {
                                managed.abandon().await;
                                return Err(oracle_console_interrupted(
                                    statement_read_only,
                                    reason,
                                ));
                            }
                        };
                        reserve_console_result_bytes(&mut retained_result_bytes, &row)?;
                        rows.push(row);
                    }
                    row_count = row_count.checked_add(1).ok_or_else(AppError::internal)?;
                }
                if !has_more {
                    break 'pages;
                }
                query_result = match await_with_cancellation(
                    &mut cancellation,
                    managed
                        .connection
                        .fetch_more(cursor_id, &columns, FETCH_ROWS),
                )
                .await
                {
                    AwaitOutcome::Completed(Ok(result)) => result,
                    AwaitOutcome::Completed(Err(error)) => {
                        managed.abandon().await;
                        return Err(oracle_console_connection_error(statement_read_only, &error));
                    }
                    AwaitOutcome::Cancelled(reason) => {
                        managed.abandon().await;
                        return Err(oracle_console_interrupted(statement_read_only, reason));
                    }
                };
            }
            if retain {
                results.push(NativeConsoleResult {
                    statement_sequence,
                    result_set_id: Some(1),
                    sql,
                    success: true,
                    message: "Statement executed successfully".to_owned(),
                    update_count: 0,
                    columns: converted_columns,
                    rows,
                    row_count,
                    has_more: row_count > page_end,
                    duration_ms: elapsed_millis(started),
                    error: None,
                });
            }
        } else if request.result_set_id.is_none() {
            let update_count = query_result.rows_affected;
            if !effective_read_only {
                match await_with_cancellation(
                    &mut cancellation,
                    tokio::time::timeout(OPERATION_TIMEOUT, managed.connection.commit()),
                )
                .await
                {
                    AwaitOutcome::Completed(Ok(Ok(()))) => {}
                    AwaitOutcome::Completed(Ok(Err(error))) => {
                        managed.abandon().await;
                        return Err(oracle_console_write_outcome_unknown(Some(&error)));
                    }
                    AwaitOutcome::Completed(Err(_)) | AwaitOutcome::Cancelled(_) => {
                        managed.abandon().await;
                        return Err(oracle_console_write_outcome_unknown(None));
                    }
                }
            }
            results.push(NativeConsoleResult {
                statement_sequence,
                result_set_id: None,
                sql,
                success: true,
                message: "Statement executed successfully".to_owned(),
                update_count,
                columns: Vec::new(),
                rows: Vec::new(),
                row_count: 0,
                has_more: false,
                duration_ms: elapsed_millis(started),
                error: None,
            });
        }
    }

    if effective_read_only {
        finish_read_only_connection(managed).await;
    } else if let Err(error) = managed.close().await {
        tracing::warn!(error = %error, "Oracle Console connection cleanup failed");
    }
    Ok(results)
}

fn prepare_console_statements(
    request: &NativeConsoleRequest,
) -> Result<(Vec<String>, u64, u64), AppError> {
    if request.page_no == 0 {
        return Err(AppError::invalid(
            "invalid_oracle_console_request",
            "pageNo must be greater than zero",
        ));
    }
    let page_size = if request.page_size_all {
        MAX_CONSOLE_PAGE_SIZE
    } else {
        request.page_size
    };
    if page_size == 0 || page_size > MAX_CONSOLE_PAGE_SIZE {
        return Err(AppError::invalid(
            "invalid_oracle_console_request",
            format!("pageSize must be between 1 and {MAX_CONSOLE_PAGE_SIZE}"),
        ));
    }
    validate_sql_text(&request.sql)?;
    let mut statements = if request.single || looks_like_plsql(&request.sql) {
        vec![normalize_preserved_statement(&request.sql)?]
    } else {
        split_oracle_script(&request.sql)?
    };
    if statements.is_empty() || statements.len() > MAX_CONSOLE_STATEMENTS {
        return Err(AppError::invalid(
            "invalid_oracle_console_request",
            format!("Oracle Console accepts between 1 and {MAX_CONSOLE_STATEMENTS} statements"),
        ));
    }
    if request.explain {
        for statement in &mut statements {
            if !is_read_candidate(statement)? {
                return Err(AppError::invalid(
                    "invalid_oracle_console_request",
                    "Oracle EXPLAIN accepts query statements only",
                ));
            }
            *statement = format!("EXPLAIN PLAN FOR {statement}");
        }
    }
    let page_offset = u64::from(request.page_no - 1)
        .checked_mul(u64::from(page_size))
        .ok_or_else(|| {
            AppError::invalid(
                "invalid_oracle_console_request",
                "The requested result page is too large",
            )
        })?;
    let page_end = page_offset
        .checked_add(u64::from(page_size))
        .ok_or_else(|| {
            AppError::invalid(
                "invalid_oracle_console_request",
                "The requested result page is too large",
            )
        })?;
    Ok((statements, page_offset, page_end))
}

fn validate_read_only_statements(statements: &[String], source: &str) -> Result<(), AppError> {
    for statement in statements {
        validate_read_sql(statement).map_err(|_| {
            AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "datasource_read_only",
                    format!("The Oracle {source} accepts read-only SELECT statements"),
                ),
            )
        })?;
    }
    Ok(())
}

fn oracle_database_error(error: &OracleError) -> bool {
    matches!(
        error,
        OracleError::OracleError { .. }
            | OracleError::ServerError { .. }
            | OracleError::SqlError(_)
            | OracleError::NoDataFound
    )
}

fn console_failure_result(
    statement_sequence: u32,
    sql: String,
    error: &AppError,
    duration_ms: u64,
) -> NativeConsoleResult {
    NativeConsoleResult {
        statement_sequence,
        result_set_id: None,
        sql,
        success: false,
        message: error.api_error().message,
        update_count: 0,
        columns: Vec::new(),
        rows: Vec::new(),
        row_count: 0,
        has_more: false,
        duration_ms,
        error: Some(error.api_error()),
    }
}

fn oracle_console_cancelled(reason: Option<String>) -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "oracle_console_cancelled",
            reason.unwrap_or_else(|| "The Oracle Console execution was cancelled".to_owned()),
        ),
    )
}

fn oracle_console_write_outcome_unknown(error: Option<&OracleError>) -> AppError {
    let message = error.map_or_else(
        || "The Oracle write was interrupted after dispatch; do not retry it blindly".to_owned(),
        |error| {
            format!(
                "The Oracle write outcome is unknown after dispatch; do not retry it blindly: {}",
                safe_oracle_error(error)
            )
        },
    );
    AppError::unavailable("database_write_outcome_unknown", message)
}

fn oracle_console_interrupted(statement_read_only: bool, reason: Option<String>) -> AppError {
    if statement_read_only {
        oracle_console_cancelled(reason)
    } else {
        oracle_console_write_outcome_unknown(None)
    }
}

fn oracle_console_connection_error(statement_read_only: bool, error: &OracleError) -> AppError {
    if statement_read_only {
        oracle_query_error(error)
    } else {
        oracle_console_write_outcome_unknown(Some(error))
    }
}

fn oracle_console_post_dispatch_error(statement_read_only: bool, error: AppError) -> AppError {
    if statement_read_only {
        error
    } else {
        oracle_console_write_outcome_unknown(None)
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn normalize_preserved_statement(sql: &str) -> Result<String, AppError> {
    let mut sql = sql.trim().to_owned();
    if sql.lines().last().is_some_and(|line| line.trim() == "/") {
        let index = sql.rfind('/').ok_or_else(AppError::internal)?;
        sql.truncate(index);
        sql = sql.trim_end().to_owned();
    }
    if sql.is_empty() {
        return Err(AppError::invalid(
            "invalid_oracle_console_request",
            "sql must contain at least one Oracle statement",
        ));
    }
    Ok(sql)
}

fn looks_like_plsql(sql: &str) -> bool {
    let words = oracle_sql_words(sql).unwrap_or_default();
    matches!(words.first().map(String::as_str), Some("BEGIN" | "DECLARE"))
        || (words.first().is_some_and(|word| word == "CREATE")
            && words.iter().take(8).any(|word| {
                matches!(
                    word.as_str(),
                    "FUNCTION" | "PROCEDURE" | "TRIGGER" | "PACKAGE" | "TYPE"
                )
            }))
}

fn first_sql_keyword(sql: &str) -> Result<Option<String>, AppError> {
    Ok(oracle_sql_words(sql)?.into_iter().next())
}

fn oracle_sql_words(sql: &str) -> Result<Vec<String>, AppError> {
    let bytes = sql.as_bytes();
    let mut words = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => skip_quoted(bytes, &mut index, b'\'', b'\'')?,
            b'"' => skip_quoted(bytes, &mut index, b'"', b'"')?,
            b'-' if bytes.get(index + 1) == Some(&b'-') => skip_line_comment(bytes, &mut index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                skip_block_comment(bytes, &mut index)?;
            }
            b'q' | b'Q' if bytes.get(index + 1) == Some(&b'\'') => {
                skip_oracle_q_quote(bytes, &mut index)?;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while bytes.get(index).is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'#')
                }) {
                    index += 1;
                }
                words.push(sql[start..index].to_ascii_uppercase());
            }
            _ => index += 1,
        }
    }
    Ok(words)
}

fn split_oracle_script(sql: &str) -> Result<Vec<String>, AppError> {
    if looks_like_plsql_without_split(sql) {
        return Ok(vec![normalize_preserved_statement(sql)?]);
    }
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => skip_quoted(bytes, &mut index, b'\'', b'\'')?,
            b'"' => skip_quoted(bytes, &mut index, b'"', b'"')?,
            b'-' if bytes.get(index + 1) == Some(&b'-') => skip_line_comment(bytes, &mut index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                skip_block_comment(bytes, &mut index)?;
            }
            b'q' | b'Q' if bytes.get(index + 1) == Some(&b'\'') => {
                skip_oracle_q_quote(bytes, &mut index)?;
            }
            b';' => {
                let statement = sql[start..index].trim();
                if !statement.is_empty() {
                    statements.push(statement.to_owned());
                }
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    let statement = sql[start..].trim();
    if !statement.is_empty() {
        statements.push(statement.to_owned());
    }
    Ok(statements)
}

fn looks_like_plsql_without_split(sql: &str) -> bool {
    let prefix = sql
        .trim_start_matches(|character: char| character.is_whitespace())
        .get(..256)
        .unwrap_or(sql)
        .to_ascii_uppercase();
    prefix.starts_with("BEGIN")
        || prefix.starts_with("DECLARE")
        || (prefix.starts_with("CREATE")
            && [" FUNCTION", " PROCEDURE", " TRIGGER", " PACKAGE", " TYPE"]
                .iter()
                .any(|keyword| prefix.contains(keyword)))
}

fn skip_quoted(
    bytes: &[u8],
    index: &mut usize,
    delimiter: u8,
    escaped_delimiter: u8,
) -> Result<(), AppError> {
    *index += 1;
    while *index < bytes.len() {
        if bytes[*index] == delimiter {
            if bytes.get(*index + 1) == Some(&escaped_delimiter) {
                *index += 2;
                continue;
            }
            *index += 1;
            return Ok(());
        }
        *index += 1;
    }
    Err(AppError::invalid(
        "invalid_query_request",
        "Oracle SQL contains an unterminated quoted value",
    ))
}

fn skip_oracle_q_quote(bytes: &[u8], index: &mut usize) -> Result<(), AppError> {
    let opener = *bytes.get(*index + 2).ok_or_else(|| {
        AppError::invalid(
            "invalid_query_request",
            "Oracle SQL contains an invalid alternative quote",
        )
    })?;
    let closer = match opener {
        b'[' => b']',
        b'{' => b'}',
        b'(' => b')',
        b'<' => b'>',
        other => other,
    };
    *index += 3;
    while *index + 1 < bytes.len() {
        if bytes[*index] == closer && bytes[*index + 1] == b'\'' {
            *index += 2;
            return Ok(());
        }
        *index += 1;
    }
    Err(AppError::invalid(
        "invalid_query_request",
        "Oracle SQL contains an unterminated alternative quote",
    ))
}

fn skip_line_comment(bytes: &[u8], index: &mut usize) {
    *index += 2;
    while *index < bytes.len() && !matches!(bytes[*index], b'\r' | b'\n') {
        *index += 1;
    }
}

fn skip_block_comment(bytes: &[u8], index: &mut usize) -> Result<(), AppError> {
    *index += 2;
    while *index + 1 < bytes.len() {
        if bytes[*index] == b'*' && bytes[*index + 1] == b'/' {
            *index += 2;
            return Ok(());
        }
        *index += 1;
    }
    Err(AppError::invalid(
        "invalid_query_request",
        "Oracle SQL contains an unterminated block comment",
    ))
}

async fn metadata_query(
    application: &Application,
    datasource_id: &str,
    sql: &str,
    parameters: Vec<OracleValue>,
) -> Result<OracleQueryResult, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let managed = open_resolved_connection(&resolved).await?;
    let result = tokio::time::timeout(
        OPERATION_TIMEOUT,
        query_all(&managed.connection, sql, &parameters, MAX_METADATA_ROWS),
    )
    .await;
    match result {
        Ok(Ok(result)) => {
            if let Err(error) = managed.close().await {
                tracing::warn!(error = %error, "Oracle metadata connection cleanup failed");
            }
            Ok(result)
        }
        Ok(Err(error)) => {
            managed.abandon().await;
            Err(error)
        }
        Err(_) => {
            managed.abandon().await;
            Err(oracle_operation_timeout("metadata query"))
        }
    }
}

async fn query_all(
    connection: &Connection,
    sql: &str,
    parameters: &[OracleValue],
    max_rows: usize,
) -> Result<OracleQueryResult, AppError> {
    let mut page = connection
        .query(sql, parameters)
        .await
        .map_err(|error| oracle_query_error(&error))?;
    let columns = page.columns.clone();
    validate_result_columns(&columns)?;
    if columns.len() > MAX_COLUMNS {
        return Err(resource_error(
            "oracle_result_too_wide",
            format!("Oracle returned more than {MAX_COLUMNS} columns"),
        ));
    }
    let cursor_id = page.cursor_id;
    let rows_affected = page.rows_affected;
    let mut rows = Vec::new();
    let mut result_bytes = 0_u64;
    loop {
        if rows.len().saturating_add(page.rows.len()) > max_rows {
            return Err(resource_error(
                "oracle_metadata_too_large",
                format!("Oracle metadata exceeded the {max_rows} row safety limit"),
            ));
        }
        for row in &page.rows {
            result_bytes = result_bytes
                .checked_add(oracle_metadata_row_size(row)?)
                .ok_or_else(|| {
                    resource_error(
                        "oracle_metadata_too_large",
                        "Oracle metadata exceeded its byte safety limit",
                    )
                })?;
            if result_bytes > MAX_METADATA_RESULT_BYTES {
                return Err(resource_error(
                    "oracle_metadata_too_large",
                    format!(
                        "Oracle metadata exceeded the {MAX_METADATA_RESULT_BYTES} byte safety limit"
                    ),
                ));
            }
        }
        rows.append(&mut page.rows);
        if !page.has_more_rows {
            break;
        }
        page = connection
            .fetch_more(cursor_id, &columns, FETCH_ROWS)
            .await
            .map_err(|error| oracle_query_error(&error))?;
    }
    Ok(OracleQueryResult {
        columns,
        rows,
        rows_affected,
        has_more_rows: false,
        cursor_id,
    })
}

fn oracle_metadata_row_size(row: &OracleRow) -> Result<u64, AppError> {
    let mut size = 0_u64;
    for index in 0..row.len() {
        let value = row.get(index).ok_or_else(result_decode_error)?;
        size = size
            .checked_add(oracle_metadata_value_size(value)?)
            .and_then(|size| size.checked_add(8))
            .ok_or_else(|| {
                resource_error(
                    "oracle_metadata_too_large",
                    "Oracle metadata exceeded its byte safety limit",
                )
            })?;
    }
    Ok(size)
}

fn oracle_metadata_value_size(value: &OracleValue) -> Result<u64, AppError> {
    let size = match value {
        OracleValue::Null | OracleValue::Lob(LobValue::Null | LobValue::Empty) => 0,
        OracleValue::String(value) => {
            u64::try_from(value.len()).map_err(|_| AppError::internal())?
        }
        OracleValue::Bytes(value) => {
            u64::try_from(value.len()).map_err(|_| AppError::internal())?
        }
        OracleValue::Integer(_) | OracleValue::Float(_) => 8,
        OracleValue::Number(value) => {
            u64::try_from(value.as_str().len()).map_err(|_| AppError::internal())?
        }
        OracleValue::Date(_) => 7,
        OracleValue::Timestamp(_) => 13,
        OracleValue::RowId(value) => {
            u64::try_from(format!("{value}").len()).map_err(|_| AppError::internal())?
        }
        OracleValue::Boolean(_) => 1,
        OracleValue::Lob(LobValue::Inline(value)) => {
            u64::try_from(value.len()).map_err(|_| AppError::internal())?
        }
        OracleValue::Lob(LobValue::Locator(locator)) => locator.size(),
        OracleValue::Json(value) => {
            u64::try_from(value.to_string().len()).map_err(|_| AppError::internal())?
        }
        OracleValue::Vector(_) | OracleValue::Cursor(_) | OracleValue::Collection(_) => {
            return Err(AppError::invalid(
                "oracle_metadata_type_not_supported",
                "Oracle metadata returned a value type that the native driver does not support",
            ));
        }
    };
    Ok(size)
}

fn validate_metadata_identifier(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(AppError::invalid(
            "invalid_oracle_metadata_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn validate_name_pattern(value: &str) -> Result<(), AppError> {
    if value.len() > MAX_IDENTIFIER_BYTES * 4
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(AppError::invalid(
            "invalid_oracle_metadata_request",
            "namePattern is invalid",
        ));
    }
    Ok(())
}

fn required_text(row: &OracleRow, index: usize) -> Result<String, AppError> {
    optional_text(row, index)?.ok_or_else(result_decode_error)
}

fn optional_text(row: &OracleRow, index: usize) -> Result<Option<String>, AppError> {
    let Some(value) = row.get(index) else {
        return Err(result_decode_error());
    };
    let value = match value {
        OracleValue::Null | OracleValue::Lob(LobValue::Null) => return Ok(None),
        OracleValue::String(value) => value.clone(),
        OracleValue::Bytes(value) => {
            String::from_utf8(value.clone()).map_err(|_| result_decode_error())?
        }
        OracleValue::Integer(value) => value.to_string(),
        OracleValue::Float(value) => value.to_string(),
        OracleValue::Number(value) => value.as_str().to_owned(),
        OracleValue::Date(value) => format_oracle_date(*value),
        OracleValue::Timestamp(value) if value.has_timezone() => {
            format_oracle_timestamp_tz(*value)?
        }
        OracleValue::Timestamp(value) => format_oracle_timestamp(*value),
        OracleValue::RowId(value) => format!("{value}"),
        OracleValue::Boolean(value) => value.to_string(),
        OracleValue::Json(value) => value.to_string(),
        OracleValue::Lob(LobValue::Inline(value)) => {
            String::from_utf8(value.to_vec()).map_err(|_| result_decode_error())?
        }
        OracleValue::Lob(LobValue::Empty) => String::new(),
        OracleValue::Lob(LobValue::Locator(_)) => return Err(result_decode_error()),
        other => oracle_display(other),
    };
    validate_result_scalar(value.len())?;
    Ok(Some(value))
}

fn optional_i32(row: &OracleRow, index: usize) -> Result<Option<i32>, AppError> {
    optional_text(row, index)?
        .map(|value| value.parse::<i32>().map_err(|_| result_decode_error()))
        .transpose()
}

fn optional_bool(row: &OracleRow, index: usize) -> Result<Option<bool>, AppError> {
    optional_text(row, index)?
        .map(|value| match value.to_ascii_uppercase().as_str() {
            "Y" | "YES" | "TRUE" | "1" => Ok(true),
            "N" | "NO" | "FALSE" | "0" => Ok(false),
            _ => Err(result_decode_error()),
        })
        .transpose()
}

pub(crate) async fn list_databases(
    application: &Application,
    request: ListDatabasesRequest,
) -> Result<DatabaseList, AppError> {
    let result = metadata_query(
        application,
        &request.datasource_id,
        "SELECT SYS_CONTEXT('USERENV', 'DB_NAME'), SYS_CONTEXT('USERENV', 'CURRENT_USER') FROM DUAL",
        Vec::new(),
    )
    .await?;
    let row = result.rows.first().ok_or_else(result_decode_error)?;
    Ok(DatabaseList {
        items: vec![DatabaseMetadata {
            name: required_text(row, 0)?,
            owner: required_text(row, 1)?,
            ..DatabaseMetadata::default()
        }],
    })
}

pub(crate) async fn list_schemas(
    application: &Application,
    request: ListSchemasRequest,
) -> Result<SchemaList, AppError> {
    let result = metadata_query(
        application,
        &request.datasource_id,
        "SELECT USERNAME FROM ALL_USERS ORDER BY USERNAME",
        Vec::new(),
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| {
            let name = required_text(row, 0)?;
            Ok(SchemaMetadata {
                database_name: request.database_name.clone(),
                owner: name.clone(),
                system: oracle_system_schema(&name),
                name,
                ..SchemaMetadata::default()
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(SchemaList { items })
}

fn oracle_system_schema(name: &str) -> bool {
    ORACLE_SYSTEM_SCHEMAS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

pub(crate) async fn list_tables(
    application: &Application,
    request: ListTablesRequest,
) -> Result<TableList, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    validate_name_pattern(&request.name_pattern)?;
    let (sql, parameters) = if request.name_pattern.is_empty() {
        (
            "SELECT T.OWNER, T.TABLE_NAME, C.COMMENTS, T.TABLESPACE_NAME, T.NUM_ROWS, T.BLOCKS \
             FROM ALL_TABLES T LEFT JOIN ALL_TAB_COMMENTS C \
               ON C.OWNER = T.OWNER AND C.TABLE_NAME = T.TABLE_NAME AND C.TABLE_TYPE = 'TABLE' \
             WHERE T.OWNER = :1 ORDER BY T.TABLE_NAME",
            vec![OracleValue::String(request.scope.schema_name.clone())],
        )
    } else {
        (
            "SELECT T.OWNER, T.TABLE_NAME, C.COMMENTS, T.TABLESPACE_NAME, T.NUM_ROWS, T.BLOCKS \
             FROM ALL_TABLES T LEFT JOIN ALL_TAB_COMMENTS C \
               ON C.OWNER = T.OWNER AND C.TABLE_NAME = T.TABLE_NAME AND C.TABLE_TYPE = 'TABLE' \
             WHERE T.OWNER = :1 AND T.TABLE_NAME LIKE :2 ORDER BY T.TABLE_NAME",
            vec![
                OracleValue::String(request.scope.schema_name.clone()),
                OracleValue::String(request.name_pattern.clone()),
            ],
        )
    };
    let result = metadata_query(application, &request.scope.datasource_id, sql, parameters).await?;
    let items = result
        .rows
        .iter()
        .map(|row| table_metadata(&request.scope.database_name, row, "TABLE"))
        .collect::<Result<_, _>>()?;
    Ok(TableList { items })
}

fn table_metadata(
    database_name: &str,
    row: &OracleRow,
    table_type: &str,
) -> Result<TableMetadata, AppError> {
    Ok(TableMetadata {
        database_name: database_name.to_owned(),
        schema_name: required_text(row, 0)?,
        name: required_text(row, 1)?,
        table_type: table_type.to_owned(),
        comment: optional_text(row, 2)?.unwrap_or_default(),
        database_type: ORACLE_DATABASE_TYPE.to_owned(),
        tablespace: optional_text(row, 3)?.unwrap_or_default(),
        rows: optional_text(row, 4)?,
        data_length: optional_text(row, 5)?,
        ..TableMetadata::default()
    })
}

pub(crate) async fn list_columns(
    application: &Application,
    request: ListColumnsRequest,
) -> Result<ColumnList, AppError> {
    let scope = &request.table.scope;
    validate_metadata_identifier(&scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.table.table_name, "tableName")?;
    let result = metadata_query(
        application,
        &scope.datasource_id,
        "SELECT C.COLUMN_NAME, C.DATA_TYPE, C.DATA_DEFAULT_VC, CC.COMMENTS, C.NULLABLE, \
                C.COLUMN_ID, C.DATA_LENGTH, C.DATA_PRECISION, C.DATA_SCALE, C.CHAR_LENGTH, \
                C.CHAR_USED, PK.CONSTRAINT_NAME, PK.POSITION, C.IDENTITY_COLUMN, C.VIRTUAL_COLUMN \
           FROM ALL_TAB_COLS C \
           LEFT JOIN ALL_COL_COMMENTS CC ON CC.OWNER = C.OWNER \
                AND CC.TABLE_NAME = C.TABLE_NAME AND CC.COLUMN_NAME = C.COLUMN_NAME \
           LEFT JOIN (SELECT AC.OWNER, ACC.TABLE_NAME, ACC.COLUMN_NAME, AC.CONSTRAINT_NAME, ACC.POSITION \
                        FROM ALL_CONSTRAINTS AC JOIN ALL_CONS_COLUMNS ACC \
                          ON ACC.OWNER = AC.OWNER AND ACC.CONSTRAINT_NAME = AC.CONSTRAINT_NAME \
                       WHERE AC.CONSTRAINT_TYPE = 'P') PK \
             ON PK.OWNER = C.OWNER AND PK.TABLE_NAME = C.TABLE_NAME AND PK.COLUMN_NAME = C.COLUMN_NAME \
          WHERE C.OWNER = :1 AND C.TABLE_NAME = :2 AND C.HIDDEN_COLUMN = 'NO' \
          ORDER BY C.COLUMN_ID",
        vec![
            OracleValue::String(scope.schema_name.clone()),
            OracleValue::String(request.table.table_name.clone()),
        ],
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| column_metadata(&request, row))
        .collect::<Result<_, _>>()?;
    Ok(ColumnList { items })
}

fn column_metadata(
    request: &ListColumnsRequest,
    row: &OracleRow,
) -> Result<ColumnMetadata, AppError> {
    let column_type = required_text(row, 1)?;
    let primary_key_name = optional_text(row, 11)?.unwrap_or_default();
    let char_used = optional_text(row, 10)?.unwrap_or_default();
    Ok(ColumnMetadata {
        database_name: request.table.scope.database_name.clone(),
        schema_name: request.table.scope.schema_name.clone(),
        table_name: request.table.table_name.clone(),
        name: required_text(row, 0)?,
        data_type: Some(oracle_metadata_jdbc_type(&column_type)),
        column_type,
        default_value: optional_text(row, 2)?,
        auto_increment: optional_bool(row, 13)?,
        comment: optional_text(row, 3)?.unwrap_or_default(),
        primary_key: Some(!primary_key_name.is_empty()),
        primary_key_name,
        primary_key_order: optional_i32(row, 12)?.unwrap_or_default(),
        column_size: optional_i32(row, 7)?.or(optional_i32(row, 9)?),
        buffer_length: optional_i32(row, 6)?,
        decimal_digits: optional_i32(row, 8)?,
        char_octet_length: optional_i32(row, 6)?,
        ordinal_position: optional_i32(row, 5)?,
        nullable: optional_text(row, 4)?.map(|value| i32::from(value.eq_ignore_ascii_case("Y"))),
        generated_column: optional_bool(row, 14)?,
        unit: match char_used.as_str() {
            "C" => "CHAR".to_owned(),
            "B" => "BYTE".to_owned(),
            _ => String::new(),
        },
        ..ColumnMetadata::default()
    })
}

fn oracle_metadata_jdbc_type(data_type: &str) -> i32 {
    match data_type.to_ascii_uppercase().as_str() {
        "VARCHAR2" | "VARCHAR" => 12,
        "NVARCHAR2" => -9,
        "CHAR" => 1,
        "NCHAR" => -15,
        "NUMBER" | "DECIMAL" | "NUMERIC" => 2,
        "FLOAT" | "BINARY_FLOAT" => 6,
        "BINARY_DOUBLE" => 8,
        "DATE" | "TIMESTAMP" | "TIMESTAMP WITH LOCAL TIME ZONE" => 93,
        "TIMESTAMP WITH TIME ZONE" => 2_014,
        "RAW" => -3,
        "LONG RAW" => -4,
        "LONG" => -1,
        "CLOB" | "NCLOB" => 2_005,
        "BLOB" => 2_004,
        "BFILE" => -13,
        "ROWID" | "UROWID" => -8,
        "BOOLEAN" => 16,
        _ => 1_111,
    }
}

pub(crate) async fn list_views(
    application: &Application,
    request: ListViewsRequest,
) -> Result<ViewList, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    validate_name_pattern(&request.name_pattern)?;
    let (sql, parameters) = if request.name_pattern.is_empty() {
        (
            "SELECT V.OWNER, V.VIEW_NAME, C.COMMENTS, CAST(NULL AS VARCHAR2(128)), \
                    CAST(NULL AS NUMBER), CAST(NULL AS NUMBER) \
               FROM ALL_VIEWS V LEFT JOIN ALL_TAB_COMMENTS C \
                 ON C.OWNER = V.OWNER AND C.TABLE_NAME = V.VIEW_NAME AND C.TABLE_TYPE = 'VIEW' \
              WHERE V.OWNER = :1 ORDER BY V.VIEW_NAME",
            vec![OracleValue::String(request.scope.schema_name.clone())],
        )
    } else {
        (
            "SELECT V.OWNER, V.VIEW_NAME, C.COMMENTS, CAST(NULL AS VARCHAR2(128)), \
                    CAST(NULL AS NUMBER), CAST(NULL AS NUMBER) \
               FROM ALL_VIEWS V LEFT JOIN ALL_TAB_COMMENTS C \
                 ON C.OWNER = V.OWNER AND C.TABLE_NAME = V.VIEW_NAME AND C.TABLE_TYPE = 'VIEW' \
              WHERE V.OWNER = :1 AND V.VIEW_NAME LIKE :2 ORDER BY V.VIEW_NAME",
            vec![
                OracleValue::String(request.scope.schema_name.clone()),
                OracleValue::String(request.name_pattern.clone()),
            ],
        )
    };
    let result = metadata_query(application, &request.scope.datasource_id, sql, parameters).await?;
    let items = result
        .rows
        .iter()
        .map(|row| table_metadata(&request.scope.database_name, row, "VIEW"))
        .collect::<Result<_, _>>()?;
    Ok(ViewList { items })
}

pub(crate) async fn get_view(
    application: &Application,
    request: MetadataObjectRef,
) -> Result<TableMetadata, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.object_name, "viewName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT V.OWNER, V.VIEW_NAME, C.COMMENTS, CAST(NULL AS VARCHAR2(128)), \
                CAST(NULL AS NUMBER), CAST(NULL AS NUMBER) \
           FROM ALL_VIEWS V LEFT JOIN ALL_TAB_COMMENTS C \
             ON C.OWNER = V.OWNER AND C.TABLE_NAME = V.VIEW_NAME AND C.TABLE_TYPE = 'VIEW' \
          WHERE V.OWNER = :1 AND V.VIEW_NAME = :2",
        vec![
            OracleValue::String(request.scope.schema_name.clone()),
            OracleValue::String(request.object_name.clone()),
        ],
    )
    .await?;
    let mut metadata = table_metadata(
        &request.scope.database_name,
        result
            .rows
            .first()
            .ok_or_else(|| metadata_not_found("view", &request))?,
        "VIEW",
    )?;
    metadata.ddl = object_ddl(
        application,
        &request.scope.datasource_id,
        &request.scope.schema_name,
        &request.object_name,
        "VIEW",
    )
    .await?;
    Ok(metadata)
}

pub(crate) async fn list_indexes(
    application: &Application,
    request: ListIndexesRequest,
) -> Result<IndexList, AppError> {
    validate_metadata_identifier(&request.table.scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.table.table_name, "tableName")?;
    let result = metadata_query(
        application,
        &request.table.scope.datasource_id,
        "SELECT I.OWNER, I.TABLE_NAME, I.INDEX_NAME, I.INDEX_TYPE, I.UNIQUENESS, \
                I.TABLESPACE_NAME, I.STATUS, C.COLUMN_POSITION, C.COLUMN_NAME, C.DESCEND \
           FROM ALL_INDEXES I JOIN ALL_IND_COLUMNS C \
             ON C.INDEX_OWNER = I.OWNER AND C.INDEX_NAME = I.INDEX_NAME \
          WHERE I.TABLE_OWNER = :1 AND I.TABLE_NAME = :2 \
          ORDER BY I.INDEX_NAME, C.COLUMN_POSITION",
        vec![
            OracleValue::String(request.table.scope.schema_name.clone()),
            OracleValue::String(request.table.table_name.clone()),
        ],
    )
    .await?;
    let mut indexes = BTreeMap::<String, IndexMetadata>::new();
    for row in &result.rows {
        let name = required_text(row, 2)?;
        let unique = required_text(row, 4)?.eq_ignore_ascii_case("UNIQUE");
        let column = IndexColumnMetadata {
            database_name: request.table.scope.database_name.clone(),
            schema_name: request.table.scope.schema_name.clone(),
            table_name: request.table.table_name.clone(),
            index_name: name.clone(),
            column_name: optional_text(row, 8)?.unwrap_or_default(),
            ordinal_position: optional_i32(row, 7)?,
            non_unique: Some(!unique),
            index_qualifier: required_text(row, 0)?,
            sort_order: optional_text(row, 9)?.unwrap_or_default(),
            ..IndexColumnMetadata::default()
        };
        indexes
            .entry(name.clone())
            .or_insert_with(|| IndexMetadata {
                database_name: request.table.scope.database_name.clone(),
                schema_name: request.table.scope.schema_name.clone(),
                table_name: request.table.table_name.clone(),
                name,
                index_type: if unique { "Unique" } else { "Normal" }.to_owned(),
                unique: Some(unique),
                method: optional_text(row, 3).ok().flatten().unwrap_or_default(),
                comment: optional_text(row, 6).ok().flatten().unwrap_or_default(),
                ..IndexMetadata::default()
            })
            .columns
            .push(column);
    }
    Ok(IndexList {
        items: indexes.into_values().collect(),
    })
}

async fn object_ddl(
    application: &Application,
    datasource_id: &str,
    schema_name: &str,
    object_name: &str,
    object_type: &str,
) -> Result<String, AppError> {
    validate_metadata_identifier(schema_name, "schemaName")?;
    validate_metadata_identifier(object_name, "objectName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let managed = open_resolved_connection(&resolved).await?;
    let ddl = tokio::time::timeout(OPERATION_TIMEOUT, async {
        let result = query_all(
            &managed.connection,
            "SELECT DBMS_METADATA.GET_DDL(:1, :2, :3) FROM DUAL",
            &[
                OracleValue::String(object_type.to_owned()),
                OracleValue::String(object_name.to_owned()),
                OracleValue::String(schema_name.to_owned()),
            ],
            2,
        )
        .await?;
        let row = result.rows.first().ok_or_else(|| {
            AppError::not_found("oracle_metadata_not_found", "Oracle object does not exist")
        })?;
        let value = row.get(0).cloned().ok_or_else(result_decode_error)?;
        let oracle_type = result
            .columns
            .first()
            .map_or(OracleType::Clob, |column| column.oracle_type);
        oracle_text(&managed.connection, value, oracle_type).await
    })
    .await;
    match ddl {
        Ok(Ok(ddl)) => {
            if let Err(error) = managed.close().await {
                tracing::warn!(error = %error, "Oracle DDL connection cleanup failed");
            }
            Ok(ddl)
        }
        Ok(Err(error)) => {
            managed.abandon().await;
            Err(error)
        }
        Err(_) => {
            managed.abandon().await;
            Err(oracle_operation_timeout("DDL lookup and LOB read"))
        }
    }
}

fn metadata_not_found(kind: &str, request: &MetadataObjectRef) -> AppError {
    AppError::not_found(
        "oracle_metadata_not_found",
        format!(
            "Oracle {kind} {}.{} does not exist",
            request.scope.schema_name, request.object_name
        ),
    )
}

pub(crate) async fn list_imported_keys(
    application: &Application,
    request: ListTableKeysRequest,
) -> Result<ForeignKeyList, AppError> {
    list_foreign_keys(application, &request, false).await
}

pub(crate) async fn list_exported_keys(
    application: &Application,
    request: ListTableKeysRequest,
) -> Result<ForeignKeyList, AppError> {
    list_foreign_keys(application, &request, true).await
}

async fn list_foreign_keys(
    application: &Application,
    request: &ListTableKeysRequest,
    exported: bool,
) -> Result<ForeignKeyList, AppError> {
    let scope = &request.table.scope;
    validate_metadata_identifier(&scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.table.table_name, "tableName")?;
    let filter = if exported {
        "PC.OWNER = :1 AND PC.TABLE_NAME = :2"
    } else {
        "FC.OWNER = :1 AND FC.TABLE_NAME = :2"
    };
    let sql = format!(
        "SELECT PC.OWNER, PC.TABLE_NAME, PCC.COLUMN_NAME, \
                FC.OWNER, FC.TABLE_NAME, FCC.COLUMN_NAME, FCC.POSITION, \
                FC.DELETE_RULE, FC.CONSTRAINT_NAME, PC.CONSTRAINT_NAME, \
                FC.DEFERRABLE, FC.DEFERRED \
           FROM ALL_CONSTRAINTS FC \
           JOIN ALL_CONS_COLUMNS FCC ON FCC.OWNER = FC.OWNER \
                AND FCC.CONSTRAINT_NAME = FC.CONSTRAINT_NAME \
           JOIN ALL_CONSTRAINTS PC ON PC.OWNER = FC.R_OWNER \
                AND PC.CONSTRAINT_NAME = FC.R_CONSTRAINT_NAME \
           JOIN ALL_CONS_COLUMNS PCC ON PCC.OWNER = PC.OWNER \
                AND PCC.CONSTRAINT_NAME = PC.CONSTRAINT_NAME \
                AND PCC.POSITION = FCC.POSITION \
          WHERE FC.CONSTRAINT_TYPE = 'R' AND {filter} \
          ORDER BY FC.CONSTRAINT_NAME, FCC.POSITION"
    );
    let result = metadata_query(
        application,
        &scope.datasource_id,
        &sql,
        vec![
            OracleValue::String(scope.schema_name.clone()),
            OracleValue::String(request.table.table_name.clone()),
        ],
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| foreign_key_metadata(&scope.database_name, row))
        .collect::<Result<_, _>>()?;
    Ok(ForeignKeyList { items })
}

fn foreign_key_metadata(
    database_name: &str,
    row: &OracleRow,
) -> Result<ForeignKeyMetadata, AppError> {
    let delete_rule = match required_text(row, 7)?.to_ascii_uppercase().as_str() {
        "CASCADE" => 0,
        "RESTRICT" => 1,
        "SET NULL" => 2,
        "SET DEFAULT" => 4,
        _ => 3,
    };
    let deferrability = if required_text(row, 10)?.eq_ignore_ascii_case("NOT DEFERRABLE") {
        7
    } else if required_text(row, 11)?.eq_ignore_ascii_case("DEFERRED") {
        5
    } else {
        6
    };
    Ok(ForeignKeyMetadata {
        primary_table_database: database_name.to_owned(),
        primary_table_schema: required_text(row, 0)?,
        primary_table_name: required_text(row, 1)?,
        primary_column_name: required_text(row, 2)?,
        foreign_table_database: database_name.to_owned(),
        foreign_table_schema: required_text(row, 3)?,
        foreign_table_name: required_text(row, 4)?,
        foreign_column_name: required_text(row, 5)?,
        key_sequence: optional_i32(row, 6)?.unwrap_or_default(),
        update_rule: 3,
        delete_rule,
        foreign_key_name: required_text(row, 8)?,
        primary_key_name: required_text(row, 9)?,
        deferrability,
    })
}

pub(crate) async fn list_primary_keys(
    application: &Application,
    request: ListTableKeysRequest,
) -> Result<PrimaryKeyList, AppError> {
    let scope = &request.table.scope;
    validate_metadata_identifier(&scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.table.table_name, "tableName")?;
    let result = metadata_query(
        application,
        &scope.datasource_id,
        "SELECT AC.OWNER, ACC.TABLE_NAME, ACC.COLUMN_NAME, AC.CONSTRAINT_NAME \
           FROM ALL_CONSTRAINTS AC JOIN ALL_CONS_COLUMNS ACC \
             ON ACC.OWNER = AC.OWNER AND ACC.CONSTRAINT_NAME = AC.CONSTRAINT_NAME \
          WHERE AC.CONSTRAINT_TYPE = 'P' AND AC.OWNER = :1 AND ACC.TABLE_NAME = :2 \
          ORDER BY ACC.POSITION",
        vec![
            OracleValue::String(scope.schema_name.clone()),
            OracleValue::String(request.table.table_name.clone()),
        ],
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| {
            Ok(PrimaryKeyMetadata {
                database_name: scope.database_name.clone(),
                schema_name: required_text(row, 0)?,
                table_name: required_text(row, 1)?,
                column_name: required_text(row, 2)?,
                name: required_text(row, 3)?,
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(PrimaryKeyList { items })
}

pub(crate) async fn list_functions(
    application: &Application,
    request: ListRoutinesRequest,
) -> Result<FunctionList, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT OWNER, OBJECT_NAME FROM ALL_OBJECTS \
          WHERE OWNER = :1 AND OBJECT_TYPE = 'FUNCTION' ORDER BY OBJECT_NAME",
        vec![OracleValue::String(request.scope.schema_name.clone())],
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| {
            let name = required_text(row, 1)?;
            Ok(FunctionMetadata {
                database_name: request.scope.database_name.clone(),
                schema_name: required_text(row, 0)?,
                name: name.clone(),
                function_type: Some(1),
                specific_name: name,
                ..FunctionMetadata::default()
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(FunctionList { items })
}

pub(crate) async fn get_function(
    application: &Application,
    request: MetadataObjectRef,
) -> Result<FunctionMetadata, AppError> {
    let body = routine_source(application, &request, "FUNCTION").await?;
    Ok(FunctionMetadata {
        database_name: request.scope.database_name,
        schema_name: request.scope.schema_name,
        name: request.object_name.clone(),
        function_type: Some(1),
        specific_name: request.object_name,
        body,
        ..FunctionMetadata::default()
    })
}

pub(crate) async fn list_function_parameters(
    application: &Application,
    request: MetadataObjectRef,
) -> Result<FunctionParameterList, AppError> {
    let rows = routine_parameters(application, &request).await?;
    let items = rows
        .iter()
        .map(|row| function_parameter_metadata(&request, row))
        .collect::<Result<_, _>>()?;
    Ok(FunctionParameterList { items })
}

fn function_parameter_metadata(
    request: &MetadataObjectRef,
    row: &OracleRow,
) -> Result<FunctionParameterMetadata, AppError> {
    let position = optional_i32(row, 1)?.unwrap_or_default();
    let mode = optional_text(row, 2)?.unwrap_or_default();
    Ok(FunctionParameterMetadata {
        function_database: request.scope.database_name.clone(),
        function_schema: request.scope.schema_name.clone(),
        function_name: request.object_name.clone(),
        column_name: optional_text(row, 0)?.unwrap_or_default(),
        column_type: Some(if position == 0 {
            4
        } else {
            routine_parameter_mode(&mode, false)
        }),
        data_type: Some(oracle_metadata_jdbc_type(&required_text(row, 3)?)),
        type_name: required_text(row, 3)?,
        length: optional_i32(row, 4)?,
        precision: optional_i32(row, 5)?,
        scale: optional_i32(row, 6)?,
        radix: optional_i32(row, 7)?,
        nullable: Some(2),
        ordinal_position: Some(position),
        is_nullable: String::new(),
        specific_name: request.object_name.clone(),
        ..FunctionParameterMetadata::default()
    })
}

pub(crate) async fn list_procedures(
    application: &Application,
    request: ListRoutinesRequest,
) -> Result<ProcedureList, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT OWNER, OBJECT_NAME FROM ALL_OBJECTS \
          WHERE OWNER = :1 AND OBJECT_TYPE = 'PROCEDURE' ORDER BY OBJECT_NAME",
        vec![OracleValue::String(request.scope.schema_name.clone())],
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| {
            let name = required_text(row, 1)?;
            Ok(ProcedureMetadata {
                database_name: request.scope.database_name.clone(),
                schema_name: required_text(row, 0)?,
                name: name.clone(),
                procedure_type: Some(1),
                specific_name: name,
                ..ProcedureMetadata::default()
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(ProcedureList { items })
}

pub(crate) async fn get_procedure(
    application: &Application,
    request: MetadataObjectRef,
) -> Result<ProcedureMetadata, AppError> {
    let body = routine_source(application, &request, "PROCEDURE").await?;
    Ok(ProcedureMetadata {
        database_name: request.scope.database_name,
        schema_name: request.scope.schema_name,
        name: request.object_name.clone(),
        procedure_type: Some(1),
        specific_name: request.object_name,
        body,
        ..ProcedureMetadata::default()
    })
}

pub(crate) async fn list_procedure_parameters(
    application: &Application,
    request: MetadataObjectRef,
) -> Result<ProcedureParameterList, AppError> {
    let rows = routine_parameters(application, &request).await?;
    let items = rows
        .iter()
        .filter(|row| optional_i32(row, 1).ok().flatten().unwrap_or_default() > 0)
        .map(|row| procedure_parameter_metadata(&request, row))
        .collect::<Result<_, _>>()?;
    Ok(ProcedureParameterList { items })
}

fn procedure_parameter_metadata(
    request: &MetadataObjectRef,
    row: &OracleRow,
) -> Result<ProcedureParameterMetadata, AppError> {
    let mode = optional_text(row, 2)?.unwrap_or_default();
    Ok(ProcedureParameterMetadata {
        procedure_database: request.scope.database_name.clone(),
        procedure_schema: request.scope.schema_name.clone(),
        procedure_name: request.object_name.clone(),
        column_name: optional_text(row, 0)?.unwrap_or_default(),
        column_type: Some(routine_parameter_mode(&mode, true)),
        data_type: Some(oracle_metadata_jdbc_type(&required_text(row, 3)?)),
        type_name: required_text(row, 3)?,
        length: optional_i32(row, 4)?,
        precision: optional_i32(row, 5)?,
        scale: optional_i32(row, 6)?,
        radix: optional_i32(row, 7)?,
        nullable: Some(2),
        ordinal_position: optional_i32(row, 1)?,
        is_nullable: String::new(),
        specific_name: request.object_name.clone(),
        ..ProcedureParameterMetadata::default()
    })
}

const fn routine_parameter_mode(mode: &str, procedure: bool) -> i32 {
    match (mode.as_bytes(), procedure) {
        (b"IN", _) => 1,
        (b"IN/OUT" | b"IN OUT", _) => 2,
        (b"OUT", true) => 4,
        (b"OUT", false) => 3,
        _ => 0,
    }
}

async fn routine_parameters(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<Vec<OracleRow>, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.object_name, "routineName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT ARGUMENT_NAME, POSITION, IN_OUT, DATA_TYPE, DATA_LENGTH, \
                DATA_PRECISION, DATA_SCALE, RADIX, DEFAULTED \
           FROM ALL_ARGUMENTS WHERE OWNER = :1 AND OBJECT_NAME = :2 \
                AND PACKAGE_NAME IS NULL ORDER BY SEQUENCE",
        vec![
            OracleValue::String(request.scope.schema_name.clone()),
            OracleValue::String(request.object_name.clone()),
        ],
    )
    .await?;
    Ok(result.rows)
}

async fn routine_source(
    application: &Application,
    request: &MetadataObjectRef,
    routine_type: &str,
) -> Result<String, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.object_name, "routineName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT TEXT FROM ALL_SOURCE WHERE OWNER = :1 AND NAME = :2 AND TYPE = :3 ORDER BY LINE",
        vec![
            OracleValue::String(request.scope.schema_name.clone()),
            OracleValue::String(request.object_name.clone()),
            OracleValue::String(routine_type.to_owned()),
        ],
    )
    .await?;
    if result.rows.is_empty() {
        return Err(metadata_not_found(
            &routine_type.to_ascii_lowercase(),
            request,
        ));
    }
    let mut body = String::new();
    for row in &result.rows {
        body.push_str(&required_text(row, 0)?);
        if body.len() > MAX_SQL_BYTES {
            return Err(resource_error(
                "oracle_routine_source_too_large",
                format!("Oracle routine source exceeds {MAX_SQL_BYTES} bytes"),
            ));
        }
    }
    Ok(body)
}

pub(crate) async fn list_triggers(
    application: &Application,
    request: ListTriggersRequest,
) -> Result<TriggerList, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT OWNER, TRIGGER_NAME, TRIGGERING_EVENT FROM ALL_TRIGGERS \
          WHERE OWNER = :1 ORDER BY TRIGGER_NAME",
        vec![OracleValue::String(request.scope.schema_name.clone())],
    )
    .await?;
    let items = result
        .rows
        .iter()
        .map(|row| {
            Ok(TriggerMetadata {
                database_name: request.scope.database_name.clone(),
                schema_name: required_text(row, 0)?,
                name: required_text(row, 1)?,
                event_manipulation: required_text(row, 2)?,
                body: String::new(),
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(TriggerList { items })
}

pub(crate) async fn get_trigger(
    application: &Application,
    request: MetadataObjectRef,
) -> Result<TriggerMetadata, AppError> {
    validate_metadata_identifier(&request.scope.schema_name, "schemaName")?;
    validate_metadata_identifier(&request.object_name, "triggerName")?;
    let result = metadata_query(
        application,
        &request.scope.datasource_id,
        "SELECT OWNER, TRIGGER_NAME, TRIGGERING_EVENT FROM ALL_TRIGGERS \
          WHERE OWNER = :1 AND TRIGGER_NAME = :2",
        vec![
            OracleValue::String(request.scope.schema_name.clone()),
            OracleValue::String(request.object_name.clone()),
        ],
    )
    .await?;
    let row = result
        .rows
        .first()
        .ok_or_else(|| metadata_not_found("trigger", &request))?;
    Ok(TriggerMetadata {
        database_name: request.scope.database_name.clone(),
        schema_name: required_text(row, 0)?,
        name: required_text(row, 1)?,
        event_manipulation: required_text(row, 2)?,
        body: object_ddl(
            application,
            &request.scope.datasource_id,
            &request.scope.schema_name,
            &request.object_name,
            "TRIGGER",
        )
        .await?,
    })
}

pub(crate) async fn load_er_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<Vec<EntityRelationTable>, AppError> {
    let tables = list_tables(
        application,
        ListTablesRequest {
            scope: MetadataScope {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
            },
            name_pattern: String::new(),
        },
    )
    .await?;
    let mut result = Vec::with_capacity(tables.items.len());
    for table in tables.items {
        let table_ref = TableRef {
            scope: MetadataScope {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
            },
            table_name: table.name.clone(),
        };
        let columns = list_columns(
            application,
            ListColumnsRequest {
                table: table_ref.clone(),
            },
        )
        .await?;
        let foreign_keys =
            list_imported_keys(application, ListTableKeysRequest { table: table_ref }).await?;
        result.push(EntityRelationTable {
            name: table.name,
            comment: table.comment,
            columns: columns
                .items
                .into_iter()
                .map(|column| EntityRelationColumn {
                    name: column.name,
                    column_type: column.column_type,
                    primary_key: column.primary_key.unwrap_or(false),
                    comment: column.comment,
                })
                .collect(),
            foreign_keys: foreign_keys
                .items
                .into_iter()
                .map(|key| EntityRelationForeignKey {
                    primary_table: key.primary_table_name,
                    primary_column: key.primary_column_name,
                    foreign_table: key.foreign_table_name,
                    foreign_column: key.foreign_column_name,
                })
                .collect(),
        });
    }
    Ok(result)
}

pub(crate) async fn start_table_preview(
    application: &Application,
    request: TablePreviewRequest,
    row_limit: u32,
) -> Result<TablePreviewAccepted, AppError> {
    if row_limit == 0 || row_limit > MAX_TABLE_PREVIEW_ROWS {
        return Err(AppError::invalid(
            "invalid_table_preview_request",
            format!("rowLimit must be between 1 and {MAX_TABLE_PREVIEW_ROWS}"),
        ));
    }
    let schema_name = quote_identifier(&request.table.scope.schema_name, "schemaName")?;
    let table_name = quote_identifier(&request.table.table_name, "tableName")?;
    let sql = format!("SELECT * FROM {schema_name}.{table_name} FETCH FIRST {row_limit} ROWS ONLY");
    let accepted = application
        .start_read_query(StartQueryRequest {
            datasource_id: request.table.scope.datasource_id,
            sql: sql.clone(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: row_limit.to_string(),
                max_result_bytes: (8 * 1024 * 1024_u64).to_string(),
                batch_rows: row_limit.min(200),
                batch_bytes: 1024 * 1024,
                result_ttl_seconds: 60 * 60,
            },
        })
        .await?;
    Ok(TablePreviewAccepted {
        operation_id: accepted.operation_id,
        sql,
        row_limit,
    })
}

fn build_oracle_create_schema(request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
    let schema = request.schema;
    if !schema.owner.trim().is_empty() && !schema.owner.eq_ignore_ascii_case(&schema.name) {
        return Err(AppError::invalid(
            "oracle_schema_owner_mismatch",
            "An Oracle schema name must match its authorization user",
        ));
    }
    let name = quote_identifier(&schema.name, "schemaName")?;
    Ok(BuiltSql {
        sql: format!("CREATE SCHEMA AUTHORIZATION {name};"),
    })
}

fn build_oracle_namespace_sql(request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
    let sql = match request.operation {
        NamespaceSqlOperation::CreateDatabase { database } => {
            build_oracle_create_database(&database)?
        }
        NamespaceSqlOperation::AlterDatabase { .. } => {
            return Err(oracle_namespace_unsupported(
                "oracle_database_alter_unsupported",
                "Oracle does not support renaming a database through a connected SQL session",
            ));
        }
        NamespaceSqlOperation::DropDatabase { database_name } => format!(
            "DROP DATABASE {};",
            quote_identifier(&database_name, "databaseName")?
        ),
        NamespaceSqlOperation::UseDatabase { .. } => {
            return Err(oracle_namespace_unsupported(
                "oracle_database_switch_unsupported",
                "Oracle selects a database service when opening the connection and cannot switch it with SQL",
            ));
        }
        NamespaceSqlOperation::CreateSchema { schema } => {
            return build_oracle_create_schema(CreateSchemaSqlRequest { schema });
        }
        NamespaceSqlOperation::AlterSchema { .. } => {
            return Err(oracle_namespace_unsupported(
                "oracle_schema_rename_unsupported",
                "Oracle schemas are database users and cannot be renamed",
            ));
        }
        NamespaceSqlOperation::DropSchema { schema_name } => format!(
            "DROP USER {} CASCADE;",
            quote_identifier(&schema_name, "schemaName")?
        ),
    };
    Ok(BuiltSql { sql })
}

fn build_oracle_create_database(database: &DatabaseDefinition) -> Result<String, AppError> {
    let mut sql = format!(
        "CREATE DATABASE {}",
        quote_identifier(&database.name, "databaseName")?
    );
    if !database.charset.trim().is_empty() {
        validate_oracle_keyword(&database.charset, "charset")?;
        write!(&mut sql, " CHARACTER SET {}", database.charset)
            .map_err(|_| AppError::internal())?;
    }
    sql.push(';');
    Ok(sql)
}

fn validate_oracle_keyword(value: &str, field: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'#'))
    {
        return Err(AppError::invalid(
            "invalid_oracle_namespace_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn oracle_namespace_unsupported(code: &'static str, message: &'static str) -> AppError {
    AppError::invalid(code, message)
}

fn build_oracle_dml(request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
    let target = oracle_dml_target(&request.target)?;
    let sql = match request.statement {
        DmlStatement::SingleInsert { columns, row } => {
            oracle_insert_sql(&target, &columns, std::slice::from_ref(&row))?
        }
        DmlStatement::MultiInsert { columns, rows } => oracle_insert_sql(&target, &columns, &rows)?,
        DmlStatement::Update {
            assignments,
            predicates,
        } => oracle_update_sql(&target, &assignments, &predicates)?,
    };
    if sql.len() > MAX_SQL_BYTES {
        return Err(invalid_oracle_dml(
            "The generated Oracle DML exceeds the SQL byte limit",
        ));
    }
    Ok(BuiltSql { sql })
}

fn oracle_dml_target(target: &DmlTarget) -> Result<String, AppError> {
    let table = quote_identifier(&target.table_name, "tableName")?;
    let qualifier = target
        .schema_name
        .as_deref()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            target
                .database_name
                .as_deref()
                .filter(|value| !value.is_empty())
        });
    qualifier.map_or(Ok(table.clone()), |schema| {
        Ok(format!(
            "{}.{}",
            quote_identifier(schema, "schemaName")?,
            table
        ))
    })
}

fn oracle_insert_sql(
    target: &str,
    columns: &[DmlColumn],
    rows: &[DmlRow],
) -> Result<String, AppError> {
    if columns.is_empty() || rows.is_empty() {
        return Err(invalid_oracle_dml(
            "Oracle INSERT requires at least one column and row",
        ));
    }
    let column_sql = columns
        .iter()
        .map(|column| quote_identifier(&column.name, "columnName"))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let values = rows
        .iter()
        .map(|row| oracle_dml_row(row, columns))
        .collect::<Result<Vec<_>, _>>()?;
    if let [values] = values.as_slice() {
        return Ok(format!(
            "INSERT INTO {target} ({column_sql}) VALUES ({values});"
        ));
    }
    let into_clauses = values
        .iter()
        .map(|values| format!("  INTO {target} ({column_sql}) VALUES ({values})"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("INSERT ALL\n{into_clauses}\nSELECT 1 FROM DUAL;"))
}

fn oracle_dml_row(row: &DmlRow, columns: &[DmlColumn]) -> Result<String, AppError> {
    if row.values.len() != columns.len() {
        return Err(invalid_oracle_dml(
            "Each Oracle INSERT row must match the selected column count",
        ));
    }
    row.values
        .iter()
        .zip(columns)
        .map(|(value, column)| oracle_dml_value(value, column))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| values.join(", "))
}

fn oracle_update_sql(
    target: &str,
    assignments: &[DmlAssignment],
    predicates: &[DmlAssignment],
) -> Result<String, AppError> {
    if assignments.is_empty() || predicates.is_empty() {
        return Err(invalid_oracle_dml(
            "Oracle UPDATE requires assignments and key predicates",
        ));
    }
    let assignments = assignments
        .iter()
        .map(|assignment| oracle_dml_assignment(assignment, false))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let predicates = predicates
        .iter()
        .map(|predicate| oracle_dml_assignment(predicate, true))
        .collect::<Result<Vec<_>, _>>()?
        .join(" AND ");
    Ok(format!(
        "UPDATE {target} SET {assignments} WHERE {predicates};"
    ))
}

fn oracle_dml_assignment(assignment: &DmlAssignment, predicate: bool) -> Result<String, AppError> {
    let column = quote_identifier(&assignment.column.name, "columnName")?;
    if predicate && matches!(assignment.value, DmlValue::Null) {
        return Ok(format!("{column} IS NULL"));
    }
    Ok(format!(
        "{column} = {}",
        oracle_dml_value(&assignment.value, &assignment.column)?
    ))
}

fn oracle_dml_value(value: &DmlValue, column: &DmlColumn) -> Result<String, AppError> {
    match value {
        DmlValue::Null => Ok("NULL".to_owned()),
        DmlValue::String(value) => quote_oracle_literal(value),
        DmlValue::Decimal(value) => {
            validate_oracle_decimal(value)?;
            Ok(value.clone())
        }
        DmlValue::Boolean(value) => {
            if column.data_type_name.eq_ignore_ascii_case("BOOLEAN") {
                Ok(if *value { "TRUE" } else { "FALSE" }.to_owned())
            } else {
                Ok(if *value { "1" } else { "0" }.to_owned())
            }
        }
        DmlValue::Temporal { kind, iso8601 } => oracle_dml_temporal(*kind, iso8601),
        DmlValue::Binary(value) => oracle_dml_binary(value, column),
    }
}

fn validate_oracle_decimal(value: &str) -> Result<(), AppError> {
    if value.len() > MAX_SCALAR_BYTES || oracle_rs::types::encode_oracle_number(value).is_err() {
        return Err(invalid_oracle_dml("The Oracle decimal value is invalid"));
    }
    Ok(())
}

fn quote_oracle_literal(value: &str) -> Result<String, AppError> {
    if value.len() > MAX_SCALAR_BYTES || value.contains('\0') {
        return Err(invalid_oracle_dml("The Oracle string value is invalid"));
    }
    let escaped_length = value
        .len()
        .checked_add(value.bytes().filter(|byte| *byte == b'\'').count())
        .and_then(|length| length.checked_add(2))
        .filter(|length| *length <= MAX_SCALAR_BYTES)
        .ok_or_else(|| invalid_oracle_dml("The escaped Oracle string value is too large"))?;
    let mut escaped = String::with_capacity(escaped_length);
    for character in value.chars() {
        if character == '\'' {
            escaped.push('\'');
        }
        escaped.push(character);
    }
    Ok(format!("'{escaped}'"))
}

fn oracle_dml_temporal(kind: DmlTemporalKind, value: &str) -> Result<String, AppError> {
    match kind {
        DmlTemporalKind::Date => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(|value| format!("DATE '{}'", value.format("%Y-%m-%d")))
            .map_err(|_| invalid_oracle_dml("The Oracle date value is invalid")),
        DmlTemporalKind::Time => parse_oracle_dml_time(value).map(|value| {
            let format = if value.nanosecond() == 0 {
                "YYYY-MM-DD HH24:MI:SS"
            } else {
                "YYYY-MM-DD HH24:MI:SS.FF"
            };
            format!(
                "TO_TIMESTAMP('1970-01-01 {}', '{format}')",
                value.format("%H:%M:%S%.f")
            )
        }),
        DmlTemporalKind::LocalDatetime => parse_oracle_dml_timestamp(value)
            .map(|value| format!("TIMESTAMP '{}'", value.format("%Y-%m-%d %H:%M:%S%.f"))),
        DmlTemporalKind::OffsetDatetime => DateTime::parse_from_rfc3339(value)
            .map(|value| {
                format!(
                    "TIMESTAMP '{} {}'",
                    value.format("%Y-%m-%d %H:%M:%S%.f"),
                    value.format("%:z")
                )
            })
            .map_err(|_| {
                invalid_oracle_dml("The Oracle timestamp with time zone value is invalid")
            }),
    }
}

fn parse_oracle_dml_time(value: &str) -> Result<NaiveTime, AppError> {
    ["%H:%M:%S%.f", "%H:%M:%S"]
        .into_iter()
        .find_map(|format| NaiveTime::parse_from_str(value, format).ok())
        .ok_or_else(|| invalid_oracle_dml("The Oracle time value is invalid"))
}

fn parse_oracle_dml_timestamp(value: &str) -> Result<NaiveDateTime, AppError> {
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| invalid_oracle_dml("The Oracle timestamp value is invalid"))
}

fn oracle_dml_binary(value: &[u8], column: &DmlColumn) -> Result<String, AppError> {
    const MAX_RAW_LITERAL_BYTES: usize = 2_000;
    if value.len() > MAX_RAW_LITERAL_BYTES {
        return Err(invalid_oracle_dml(
            "Oracle inline binary DML values cannot exceed 2000 bytes",
        ));
    }
    let raw = format!("HEXTORAW('{}')", hex::encode_upper(value));
    if column.data_type_name.eq_ignore_ascii_case("BLOB") {
        Ok(format!("TO_BLOB({raw})"))
    } else {
        Ok(raw)
    }
}

fn invalid_oracle_dml(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_oracle_dml", message)
}

fn quote_identifier(value: &str, field: &str) -> Result<String, AppError> {
    validate_metadata_identifier(value, field)?;
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{DatasourceConnection, DatasourceConnectionProperty};
    use oracle_rs::{
        Error as OracleError, OracleType, Value as OracleValue, config::ServiceMethod,
        types::OracleTimestamp,
    };

    use super::{
        OracleNativeDriver, apply_ssh_forward, build_oracle_create_schema, build_oracle_dml,
        build_oracle_namespace_sql, connection_config, format_oracle_timestamp_tz, oracle_bool,
        oracle_console_connection_error, oracle_console_interrupted, oracle_f32, oracle_f64,
        oracle_i64, oracle_query_parameters, oracle_result_type_supported,
        parse_jdbc_oracle_target, parse_oracle_url_target, quote_identifier, split_oracle_script,
        validate_metadata_identifier, validate_read_sql,
    };
    use crate::native_driver::NativeDriver as _;
    use crate::native_driver_types::{
        CreateSchemaSqlRequest, DatabaseDefinition, DmlAssignment, DmlColumn, DmlRow,
        DmlSqlRequest, DmlStatement, DmlTarget, DmlTemporalKind, DmlValue, NamespaceSqlOperation,
        NamespaceSqlRequest, SchemaDefinition,
    };
    use crate::query::{DatabaseValue, QueryParameter};

    #[test]
    fn oracle_urls_support_service_name_sid_credentials_and_tls() {
        let (service, _, _, tls) = parse_jdbc_oracle_target("db.example:1522/FREEPDB1?ssl=true")
            .expect("JDBC service-name URL must parse");
        assert_eq!(service.host, "db.example");
        assert_eq!(service.port, 1_522);
        assert_eq!(
            service.service,
            ServiceMethod::ServiceName("FREEPDB1".to_owned())
        );
        assert!(tls);

        let (sid, _, _, tls) =
            parse_jdbc_oracle_target("db.example:1523:ORCL").expect("JDBC SID URL must parse");
        assert_eq!(sid.host, "db.example");
        assert_eq!(sid.port, 1_523);
        assert_eq!(sid.service, ServiceMethod::Sid("ORCL".to_owned()));
        assert!(!tls);

        let (native, username, password, tls) =
            parse_oracle_url_target("oracle://scott:tiger@db.example:2484/ignored?sid=ORCL&tcps=1")
                .expect("native SID URL must parse");
        assert_eq!(native.service, ServiceMethod::Sid("ORCL".to_owned()));
        assert_eq!(username.as_deref(), Some("scott"));
        assert_eq!(password.as_deref(), Some("tiger"));
        assert!(tls);
    }

    #[test]
    fn oracle_connection_properties_reject_duplicates_and_unknown_url_options() {
        let duplicate_user = DatasourceConnection {
            jdbc_url: "jdbc:oracle:thin:@localhost:1521/FREEPDB1".to_owned(),
            properties: vec![
                property("user", "app", false),
                property("username", "duplicate", false),
                property("password", "secret", true),
            ],
            read_only: false,
            ssh: None,
        };
        let error = connection_config(&duplicate_user)
            .expect_err("duplicate username aliases must be rejected");
        assert_eq!(error.api_error().code, "invalid_oracle_connection");

        let error =
            parse_oracle_url_target("oracle://localhost/FREEPDB1?wallet=/tmp/not-supported")
                .expect_err("unknown native URL properties must be rejected");
        assert_eq!(error.api_error().code, "invalid_oracle_connection");

        let error = parse_oracle_url_target("oracle://localhost/FREEPDB1?sid=A&sid=B")
            .expect_err("duplicate SID properties must be rejected");
        assert_eq!(error.api_error().code, "invalid_oracle_connection");
    }

    #[test]
    fn oracle_tcps_configuration_never_panics_and_keeps_tls_identity_through_ssh() {
        let connection = DatasourceConnection {
            jdbc_url: "jdbc:oracle:thin:@db.example:2484/FREEPDB1?tcps=true".to_owned(),
            properties: vec![
                property("user", "app", false),
                property("password", "secret", true),
            ],
            read_only: false,
            ssh: None,
        };
        let configured = std::panic::catch_unwind(|| connection_config(&connection));
        assert!(configured.is_ok(), "TCPS configuration must not panic");
        let mut config = configured
            .expect("panic checked above")
            .expect("TCPS configuration must build");
        assert!(config.is_tls_enabled());

        apply_ssh_forward(&mut config, 31_521);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 31_521);
        assert_eq!(
            config
                .tls_config
                .as_ref()
                .and_then(|tls| tls.server_name.as_deref()),
            Some("db.example")
        );
    }

    #[test]
    fn oracle_console_only_reports_plain_cancellation_for_read_only_statements() {
        assert_eq!(
            oracle_console_interrupted(true, Some("cancelled".to_owned()))
                .api_error()
                .code,
            "oracle_console_cancelled"
        );
        assert_eq!(
            oracle_console_interrupted(false, Some("cancelled".to_owned()))
                .api_error()
                .code,
            "database_write_outcome_unknown"
        );
        let connection_error = OracleError::ConnectionClosed;
        assert_eq!(
            oracle_console_connection_error(true, &connection_error)
                .api_error()
                .code,
            "oracle_connection_failed"
        );
        assert_eq!(
            oracle_console_connection_error(false, &connection_error)
                .api_error()
                .code,
            "database_write_outcome_unknown"
        );
    }

    #[test]
    fn oracle_unsupported_result_types_fail_closed() {
        for oracle_type in [
            OracleType::BinaryFloat,
            OracleType::BinaryDouble,
            OracleType::Rowid,
            OracleType::Urowid,
            OracleType::Bfile,
            OracleType::Cursor,
            OracleType::Object,
            OracleType::Vector,
            OracleType::IntervalYm,
            OracleType::IntervalDs,
        ] {
            assert!(!oracle_result_type_supported(oracle_type));
        }
        for oracle_type in [
            OracleType::Number,
            OracleType::Long,
            OracleType::LongRaw,
            OracleType::Blob,
            OracleType::Clob,
            OracleType::Json,
            OracleType::Boolean,
        ] {
            assert!(oracle_result_type_supported(oracle_type));
        }
    }

    #[test]
    fn oracle_console_splitter_respects_strings_comments_q_quotes_and_plsql() {
        let statements = split_oracle_script(
            "SELECT ';' FROM DUAL; -- keep ; in comment\n\
             SELECT q'[a;b]' FROM DUAL; SELECT \"odd;name\" FROM DUAL",
        )
        .expect("valid Oracle script must split");
        assert_eq!(statements.len(), 3);
        assert_eq!(statements[0], "SELECT ';' FROM DUAL");
        assert!(statements[1].ends_with("SELECT q'[a;b]' FROM DUAL"));
        assert_eq!(statements[2], "SELECT \"odd;name\" FROM DUAL");

        let block = "BEGIN\n  DBMS_OUTPUT.PUT_LINE(q'[a;b]');\nEND;\n/";
        assert_eq!(
            split_oracle_script(block).expect("PL/SQL block must stay intact"),
            ["BEGIN\n  DBMS_OUTPUT.PUT_LINE(q'[a;b]');\nEND;"]
        );

        for invalid in [
            "SELECT 'unterminated",
            "SELECT q'[unterminated",
            "SELECT /* open",
        ] {
            assert!(
                split_oracle_script(invalid).is_err(),
                "{invalid} must be rejected"
            );
        }
    }

    #[test]
    fn oracle_native_reads_reject_writes_locking_and_multiple_statements() {
        for sql in [
            "SELECT 1 FROM DUAL",
            "/* leading */ SELECT 'FOR UPDATE' FROM DUAL",
            "WITH item AS (SELECT 1 value FROM DUAL) SELECT value FROM item",
        ] {
            validate_read_sql(sql)
                .unwrap_or_else(|error| panic!("{sql} should be accepted: {error}"));
        }

        for sql in [
            "UPDATE items SET value = 1",
            "SELECT * FROM items FOR UPDATE",
            "SELECT * FROM items FOR/**/UPDATE",
            "SELECT 1 FROM DUAL; SELECT 2 FROM DUAL",
        ] {
            let error = validate_read_sql(sql).expect_err("unsafe read SQL must be rejected");
            assert_eq!(error.api_error().code, "oracle_native_query_unsupported");
        }
    }

    #[test]
    fn oracle_parameters_are_sorted_and_must_be_contiguous() {
        let parameters = vec![
            QueryParameter {
                position: 2,
                value: DatabaseValue::Text("second".to_owned()),
            },
            QueryParameter {
                position: 1,
                value: DatabaseValue::SignedInteger(1),
            },
        ];
        let converted = oracle_query_parameters(&parameters)
            .expect("out-of-order contiguous parameters must be sorted");
        assert!(matches!(&converted[0], OracleValue::Integer(1)));
        assert!(matches!(&converted[1], OracleValue::String(value) if value == "second"));

        for positions in [[1, 1], [1, 3], [0, 1]] {
            let parameters = positions.map(|position| QueryParameter {
                position,
                value: DatabaseValue::Null,
            });
            let error = oracle_query_parameters(&parameters)
                .expect_err("duplicate, missing, and zero positions must be rejected");
            assert_eq!(error.api_error().code, "invalid_query_parameter");
        }
    }

    #[test]
    fn oracle_number_strings_convert_to_expected_numeric_wire_values() {
        let one = OracleValue::String("1".to_owned());
        assert_eq!(oracle_i64(&one), Some(1));
        assert_eq!(oracle_f32(&one), Some(1.0));
        assert_eq!(oracle_f64(&one), Some(1.0));

        let fractional = OracleValue::Float(1.5);
        assert_eq!(oracle_i64(&fractional), None);
    }

    #[test]
    fn oracle_boolean_decoding_accepts_only_known_wire_shapes() {
        assert_eq!(oracle_bool(&OracleValue::Boolean(true)), Some(true));
        assert_eq!(
            oracle_bool(&OracleValue::String("true".to_owned())),
            Some(true)
        );
        assert_eq!(
            oracle_bool(&OracleValue::String("false".to_owned())),
            Some(false)
        );
        assert_eq!(
            oracle_bool(&OracleValue::String("\u{1}\u{1}".to_owned())),
            Some(true)
        );
        assert_eq!(
            oracle_bool(&OracleValue::String("\u{1}\0".to_owned())),
            Some(false)
        );
        assert_eq!(
            oracle_bool(&OracleValue::String("\u{1}\u{2}".to_owned())),
            None
        );
    }

    #[test]
    fn oracle_timestamp_timezone_conversion_is_checked_and_crosses_days() {
        let positive = OracleTimestamp::with_timezone(2026, 12, 31, 20, 30, 0, 123_456, 8, 0);
        assert_eq!(
            format_oracle_timestamp_tz(positive).expect("positive Oracle offset"),
            "2027-01-01T04:30:00.123456+08:00"
        );
        let negative = OracleTimestamp::with_timezone(2026, 1, 1, 1, 15, 0, 0, -5, -30);
        assert_eq!(
            format_oracle_timestamp_tz(negative).expect("negative Oracle offset"),
            "2025-12-31T19:45:00-05:30"
        );
        let invalid = OracleTimestamp::with_timezone(2026, 1, 1, 1, 15, 0, 0, 1, -30);
        assert!(format_oracle_timestamp_tz(invalid).is_err());
    }

    #[test]
    fn oracle_dialect_builds_schema_and_supported_namespace_sql() {
        assert!(OracleNativeDriver.dialect().is_some());
        let schema = SchemaDefinition {
            database_name: "FREEPDB1".to_owned(),
            name: "APP".to_owned(),
            comment: String::new(),
            owner: "app".to_owned(),
            system: false,
        };
        assert_eq!(
            build_oracle_create_schema(CreateSchemaSqlRequest {
                schema: schema.clone(),
            })
            .expect("Oracle schema SQL")
            .sql,
            "CREATE SCHEMA AUTHORIZATION \"APP\";"
        );
        assert_eq!(
            build_oracle_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::CreateSchema { schema },
            })
            .expect("Oracle namespace schema SQL")
            .sql,
            "CREATE SCHEMA AUTHORIZATION \"APP\";"
        );
        assert_eq!(
            build_oracle_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::DropSchema {
                    schema_name: "Odd\"User".to_owned(),
                },
            })
            .expect("Oracle drop schema SQL")
            .sql,
            "DROP USER \"Odd\"\"User\" CASCADE;"
        );
        assert_eq!(
            build_oracle_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::CreateDatabase {
                    database: DatabaseDefinition {
                        name: "APPDB".to_owned(),
                        comment: String::new(),
                        charset: "AL32UTF8".to_owned(),
                        collation: String::new(),
                        owner: String::new(),
                        system: false,
                    },
                },
            })
            .expect("Oracle create database SQL")
            .sql,
            "CREATE DATABASE \"APPDB\" CHARACTER SET AL32UTF8;"
        );
        let error = build_oracle_namespace_sql(NamespaceSqlRequest {
            operation: NamespaceSqlOperation::UseDatabase {
                database_name: "APPDB".to_owned(),
            },
        })
        .expect_err("Oracle database switching must fail explicitly");
        assert_eq!(error.api_error().code, "oracle_database_switch_unsupported");
    }

    #[test]
    fn oracle_dialect_builds_typed_single_and_multi_insert_sql() {
        let target = DmlTarget {
            database_name: Some("FREEPDB1".to_owned()),
            schema_name: Some("APP".to_owned()),
            table_name: "ITEMS".to_owned(),
        };
        let columns = vec![
            dml_column("ID", "NUMBER"),
            dml_column("LABEL", "VARCHAR2"),
            dml_column("ACTIVE", "BOOLEAN"),
            dml_column("PAYLOAD", "BLOB"),
            dml_column("CREATED_AT", "TIMESTAMP"),
        ];
        let row = DmlRow {
            values: vec![
                DmlValue::Decimal("1".to_owned()),
                DmlValue::String("owner's".to_owned()),
                DmlValue::Boolean(true),
                DmlValue::Binary(vec![0, 255]),
                DmlValue::Temporal {
                    kind: DmlTemporalKind::LocalDatetime,
                    iso8601: "2026-08-07T12:34:56.123456".to_owned(),
                },
            ],
        };
        let sql = build_oracle_dml(DmlSqlRequest {
            target: target.clone(),
            statement: DmlStatement::SingleInsert {
                columns: columns.clone(),
                row,
            },
        })
        .expect("Oracle single INSERT SQL")
        .sql;
        assert_eq!(
            sql,
            "INSERT INTO \"APP\".\"ITEMS\" (\"ID\", \"LABEL\", \"ACTIVE\", \"PAYLOAD\", \"CREATED_AT\") VALUES (1, 'owner''s', TRUE, TO_BLOB(HEXTORAW('00FF')), TIMESTAMP '2026-08-07 12:34:56.123456');"
        );

        let sql = build_oracle_dml(DmlSqlRequest {
            target,
            statement: DmlStatement::MultiInsert {
                columns: vec![dml_column("ID", "NUMBER")],
                rows: vec![
                    DmlRow {
                        values: vec![DmlValue::Decimal("1".to_owned())],
                    },
                    DmlRow {
                        values: vec![DmlValue::Decimal("2".to_owned())],
                    },
                ],
            },
        })
        .expect("Oracle multi INSERT SQL")
        .sql;
        assert_eq!(
            sql,
            "INSERT ALL\n  INTO \"APP\".\"ITEMS\" (\"ID\") VALUES (1)\n  INTO \"APP\".\"ITEMS\" (\"ID\") VALUES (2)\nSELECT 1 FROM DUAL;"
        );
    }

    #[test]
    fn oracle_dialect_builds_bounded_update_sql() {
        let sql = build_oracle_dml(DmlSqlRequest {
            target: DmlTarget {
                database_name: None,
                schema_name: Some("APP".to_owned()),
                table_name: "ITEMS".to_owned(),
            },
            statement: DmlStatement::Update {
                assignments: vec![DmlAssignment {
                    column: dml_column("LABEL", "VARCHAR2"),
                    value: DmlValue::String("next".to_owned()),
                }],
                predicates: vec![
                    DmlAssignment {
                        column: dml_column("ID", "NUMBER"),
                        value: DmlValue::Decimal("7".to_owned()),
                    },
                    DmlAssignment {
                        column: dml_column("DELETED_AT", "TIMESTAMP"),
                        value: DmlValue::Null,
                    },
                ],
            },
        })
        .expect("Oracle UPDATE SQL")
        .sql;
        assert_eq!(
            sql,
            "UPDATE \"APP\".\"ITEMS\" SET \"LABEL\" = 'next' WHERE \"ID\" = 7 AND \"DELETED_AT\" IS NULL;"
        );
    }

    #[test]
    fn oracle_identifiers_are_quoted_and_bounded() {
        assert_eq!(
            quote_identifier("Odd\"Name", "tableName").expect("identifier must quote"),
            "\"Odd\"\"Name\""
        );
        for invalid in [String::new(), "bad\nname".to_owned(), "x".repeat(129)] {
            assert!(
                validate_metadata_identifier(&invalid, "tableName").is_err(),
                "invalid identifier must be rejected"
            );
        }
    }

    fn property(key: &str, value: &str, sensitive: bool) -> DatasourceConnectionProperty {
        DatasourceConnectionProperty {
            key: key.to_owned(),
            value: value.to_owned(),
            sensitive,
        }
    }

    fn dml_column(name: &str, data_type_name: &str) -> DmlColumn {
        DmlColumn {
            name: name.to_owned(),
            data_type_name: data_type_name.to_owned(),
            precision: None,
            scale: None,
        }
    }
}
