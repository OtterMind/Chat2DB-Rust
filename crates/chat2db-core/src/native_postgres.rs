use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt::Write as _,
    future::Future,
    mem::size_of,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
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
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta};
use futures_util::StreamExt;
use prost::Message;
use sqlparser::{dialect::PostgreSqlDialect, parser::Parser};
use tokio::{sync::watch, task::JoinHandle};
use tokio_postgres::{
    Client, Config, Error as PostgresError, NoTls, Row,
    config::SslMode,
    types::{Format, FromSql, IsNull, Kind, ToSql, Type, private::BytesMut},
};
use tokio_postgres_rustls::MakeRustlsConnect;
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
        BuiltSql, ColumnList, ColumnMetadata, CreateSchemaSqlRequest, DatabaseList,
        DatabaseMetadata, DmlAssignment, DmlColumn, DmlRow, DmlSqlRequest, DmlStatement, DmlTarget,
        DmlTemporalKind, DmlValue, EntityRelationColumn, EntityRelationForeignKey,
        EntityRelationTable, ForeignKeyList, ForeignKeyMetadata, FunctionList, FunctionMetadata,
        FunctionParameterList, FunctionParameterMetadata, IndexColumnMetadata, IndexList,
        IndexMetadata, ListColumnsRequest, ListDatabasesRequest, ListIndexesRequest,
        ListRoutinesRequest, ListSchemasRequest, ListTableKeysRequest, ListTablesRequest,
        ListTriggersRequest, ListViewsRequest, MetadataObjectRef, NamespaceSqlOperation,
        NamespaceSqlRequest, NativeDriverDescriptor, PrimaryKeyList, PrimaryKeyMetadata,
        ProcedureList, ProcedureMetadata, ProcedureParameterList, ProcedureParameterMetadata,
        SchemaList, SchemaMetadata, TableList, TableMetadata, TablePreviewAccepted,
        TablePreviewRequest, TriggerList, TriggerMetadata, ViewList,
    },
    operation::CancellationRequest,
    query::{
        DatabaseValue, DatabaseWriteError, NativeConsoleRequest, NativeConsoleResult,
        PreparedQuery, QueryExecutionOptions, QueryParameter, QueryTaskError, RetainedWriter,
    },
    ssh::{SshTunnel, SshTunnelIdentity},
};

const POSTGRES_SCHEME: &str = "postgresql://";
const JDBC_POSTGRES_SCHEME: &str = "jdbc:postgresql://";
const POSTGRES_DEFAULT_PORT: u16 = 5_432;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const CONSOLE_STATEMENT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
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
const MAX_IDENTIFIER_BYTES: usize = 63;
const MAX_CONSOLE_RESULT_BYTES: u64 = DEFAULT_RESULT_BYTES;
const MAX_CONSOLE_PAGE_SIZE: u32 = 10_000;
const MAX_CONSOLE_STATEMENTS: usize = 1_000;
const MAX_CONSOLE_SCANNED_ROWS: u64 = 1_000_000;
const MAX_CONSOLE_SCANNED_BYTES: u64 = MAX_RESULT_BYTES;
const POSTGRES_ARRAY_MAX_DIMENSIONS: usize = 6;

pub(crate) const POSTGRES_DRIVER_DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
    id: "postgresql",
    implementation: "tokio-postgres",
    database_types: &["POSTGRESQL", "POSTGRES"],
    compatibility_aliases: &["postgresql", "postgres", "tokio-postgres"],
};

pub(crate) struct PostgresNativeDriver;

impl NativeDriver for PostgresNativeDriver {
    fn descriptor(&self) -> &'static NativeDriverDescriptor {
        &POSTGRES_DRIVER_DESCRIPTOR
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

#[async_trait]
impl NativeConnectionDriver for PostgresNativeDriver {
    async fn test_connection(&self, connection: &DatasourceConnection) -> Result<(), AppError> {
        self.test_connection_with_local_port(connection)
            .await
            .map(|_| ())
    }

    async fn test_connection_with_local_port(
        &self,
        connection: &DatasourceConnection,
    ) -> Result<Option<u16>, AppError> {
        let connection = open_connection(connection).await?;
        let local_port = connection.local_tunnel_port();
        let result = postgres_timeout(
            CONNECT_TIMEOUT,
            "postgres_connection_timeout",
            "The PostgreSQL connection test timed out",
            connection.client().simple_query("SELECT 1"),
        )
        .await
        .map(|_| ());
        finish_connection(connection, result).await?;
        Ok(local_port)
    }
}

#[async_trait]
impl NativeQueryDriver for PostgresNativeDriver {
    fn is_read_candidate(&self, sql: &str) -> Result<bool, AppError> {
        is_native_read_candidate(sql)
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
impl NativeMetadataDriver for PostgresNativeDriver {
    async fn list_schemas(
        &self,
        application: &Application,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError> {
        list_schemas(application, &request.datasource_id, &request.database_name).await
    }

    async fn list_databases(
        &self,
        application: &Application,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError> {
        list_databases(application, &request.datasource_id).await
    }

    async fn list_tables(
        &self,
        application: &Application,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError> {
        list_tables(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name_pattern,
        )
        .await
    }

    async fn list_columns(
        &self,
        application: &Application,
        request: ListColumnsRequest,
    ) -> Result<ColumnList, AppError> {
        list_columns(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.scope.schema_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_indexes(
        &self,
        application: &Application,
        request: ListIndexesRequest,
    ) -> Result<IndexList, AppError> {
        list_indexes(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.scope.schema_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_views(
        &self,
        application: &Application,
        request: ListViewsRequest,
    ) -> Result<ViewList, AppError> {
        list_views(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name_pattern,
        )
        .await
    }

    async fn get_view(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TableMetadata, AppError> {
        get_view(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }

    async fn list_imported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        list_foreign_keys(application, &request, ForeignKeyDirection::Imported).await
    }

    async fn list_exported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        list_foreign_keys(application, &request, ForeignKeyDirection::Exported).await
    }

    async fn list_primary_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<PrimaryKeyList, AppError> {
        list_primary_keys(application, &request).await
    }

    async fn list_functions(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<FunctionList, AppError> {
        list_functions(application, &request).await
    }

    async fn get_function(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionMetadata, AppError> {
        get_function(application, &request).await
    }

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionParameterList, AppError> {
        list_function_parameters(application, &request).await
    }

    async fn list_procedures(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError> {
        list_procedures(application, &request).await
    }

    async fn get_procedure(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureMetadata, AppError> {
        get_procedure(application, &request).await
    }

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureParameterList, AppError> {
        list_procedure_parameters(application, &request).await
    }

    async fn list_triggers(
        &self,
        application: &Application,
        request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError> {
        list_triggers(application, &request).await
    }

    async fn get_trigger(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TriggerMetadata, AppError> {
        get_trigger(application, &request).await
    }
}

#[async_trait]
impl NativeTableDriver for PostgresNativeDriver {
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
        Err(postgres_capability_not_supported(
            "physical column reordering",
        ))
    }

    async fn table_ddl(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        table_ddl(
            application,
            datasource_id,
            database_name,
            schema_name,
            table_name,
        )
        .await
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

impl NativeDialectDriver for PostgresNativeDriver {
    fn build_create_schema(&self, request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
        build_create_schema(request)
    }

    fn build_namespace_sql(&self, request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
        build_namespace_sql(request)
    }

    fn build_dml(&self, request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
        build_dml(request)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostgresTlsMode {
    Disable,
    Prefer,
    Require,
}

struct PreparedPostgresConnection {
    config: Config,
    tls_mode: PostgresTlsMode,
    tunnel: Option<SshTunnel>,
}

struct ManagedPostgresConnection {
    client: Option<Client>,
    task: Option<JoinHandle<Result<(), PostgresError>>>,
    tunnel: Option<SshTunnel>,
}

impl ManagedPostgresConnection {
    fn new(
        client: Client,
        task: JoinHandle<Result<(), PostgresError>>,
        tunnel: Option<SshTunnel>,
    ) -> Self {
        Self {
            client: Some(client),
            task: Some(task),
            tunnel,
        }
    }

    fn client(&self) -> &Client {
        self.client
            .as_ref()
            .expect("managed PostgreSQL client exists until cleanup")
    }

    fn local_tunnel_port(&self) -> Option<u16> {
        self.tunnel.as_ref().map(SshTunnel::local_port)
    }

    async fn abort(mut self) {
        self.client.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(tunnel) = self.tunnel.take()
            && let Err(error) = tunnel.close().await
        {
            tracing::warn!(error = %error, "SSH tunnel cleanup failed after PostgreSQL abort");
        }
    }
}

impl Drop for ManagedPostgresConnection {
    fn drop(&mut self) {
        self.client.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn open_connection(
    connection: &DatasourceConnection,
) -> Result<ManagedPostgresConnection, AppError> {
    open_prepared_connection(
        prepare_connection_with_identity(connection, SshTunnelIdentity::Ephemeral, None).await?,
    )
    .await
}

async fn open_resolved_connection(
    resolved: &ResolvedDatasourceConnection,
    database_name: Option<&str>,
) -> Result<ManagedPostgresConnection, AppError> {
    open_prepared_connection(
        prepare_connection_with_identity(
            &resolved.connection,
            SshTunnelIdentity::Datasource {
                datasource_id: &resolved.datasource_id,
                revision: resolved.datasource_revision,
            },
            database_name,
        )
        .await?,
    )
    .await
}

async fn prepare_connection_with_identity(
    connection: &DatasourceConnection,
    identity: SshTunnelIdentity<'_>,
    database_name: Option<&str>,
) -> Result<PreparedPostgresConnection, AppError> {
    let (config, tls_mode, target_host, target_port) =
        connection_config(connection, database_name, None)?;
    let Some(ssh) = connection.ssh.as_ref() else {
        return Ok(PreparedPostgresConnection {
            config,
            tls_mode,
            tunnel: None,
        });
    };

    let tunnel = SshTunnel::open(identity, ssh, target_host, target_port).await?;
    let (mut config, tls_mode, _, _) =
        connection_config(connection, database_name, Some(tunnel.local_port()))?;
    config.hostaddr(IpAddr::V4(Ipv4Addr::LOCALHOST));
    Ok(PreparedPostgresConnection {
        config,
        tls_mode,
        tunnel: Some(tunnel),
    })
}

async fn open_prepared_connection(
    mut prepared: PreparedPostgresConnection,
) -> Result<ManagedPostgresConnection, AppError> {
    let connect = async {
        match prepared.tls_mode {
            PostgresTlsMode::Disable => {
                let (client, connection) = prepared
                    .config
                    .connect(NoTls)
                    .await
                    .map_err(postgres_connection_error)?;
                Ok(ManagedPostgresConnection::new(
                    client,
                    tokio::spawn(connection),
                    prepared.tunnel.take(),
                ))
            }
            PostgresTlsMode::Prefer | PostgresTlsMode::Require => {
                ensure_postgres_rustls_provider()?;
                let (connector, errors) = MakeRustlsConnect::with_native_certs().map_err(|_| {
                    AppError::unavailable(
                        "postgres_tls_roots_unavailable",
                        "No trusted system certificate roots are available for PostgreSQL TLS",
                    )
                })?;
                if !errors.is_empty() {
                    tracing::warn!(
                        error_count = errors.len(),
                        "some native PostgreSQL TLS certificates could not be loaded"
                    );
                }
                let (client, connection) = prepared
                    .config
                    .connect(connector)
                    .await
                    .map_err(postgres_connection_error)?;
                Ok(ManagedPostgresConnection::new(
                    client,
                    tokio::spawn(connection),
                    prepared.tunnel.take(),
                ))
            }
        }
    };

    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(Ok(connection)) => Ok(connection),
        Ok(Err(error)) => {
            close_tunnel_quietly(prepared.tunnel.take()).await;
            Err(error)
        }
        Err(_) => {
            close_tunnel_quietly(prepared.tunnel.take()).await;
            Err(AppError::unavailable(
                "postgres_connection_timeout",
                "The PostgreSQL connection attempt timed out",
            ))
        }
    }
}

fn ensure_postgres_rustls_provider() -> Result<(), AppError> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        Ok(())
    } else {
        Err(AppError::unavailable(
            "postgres_tls_provider_unavailable",
            "A cryptographic provider could not be initialized for PostgreSQL TLS",
        ))
    }
}

async fn finish_connection<T>(
    mut connection: ManagedPostgresConnection,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    connection.client.take();
    let close_result = match connection.task.take() {
        Some(mut task) => match tokio::time::timeout(DISCONNECT_TIMEOUT, &mut task).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(postgres_connection_error(error)),
            Ok(Err(_)) => Err(AppError::internal()),
            Err(_) => {
                task.abort();
                Err(AppError::unavailable(
                    "postgres_disconnect_timeout",
                    "The PostgreSQL connection did not close in time",
                ))
            }
        },
        None => Ok(()),
    };
    let tunnel_result = match connection.tunnel.take() {
        Some(tunnel) => tunnel.close().await,
        None => Ok(()),
    };
    let cleanup_result = close_result.and(tunnel_result);
    match result {
        Ok(value) => cleanup_result.map(|()| value),
        Err(primary) => {
            if let Err(cleanup_error) = cleanup_result {
                tracing::warn!(error = %cleanup_error, "PostgreSQL cleanup also failed");
            }
            Err(primary)
        }
    }
}

async fn close_tunnel_quietly(tunnel: Option<SshTunnel>) {
    if let Some(tunnel) = tunnel
        && let Err(error) = tunnel.close().await
    {
        tracing::warn!(error = %error, "SSH tunnel cleanup failed");
    }
}

fn connection_config(
    connection: &DatasourceConnection,
    database_name: Option<&str>,
    connect_port: Option<u16>,
) -> Result<(Config, PostgresTlsMode, String, u16), AppError> {
    let normalized = normalize_postgres_url(&connection.jdbc_url)?;
    let parsed = Url::parse(&normalized).map_err(|_| invalid_connection_url())?;
    if parsed.scheme() != "postgresql" || parsed.host_str().is_none() || parsed.fragment().is_some()
    {
        return Err(invalid_connection_url());
    }
    let target_host = parsed
        .host_str()
        .ok_or_else(invalid_connection_url)?
        .to_owned();
    let target_port = parsed.port().unwrap_or(POSTGRES_DEFAULT_PORT);
    let query_properties = parsed
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut base = parsed;
    base.set_query(None);
    if let Some(connect_port) = connect_port {
        base.set_port(Some(connect_port))
            .map_err(|()| invalid_connection_url())?;
    }
    let mut config = Config::from_str(base.as_str()).map_err(|_| invalid_connection_url())?;
    config.connect_timeout(CONNECT_TIMEOUT);
    config.application_name("Chat2DB-Rust");

    let mut tls_mode = PostgresTlsMode::Prefer;
    for (key, value) in query_properties
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .chain(
            connection
                .properties
                .iter()
                .map(|property| (property.key.as_str(), property.value.as_str())),
        )
    {
        apply_connection_property(&mut config, &mut tls_mode, key, value)?;
    }
    if let Some(database_name) = database_name {
        validate_identifier(database_name, "databaseName")?;
        config.dbname(database_name);
    }
    config.ssl_mode(match tls_mode {
        PostgresTlsMode::Disable => SslMode::Disable,
        PostgresTlsMode::Prefer => SslMode::Prefer,
        PostgresTlsMode::Require => SslMode::Require,
    });
    Ok((config, tls_mode, target_host, target_port))
}

fn apply_connection_property(
    config: &mut Config,
    tls_mode: &mut PostgresTlsMode,
    key: &str,
    value: &str,
) -> Result<(), AppError> {
    match key.trim().to_ascii_lowercase().as_str() {
        "user" | "username" => {
            config.user(value);
        }
        "password" => {
            config.password(value.as_bytes());
        }
        "database" | "databasename" => {
            config.dbname(value);
        }
        "applicationname" | "application_name" => {
            config.application_name(value);
        }
        "options" => {
            config.options(value);
        }
        "connecttimeout" | "connect_timeout" => {
            let seconds = value
                .trim()
                .parse::<u64>()
                .ok()
                .filter(|seconds| *seconds > 0)
                .ok_or_else(|| invalid_connection_property("connectTimeout"))?;
            config.connect_timeout(Duration::from_secs(seconds.min(300)));
        }
        "ssl" => {
            *tls_mode = if parse_bool(value) {
                PostgresTlsMode::Require
            } else {
                PostgresTlsMode::Disable
            };
        }
        "sslmode" => {
            *tls_mode = match value.trim().to_ascii_lowercase().as_str() {
                "disable" | "disabled" | "false" => PostgresTlsMode::Disable,
                "allow" | "prefer" => PostgresTlsMode::Prefer,
                "require" | "verify-ca" | "verify-full" | "true" => PostgresTlsMode::Require,
                _ => return Err(invalid_connection_property("sslMode")),
            };
        }
        "sslrootcert" | "sslcert" | "sslkey" if !value.trim().is_empty() => {
            return Err(AppError::invalid(
                "postgres_tls_property_not_supported",
                "Custom PostgreSQL TLS certificate files are not supported; use the system trust store",
            ));
        }
        "currentschema" | "current_schema" => {
            validate_identifier(value, "currentSchema")?;
            config.options(format!("-c search_path={}", quote_config_value(value)?));
        }
        _ => {
            return Err(AppError::invalid(
                "postgres_connection_property_not_supported",
                format!("The PostgreSQL connection property {key} is not supported"),
            ));
        }
    }
    Ok(())
}

fn quote_config_value(value: &str) -> Result<String, AppError> {
    if value
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte == b'\\')
    {
        return Err(invalid_connection_property("currentSchema"));
    }
    Ok(value.to_owned())
}

fn normalize_postgres_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value
        .get(..JDBC_POSTGRES_SCHEME.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(JDBC_POSTGRES_SCHEME))
    {
        return Ok(format!(
            "{POSTGRES_SCHEME}{}",
            &value[JDBC_POSTGRES_SCHEME.len()..]
        ));
    }
    for scheme in [POSTGRES_SCHEME, "postgres://"] {
        if value
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        {
            return Ok(format!("{POSTGRES_SCHEME}{}", &value[scheme.len()..]));
        }
    }
    Err(invalid_connection_url())
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "required"
    )
}

fn invalid_connection_url() -> AppError {
    AppError::invalid(
        "invalid_postgres_connection",
        "A valid jdbc:postgresql://, postgresql://, or postgres:// connection URL is required",
    )
}

fn invalid_connection_property(property: &str) -> AppError {
    AppError::invalid(
        "invalid_postgres_connection",
        format!("The PostgreSQL connection property {property} is invalid"),
    )
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the owned signature lets this mapper be passed directly to map_err"
)]
fn postgres_connection_error(error: PostgresError) -> AppError {
    if let Some(database) = error.as_db_error() {
        return AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("postgres_connection_rejected", database.message()),
        );
    }
    AppError::unavailable(
        "postgres_connection_failed",
        "The PostgreSQL server could not be reached",
    )
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the owned signature lets this mapper be passed directly to map_err"
)]
fn postgres_query_error(error: PostgresError) -> AppError {
    if let Some(database) = error.as_db_error() {
        return AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("postgres_query_rejected", database.message()),
        );
    }
    AppError::unavailable(
        "postgres_query_failed",
        "The PostgreSQL query could not be completed",
    )
}

async fn postgres_timeout<T, F>(
    duration: Duration,
    code: &'static str,
    message: &'static str,
    future: F,
) -> Result<T, AppError>
where
    F: Future<Output = Result<T, PostgresError>>,
{
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| AppError::unavailable(code, message))?
        .map_err(postgres_query_error)
}

async fn resolve_native_connection(
    application: &Application,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    if application
        .native_driver_for_datasource_driver_id(&resolved.driver_id)
        .is_none_or(|driver| driver.descriptor().id != POSTGRES_DRIVER_DESCRIPTOR.id)
    {
        return Err(AppError::invalid(
            "postgres_driver_mismatch",
            "The datasource is not configured with a PostgreSQL driver",
        ));
    }
    Ok(resolved)
}

fn postgres_capability_not_supported(capability: &'static str) -> AppError {
    AppError::invalid(
        "native_driver_capability_not_supported",
        format!("The PostgreSQL driver does not implement {capability}"),
    )
}

async fn metadata_rows(
    application: &Application,
    datasource_id: &str,
    database_name: Option<&str>,
    sql: &str,
    parameters: &[&(dyn ToSql + Sync)],
) -> Result<Vec<Row>, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let connection = open_resolved_connection(&resolved, database_name).await?;
    let result = postgres_timeout(
        METADATA_TIMEOUT,
        "postgres_metadata_timeout",
        "The PostgreSQL metadata query did not finish in time",
        connection.client().query(sql, parameters),
    )
    .await;
    finish_connection(connection, result).await
}

async fn list_databases(
    application: &Application,
    datasource_id: &str,
) -> Result<DatabaseList, AppError> {
    let rows = metadata_rows(
        application,
        datasource_id,
        None,
        "SELECT d.datname, pg_encoding_to_char(d.encoding), d.datcollate, \
                pg_get_userbyid(d.datdba), COALESCE(obj_description(d.oid, 'pg_database'), ''), \
                d.datistemplate OR NOT d.datallowconn \
         FROM pg_database d ORDER BY d.datname",
        &[],
    )
    .await?;
    Ok(DatabaseList {
        items: rows
            .into_iter()
            .map(|row| {
                Ok(DatabaseMetadata {
                    name: row.try_get(0).map_err(postgres_query_error)?,
                    charset: row.try_get(1).map_err(postgres_query_error)?,
                    collation: row.try_get(2).map_err(postgres_query_error)?,
                    owner: row.try_get(3).map_err(postgres_query_error)?,
                    comment: row.try_get(4).map_err(postgres_query_error)?,
                    system: row.try_get(5).map_err(postgres_query_error)?,
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

async fn list_schemas(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) -> Result<SchemaList, AppError> {
    validate_identifier(database_name, "databaseName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT current_database(), n.nspname, \
                COALESCE(obj_description(n.oid, 'pg_namespace'), ''), \
                pg_get_userbyid(n.nspowner), \
                n.nspname LIKE 'pg\\_%' ESCAPE '\\' OR n.nspname = 'information_schema' \
         FROM pg_namespace n ORDER BY n.nspname",
        &[],
    )
    .await?;
    Ok(SchemaList {
        items: rows
            .into_iter()
            .map(|row| {
                Ok(SchemaMetadata {
                    database_name: row.try_get(0).map_err(postgres_query_error)?,
                    name: row.try_get(1).map_err(postgres_query_error)?,
                    comment: row.try_get(2).map_err(postgres_query_error)?,
                    owner: row.try_get(3).map_err(postgres_query_error)?,
                    system: row.try_get(4).map_err(postgres_query_error)?,
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

async fn list_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    name_pattern: &str,
) -> Result<TableList, AppError> {
    validate_metadata_scope(database_name, schema_name)?;
    let pattern = name_pattern.trim().to_owned();
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT current_database(), n.nspname, c.relname, c.relkind::text, \
                COALESCE(obj_description(c.oid, 'pg_class'), ''), \
                COALESCE(ts.spcname, ''), c.reltuples::bigint::text, \
                pg_total_relation_size(c.oid)::bigint::text, c.relpersistence::text \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace \
         WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f') \
           AND ($2 = '' OR c.relname LIKE $2 ESCAPE '\\') \
         ORDER BY c.relname",
        &[&schema_name, &pattern],
    )
    .await?;
    Ok(TableList {
        items: rows
            .iter()
            .map(postgres_table_metadata)
            .collect::<Result<_, _>>()?,
    })
}

fn postgres_table_metadata(row: &Row) -> Result<TableMetadata, AppError> {
    let relation_kind: String = row.try_get(3).map_err(postgres_query_error)?;
    let persistence: String = row.try_get(8).map_err(postgres_query_error)?;
    let engine = match (relation_kind.as_str(), persistence.as_str()) {
        ("p", _) => "PARTITIONED",
        ("f", _) => "FOREIGN",
        (_, "u") => "UNLOGGED",
        (_, "t") => "TEMPORARY",
        _ => "HEAP",
    };
    Ok(TableMetadata {
        database_name: row.try_get(0).map_err(postgres_query_error)?,
        schema_name: row.try_get(1).map_err(postgres_query_error)?,
        name: row.try_get(2).map_err(postgres_query_error)?,
        table_type: "TABLE".to_owned(),
        comment: row.try_get(4).map_err(postgres_query_error)?,
        database_type: "POSTGRESQL".to_owned(),
        engine: engine.to_owned(),
        tablespace: row.try_get(5).map_err(postgres_query_error)?,
        rows: row.try_get(6).map_err(postgres_query_error)?,
        data_length: row.try_get(7).map_err(postgres_query_error)?,
        ..TableMetadata::default()
    })
}

async fn list_columns(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<ColumnList, AppError> {
    validate_metadata_table(database_name, schema_name, table_name)?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT current_database(), n.nspname, c.relname, a.attname, \
                format_type(a.atttypid, a.atttypmod), t.typname, \
                pg_get_expr(ad.adbin, ad.adrelid), a.attnotnull, \
                a.attidentity::text, a.attgenerated::text, \
                COALESCE(col_description(c.oid, a.attnum), ''), a.attnum::int4, \
                information_schema._pg_numeric_precision(a.atttypid, a.atttypmod)::int4, \
                information_schema._pg_numeric_scale(a.atttypid, a.atttypmod)::int4, \
                information_schema._pg_char_max_length(a.atttypid, a.atttypmod)::int4, \
                COALESCE(coll.collname, ''), COALESCE(pk.conname, ''), \
                COALESCE(array_position(pk.conkey, a.attnum), 0)::int4 \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         JOIN pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_attrdef ad ON ad.adrelid = c.oid AND ad.adnum = a.attnum \
         LEFT JOIN pg_collation coll ON coll.oid = a.attcollation AND a.attcollation <> 0 \
         LEFT JOIN pg_constraint pk ON pk.conrelid = c.oid AND pk.contype = 'p' \
                                    AND a.attnum = ANY(pk.conkey) \
         WHERE n.nspname = $1 AND c.relname = $2 \
         ORDER BY a.attnum",
        &[&schema_name, &table_name],
    )
    .await?;
    Ok(ColumnList {
        items: rows
            .iter()
            .map(postgres_column_metadata)
            .collect::<Result<_, _>>()?,
    })
}

fn postgres_column_metadata(row: &Row) -> Result<ColumnMetadata, AppError> {
    let column_type: String = row.try_get(4).map_err(postgres_query_error)?;
    let type_name: String = row.try_get(5).map_err(postgres_query_error)?;
    let default_value: Option<String> = row.try_get(6).map_err(postgres_query_error)?;
    let not_null: bool = row.try_get(7).map_err(postgres_query_error)?;
    let identity: String = row.try_get(8).map_err(postgres_query_error)?;
    let generated: String = row.try_get(9).map_err(postgres_query_error)?;
    let primary_key_name: String = row.try_get(16).map_err(postgres_query_error)?;
    let primary_key_order: i32 = row.try_get(17).map_err(postgres_query_error)?;
    Ok(ColumnMetadata {
        database_name: row.try_get(0).map_err(postgres_query_error)?,
        schema_name: row.try_get(1).map_err(postgres_query_error)?,
        table_name: row.try_get(2).map_err(postgres_query_error)?,
        name: row.try_get(3).map_err(postgres_query_error)?,
        column_type,
        data_type: Some(postgres_jdbc_type_name(&type_name)),
        default_value,
        auto_increment: Some(!identity.is_empty()),
        comment: row.try_get(10).map_err(postgres_query_error)?,
        primary_key: Some(primary_key_order > 0),
        primary_key_name,
        primary_key_order,
        column_size: row.try_get(14).map_err(postgres_query_error)?,
        decimal_digits: row.try_get(13).map_err(postgres_query_error)?,
        num_prec_radix: row
            .try_get::<_, Option<i32>>(12)
            .map_err(postgres_query_error)?
            .map(|_| 10),
        ordinal_position: Some(row.try_get(11).map_err(postgres_query_error)?),
        nullable: Some(i32::from(!not_null)),
        generated_column: Some(!generated.is_empty()),
        extent: if !identity.is_empty() {
            format!(
                "GENERATED {} AS IDENTITY",
                if identity == "a" {
                    "ALWAYS"
                } else {
                    "BY DEFAULT"
                }
            )
        } else if !generated.is_empty() {
            "GENERATED ALWAYS".to_owned()
        } else {
            String::new()
        },
        collation: row.try_get(15).map_err(postgres_query_error)?,
        ..ColumnMetadata::default()
    })
}

async fn list_indexes(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<IndexList, AppError> {
    validate_metadata_table(database_name, schema_name, table_name)?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT idx.relname, i.indisunique, am.amname, \
                COALESCE(obj_description(idx.oid, 'pg_class'), ''), \
                key.ordinality::int4, \
                COALESCE(att.attname, pg_get_indexdef(i.indexrelid, key.ordinality, true)), \
                COALESCE(pg_get_expr(i.indpred, i.indrelid), ''), \
                CASE WHEN ((i.indoption::smallint[])[key.ordinality - 1] & 1) = 1 \
                     THEN 'D' ELSE 'A' END \
         FROM pg_index i \
         JOIN pg_class tbl ON tbl.oid = i.indrelid \
         JOIN pg_namespace n ON n.oid = tbl.relnamespace \
         JOIN pg_class idx ON idx.oid = i.indexrelid \
         JOIN pg_am am ON am.oid = idx.relam \
         CROSS JOIN LATERAL generate_series(1, i.indnatts) key(ordinality) \
         LEFT JOIN pg_attribute att ON att.attrelid = tbl.oid \
              AND att.attnum = (i.indkey::smallint[])[key.ordinality - 1] \
         WHERE n.nspname = $1 AND tbl.relname = $2 \
         ORDER BY idx.relname, key.ordinality",
        &[&schema_name, &table_name],
    )
    .await?;

    let mut indexes = BTreeMap::<String, IndexMetadata>::new();
    for row in rows {
        let name: String = row.try_get(0).map_err(postgres_query_error)?;
        let unique: bool = row.try_get(1).map_err(postgres_query_error)?;
        let method: String = row.try_get(2).map_err(postgres_query_error)?;
        let comment: String = row.try_get(3).map_err(postgres_query_error)?;
        let ordinal_position: i32 = row.try_get(4).map_err(postgres_query_error)?;
        let column_name: String = row.try_get(5).map_err(postgres_query_error)?;
        let filter_condition: String = row.try_get(6).map_err(postgres_query_error)?;
        let sort_order: String = row.try_get(7).map_err(postgres_query_error)?;
        let index = indexes
            .entry(name.clone())
            .or_insert_with(|| IndexMetadata {
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
                table_name: table_name.to_owned(),
                name: name.clone(),
                index_type: if name.ends_with("_pkey") {
                    "PRIMARY".to_owned()
                } else if unique {
                    "UNIQUE".to_owned()
                } else {
                    "INDEX".to_owned()
                },
                unique: Some(unique),
                comment,
                method: method.clone(),
                ..IndexMetadata::default()
            });
        index.columns.push(IndexColumnMetadata {
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: table_name.to_owned(),
            index_name: name,
            column_name,
            ordinal_position: Some(ordinal_position),
            non_unique: Some(!unique),
            sort_order,
            filter_condition,
            ..IndexColumnMetadata::default()
        });
    }
    Ok(IndexList {
        items: indexes.into_values().collect(),
    })
}

async fn list_views(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    name_pattern: &str,
) -> Result<ViewList, AppError> {
    validate_metadata_scope(database_name, schema_name)?;
    let pattern = name_pattern.trim().to_owned();
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT current_database(), n.nspname, c.relname, c.relkind::text, \
                COALESCE(obj_description(c.oid, 'pg_class'), ''), \
                pg_get_viewdef(c.oid, true) \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relkind IN ('v', 'm') \
           AND ($2 = '' OR c.relname LIKE $2 ESCAPE '\\') \
         ORDER BY c.relname",
        &[&schema_name, &pattern],
    )
    .await?;
    Ok(ViewList {
        items: rows
            .iter()
            .map(postgres_view_metadata)
            .collect::<Result<_, _>>()?,
    })
}

async fn get_view(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    view_name: &str,
) -> Result<TableMetadata, AppError> {
    validate_metadata_table(database_name, schema_name, view_name)?;
    let mut rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT current_database(), n.nspname, c.relname, c.relkind::text, \
                COALESCE(obj_description(c.oid, 'pg_class'), ''), \
                pg_get_viewdef(c.oid, true) \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v', 'm')",
        &[&schema_name, &view_name],
    )
    .await?;
    let row = rows
        .pop()
        .ok_or_else(|| metadata_not_found("view", database_name, schema_name, view_name))?;
    postgres_view_metadata(&row)
}

fn postgres_view_metadata(row: &Row) -> Result<TableMetadata, AppError> {
    let database_name: String = row.try_get(0).map_err(postgres_query_error)?;
    let schema_name: String = row.try_get(1).map_err(postgres_query_error)?;
    let name: String = row.try_get(2).map_err(postgres_query_error)?;
    let kind: String = row.try_get(3).map_err(postgres_query_error)?;
    let definition: String = row.try_get(5).map_err(postgres_query_error)?;
    let materialized = kind == "m";
    let ddl = format!(
        "CREATE {}VIEW {}.{} AS\n{};",
        if materialized { "MATERIALIZED " } else { "" },
        quote_identifier(&schema_name, "schemaName")?,
        quote_identifier(&name, "viewName")?,
        definition.trim_end_matches(';')
    );
    Ok(TableMetadata {
        database_name,
        schema_name,
        name,
        table_type: if materialized {
            "MATERIALIZED VIEW".to_owned()
        } else {
            "VIEW".to_owned()
        },
        comment: row.try_get(4).map_err(postgres_query_error)?,
        database_type: "POSTGRESQL".to_owned(),
        ddl,
        ..TableMetadata::default()
    })
}

#[derive(Clone, Copy)]
enum ForeignKeyDirection {
    Imported,
    Exported,
}

async fn list_foreign_keys(
    application: &Application,
    request: &ListTableKeysRequest,
    direction: ForeignKeyDirection,
) -> Result<ForeignKeyList, AppError> {
    let scope = &request.table.scope;
    validate_metadata_table(
        &scope.database_name,
        &scope.schema_name,
        &request.table.table_name,
    )?;
    let filter = match direction {
        ForeignKeyDirection::Imported => "fn.nspname = $1 AND ft.relname = $2",
        ForeignKeyDirection::Exported => "pn.nspname = $1 AND pt.relname = $2",
    };
    let sql = format!(
        "SELECT current_database(), pn.nspname, pt.relname, pa.attname, \
                fn.nspname, ft.relname, fa.attname, pos.n::int4, \
                con.confupdtype::text, con.confdeltype::text, con.conname, \
                COALESCE(pkidx.relname, ''), con.condeferrable, con.condeferred \
         FROM pg_constraint con \
         JOIN pg_class ft ON ft.oid = con.conrelid \
         JOIN pg_namespace fn ON fn.oid = ft.relnamespace \
         JOIN pg_class pt ON pt.oid = con.confrelid \
         JOIN pg_namespace pn ON pn.oid = pt.relnamespace \
         JOIN LATERAL generate_subscripts(con.conkey, 1) pos(n) ON true \
         JOIN pg_attribute fa ON fa.attrelid = ft.oid AND fa.attnum = con.conkey[pos.n] \
         JOIN pg_attribute pa ON pa.attrelid = pt.oid AND pa.attnum = con.confkey[pos.n] \
         LEFT JOIN pg_class pkidx ON pkidx.oid = con.conindid \
         WHERE con.contype = 'f' AND {filter} \
         ORDER BY con.conname, pos.n"
    );
    let rows = metadata_rows(
        application,
        &scope.datasource_id,
        Some(&scope.database_name),
        &sql,
        &[&scope.schema_name, &request.table.table_name],
    )
    .await?;
    Ok(ForeignKeyList {
        items: rows
            .into_iter()
            .map(|row| {
                let deferrable: bool = row.try_get(12).map_err(postgres_query_error)?;
                let deferred: bool = row.try_get(13).map_err(postgres_query_error)?;
                Ok(ForeignKeyMetadata {
                    primary_table_database: row.try_get(0).map_err(postgres_query_error)?,
                    primary_table_schema: row.try_get(1).map_err(postgres_query_error)?,
                    primary_table_name: row.try_get(2).map_err(postgres_query_error)?,
                    primary_column_name: row.try_get(3).map_err(postgres_query_error)?,
                    foreign_table_database: row.try_get(0).map_err(postgres_query_error)?,
                    foreign_table_schema: row.try_get(4).map_err(postgres_query_error)?,
                    foreign_table_name: row.try_get(5).map_err(postgres_query_error)?,
                    foreign_column_name: row.try_get(6).map_err(postgres_query_error)?,
                    key_sequence: row.try_get(7).map_err(postgres_query_error)?,
                    update_rule: postgres_referential_rule(
                        &row.try_get::<_, String>(8).map_err(postgres_query_error)?,
                    ),
                    delete_rule: postgres_referential_rule(
                        &row.try_get::<_, String>(9).map_err(postgres_query_error)?,
                    ),
                    foreign_key_name: row.try_get(10).map_err(postgres_query_error)?,
                    primary_key_name: row.try_get(11).map_err(postgres_query_error)?,
                    deferrability: if !deferrable {
                        7
                    } else if deferred {
                        5
                    } else {
                        6
                    },
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

async fn list_primary_keys(
    application: &Application,
    request: &ListTableKeysRequest,
) -> Result<PrimaryKeyList, AppError> {
    let scope = &request.table.scope;
    validate_metadata_table(
        &scope.database_name,
        &scope.schema_name,
        &request.table.table_name,
    )?;
    let rows = metadata_rows(
        application,
        &scope.datasource_id,
        Some(&scope.database_name),
        "SELECT current_database(), n.nspname, c.relname, a.attname, con.conname \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN LATERAL unnest(con.conkey) WITH ORDINALITY key(attnum, ordinality) ON true \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = key.attnum \
         WHERE con.contype = 'p' AND n.nspname = $1 AND c.relname = $2 \
         ORDER BY key.ordinality",
        &[&scope.schema_name, &request.table.table_name],
    )
    .await?;
    Ok(PrimaryKeyList {
        items: rows
            .into_iter()
            .map(|row| {
                Ok(PrimaryKeyMetadata {
                    database_name: row.try_get(0).map_err(postgres_query_error)?,
                    schema_name: row.try_get(1).map_err(postgres_query_error)?,
                    table_name: row.try_get(2).map_err(postgres_query_error)?,
                    column_name: row.try_get(3).map_err(postgres_query_error)?,
                    name: row.try_get(4).map_err(postgres_query_error)?,
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

fn postgres_referential_rule(value: &str) -> i32 {
    match value {
        "c" => 0,
        "r" => 1,
        "n" => 2,
        "d" => 4,
        _ => 3,
    }
}

fn validate_metadata_scope(database_name: &str, schema_name: &str) -> Result<(), AppError> {
    validate_identifier(database_name, "databaseName")?;
    validate_identifier(schema_name, "schemaName")
}

fn validate_metadata_table(
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<(), AppError> {
    validate_metadata_scope(database_name, schema_name)?;
    validate_identifier(table_name, "tableName")
}

fn validate_identifier(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_postgres_metadata_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn quote_identifier(value: &str, field: &str) -> Result<String, AppError> {
    validate_identifier(value, field)?;
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

fn metadata_not_found(
    kind: &str,
    database_name: &str,
    schema_name: &str,
    object_name: &str,
) -> AppError {
    AppError::not_found(
        "postgres_metadata_not_found",
        format!("PostgreSQL {kind} {database_name}.{schema_name}.{object_name} was not found"),
    )
}

async fn list_functions(
    application: &Application,
    request: &ListRoutinesRequest,
) -> Result<FunctionList, AppError> {
    validate_metadata_scope(&request.scope.database_name, &request.scope.schema_name)?;
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        "SELECT current_database(), n.nspname, p.proname, \
                COALESCE(obj_description(p.oid, 'pg_proc'), ''), p.oid::text, \
                pg_get_function_identity_arguments(p.oid) \
         FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = $1 AND p.prokind = 'f' \
         ORDER BY p.proname, pg_get_function_identity_arguments(p.oid)",
        &[&request.scope.schema_name],
    )
    .await?;
    Ok(FunctionList {
        items: rows
            .into_iter()
            .map(|row| {
                let name: String = row.try_get(2).map_err(postgres_query_error)?;
                let oid: String = row.try_get(4).map_err(postgres_query_error)?;
                let arguments: String = row.try_get(5).map_err(postgres_query_error)?;
                Ok(FunctionMetadata {
                    database_name: row.try_get(0).map_err(postgres_query_error)?,
                    schema_name: row.try_get(1).map_err(postgres_query_error)?,
                    name: name.clone(),
                    remarks: row.try_get(3).map_err(postgres_query_error)?,
                    function_type: Some(1),
                    specific_name: format!("{name}_{oid}"),
                    template: format!("{name}({arguments})"),
                    ..FunctionMetadata::default()
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

async fn get_function(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<FunctionMetadata, AppError> {
    let routine = resolve_routine(application, request, "f", "function").await?;
    Ok(FunctionMetadata {
        database_name: request.scope.database_name.clone(),
        schema_name: request.scope.schema_name.clone(),
        name: routine.name.clone(),
        remarks: routine.remarks,
        function_type: Some(1),
        specific_name: format!("{}_{}", routine.name, routine.oid),
        body: routine.definition,
        template: format!("{}({})", routine.name, routine.identity_arguments),
    })
}

async fn list_function_parameters(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<FunctionParameterList, AppError> {
    let routine = resolve_routine(application, request, "f", "function").await?;
    let rows = routine_parameter_rows(application, request, routine.oid, true).await?;
    Ok(FunctionParameterList {
        items: rows
            .into_iter()
            .map(|row| {
                let ordinal: i32 = row.try_get(0).map_err(postgres_query_error)?;
                let mode: String = row.try_get(2).map_err(postgres_query_error)?;
                let type_name: String = row.try_get(3).map_err(postgres_query_error)?;
                Ok(FunctionParameterMetadata {
                    function_database: request.scope.database_name.clone(),
                    function_schema: request.scope.schema_name.clone(),
                    function_name: routine.name.clone(),
                    column_name: row.try_get(1).map_err(postgres_query_error)?,
                    column_type: Some(postgres_function_column_type(&mode, ordinal)),
                    data_type: Some(postgres_jdbc_type_name(&type_name)),
                    type_name,
                    ordinal_position: Some(ordinal),
                    nullable: Some(2),
                    is_nullable: String::new(),
                    specific_name: format!("{}_{}", routine.name, routine.oid),
                    ..FunctionParameterMetadata::default()
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

async fn list_procedures(
    application: &Application,
    request: &ListRoutinesRequest,
) -> Result<ProcedureList, AppError> {
    validate_metadata_scope(&request.scope.database_name, &request.scope.schema_name)?;
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        "SELECT current_database(), n.nspname, p.proname, \
                COALESCE(obj_description(p.oid, 'pg_proc'), ''), p.oid::text \
         FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = $1 AND p.prokind = 'p' \
         ORDER BY p.proname, pg_get_function_identity_arguments(p.oid)",
        &[&request.scope.schema_name],
    )
    .await?;
    Ok(ProcedureList {
        items: rows
            .into_iter()
            .map(|row| {
                let name: String = row.try_get(2).map_err(postgres_query_error)?;
                let oid: String = row.try_get(4).map_err(postgres_query_error)?;
                Ok(ProcedureMetadata {
                    database_name: row.try_get(0).map_err(postgres_query_error)?,
                    schema_name: row.try_get(1).map_err(postgres_query_error)?,
                    name: name.clone(),
                    remarks: row.try_get(3).map_err(postgres_query_error)?,
                    procedure_type: Some(2),
                    specific_name: format!("{name}_{oid}"),
                    body: String::new(),
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

async fn get_procedure(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<ProcedureMetadata, AppError> {
    let routine = resolve_routine(application, request, "p", "procedure").await?;
    Ok(ProcedureMetadata {
        database_name: request.scope.database_name.clone(),
        schema_name: request.scope.schema_name.clone(),
        name: routine.name.clone(),
        remarks: routine.remarks,
        procedure_type: Some(2),
        specific_name: format!("{}_{}", routine.name, routine.oid),
        body: routine.definition,
    })
}

async fn list_procedure_parameters(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<ProcedureParameterList, AppError> {
    let routine = resolve_routine(application, request, "p", "procedure").await?;
    let rows = routine_parameter_rows(application, request, routine.oid, false).await?;
    Ok(ProcedureParameterList {
        items: rows
            .into_iter()
            .map(|row| {
                let ordinal: i32 = row.try_get(0).map_err(postgres_query_error)?;
                let mode: String = row.try_get(2).map_err(postgres_query_error)?;
                let type_name: String = row.try_get(3).map_err(postgres_query_error)?;
                Ok(ProcedureParameterMetadata {
                    procedure_database: request.scope.database_name.clone(),
                    procedure_schema: request.scope.schema_name.clone(),
                    procedure_name: routine.name.clone(),
                    column_name: row.try_get(1).map_err(postgres_query_error)?,
                    column_type: Some(postgres_procedure_column_type(&mode)),
                    data_type: Some(postgres_jdbc_type_name(&type_name)),
                    type_name,
                    ordinal_position: Some(ordinal),
                    nullable: Some(2),
                    specific_name: format!("{}_{}", routine.name, routine.oid),
                    ..ProcedureParameterMetadata::default()
                })
            })
            .collect::<Result<_, AppError>>()?,
    })
}

struct ResolvedRoutine {
    oid: u32,
    name: String,
    remarks: String,
    definition: String,
    identity_arguments: String,
}

async fn resolve_routine(
    application: &Application,
    request: &MetadataObjectRef,
    kind: &str,
    label: &str,
) -> Result<ResolvedRoutine, AppError> {
    validate_metadata_scope(&request.scope.database_name, &request.scope.schema_name)?;
    validate_identifier(&request.object_name, "routineName")?;
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        "SELECT p.oid, p.proname, COALESCE(obj_description(p.oid, 'pg_proc'), ''), \
                pg_get_functiondef(p.oid), pg_get_function_identity_arguments(p.oid) \
         FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = $1 AND p.prokind::text = $2 \
           AND (p.proname = $3 OR p.proname || '_' || p.oid::text = $3) \
         ORDER BY p.oid LIMIT 2",
        &[&request.scope.schema_name, &kind, &request.object_name],
    )
    .await?;
    if rows.is_empty() {
        return Err(metadata_not_found(
            label,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        ));
    }
    if rows.len() > 1 {
        return Err(AppError::invalid(
            "postgres_routine_ambiguous",
            format!(
                "PostgreSQL {label} {} is overloaded; use its specificName",
                request.object_name
            ),
        ));
    }
    let row = &rows[0];
    Ok(ResolvedRoutine {
        oid: row.try_get(0).map_err(postgres_query_error)?,
        name: row.try_get(1).map_err(postgres_query_error)?,
        remarks: row.try_get(2).map_err(postgres_query_error)?,
        definition: row.try_get(3).map_err(postgres_query_error)?,
        identity_arguments: row.try_get(4).map_err(postgres_query_error)?,
    })
}

async fn routine_parameter_rows(
    application: &Application,
    request: &MetadataObjectRef,
    oid: u32,
    include_return: bool,
) -> Result<Vec<Row>, AppError> {
    let return_filter = if include_return { "" } else { "WHERE false" };
    let sql = format!(
        "WITH target AS (SELECT * FROM pg_proc WHERE oid = $1), \
         arguments AS ( \
             SELECT ordinality::int4 AS ordinal, \
                    COALESCE(p.proargnames[ordinality], '') AS name, \
                    COALESCE(p.proargmodes[ordinality], 'i')::text AS mode, \
                    format_type(CASE WHEN p.proallargtypes IS NULL \
                                     THEN p.proargtypes[ordinality - 1] \
                                     ELSE p.proallargtypes[ordinality] END, NULL) AS type_name \
             FROM target p, LATERAL generate_series( \
                 1, COALESCE(array_length(p.proallargtypes, 1), p.pronargs) \
             ) AS ordinality \
         ) \
         SELECT 0::int4, ''::text, 'r'::text, format_type(prorettype, NULL) FROM target {return_filter} \
         UNION ALL SELECT ordinal, name, mode, type_name FROM arguments \
         ORDER BY 1"
    );
    metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        &sql,
        &[&oid],
    )
    .await
}

fn postgres_function_column_type(mode: &str, ordinal: i32) -> i32 {
    if ordinal == 0 || mode == "r" {
        return 4;
    }
    match mode {
        "i" | "v" => 1,
        "b" => 2,
        "o" | "t" => 3,
        _ => 0,
    }
}

fn postgres_procedure_column_type(mode: &str) -> i32 {
    match mode {
        "i" | "v" => 1,
        "b" => 2,
        "o" | "t" => 4,
        _ => 0,
    }
}

async fn list_triggers(
    application: &Application,
    request: &ListTriggersRequest,
) -> Result<TriggerList, AppError> {
    validate_metadata_scope(&request.scope.database_name, &request.scope.schema_name)?;
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        "SELECT current_database(), n.nspname, t.tgname, \
                concat_ws(',', \
                    CASE WHEN (t.tgtype & 4) <> 0 THEN 'INSERT' END, \
                    CASE WHEN (t.tgtype & 8) <> 0 THEN 'DELETE' END, \
                    CASE WHEN (t.tgtype & 16) <> 0 THEN 'UPDATE' END, \
                    CASE WHEN (t.tgtype & 32) <> 0 THEN 'TRUNCATE' END), \
                pg_get_triggerdef(t.oid, true) \
         FROM pg_trigger t \
         JOIN pg_class c ON c.oid = t.tgrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE NOT t.tgisinternal AND n.nspname = $1 \
         ORDER BY t.tgname",
        &[&request.scope.schema_name],
    )
    .await?;
    Ok(TriggerList {
        items: rows
            .iter()
            .map(postgres_trigger_metadata)
            .collect::<Result<_, _>>()?,
    })
}

async fn get_trigger(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<TriggerMetadata, AppError> {
    validate_metadata_scope(&request.scope.database_name, &request.scope.schema_name)?;
    validate_identifier(&request.object_name, "triggerName")?;
    let mut rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        "SELECT current_database(), n.nspname, t.tgname, \
                concat_ws(',', \
                    CASE WHEN (t.tgtype & 4) <> 0 THEN 'INSERT' END, \
                    CASE WHEN (t.tgtype & 8) <> 0 THEN 'DELETE' END, \
                    CASE WHEN (t.tgtype & 16) <> 0 THEN 'UPDATE' END, \
                    CASE WHEN (t.tgtype & 32) <> 0 THEN 'TRUNCATE' END), \
                pg_get_triggerdef(t.oid, true) \
         FROM pg_trigger t \
         JOIN pg_class c ON c.oid = t.tgrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE NOT t.tgisinternal AND n.nspname = $1 AND t.tgname = $2",
        &[&request.scope.schema_name, &request.object_name],
    )
    .await?;
    let row = rows.pop().ok_or_else(|| {
        metadata_not_found(
            "trigger",
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
    })?;
    postgres_trigger_metadata(&row)
}

fn postgres_trigger_metadata(row: &Row) -> Result<TriggerMetadata, AppError> {
    Ok(TriggerMetadata {
        database_name: row.try_get(0).map_err(postgres_query_error)?,
        schema_name: row.try_get(1).map_err(postgres_query_error)?,
        name: row.try_get(2).map_err(postgres_query_error)?,
        event_manipulation: row.try_get(3).map_err(postgres_query_error)?,
        body: row.try_get(4).map_err(postgres_query_error)?,
    })
}

async fn load_er_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<Vec<EntityRelationTable>, AppError> {
    validate_metadata_scope(database_name, schema_name)?;
    let tables = list_tables(application, datasource_id, database_name, schema_name, "").await?;
    let column_rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT c.relname, a.attname, format_type(a.atttypid, a.atttypmod), \
                EXISTS (SELECT 1 FROM pg_constraint pk \
                        WHERE pk.conrelid = c.oid AND pk.contype = 'p' \
                          AND a.attnum = ANY(pk.conkey)), \
                COALESCE(col_description(c.oid, a.attnum), '') \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f') \
         ORDER BY c.relname, a.attnum",
        &[&schema_name],
    )
    .await?;
    let foreign_rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT pt.relname, pa.attname, ft.relname, fa.attname \
         FROM pg_constraint con \
         JOIN pg_class ft ON ft.oid = con.conrelid \
         JOIN pg_namespace fn ON fn.oid = ft.relnamespace \
         JOIN pg_class pt ON pt.oid = con.confrelid \
         JOIN pg_namespace pn ON pn.oid = pt.relnamespace \
         JOIN LATERAL generate_subscripts(con.conkey, 1) pos(n) ON true \
         JOIN pg_attribute fa ON fa.attrelid = ft.oid AND fa.attnum = con.conkey[pos.n] \
         JOIN pg_attribute pa ON pa.attrelid = pt.oid AND pa.attnum = con.confkey[pos.n] \
         WHERE con.contype = 'f' AND fn.nspname = $1 AND pn.nspname = $1 \
         ORDER BY ft.relname, con.conname, pos.n",
        &[&schema_name],
    )
    .await?;

    let mut result = tables
        .items
        .into_iter()
        .map(|table| EntityRelationTable {
            name: table.name,
            comment: table.comment,
            columns: Vec::new(),
            foreign_keys: Vec::new(),
        })
        .collect::<Vec<_>>();
    let indexes = result
        .iter()
        .enumerate()
        .map(|(index, table)| (table.name.clone(), index))
        .collect::<HashMap<_, _>>();
    for row in column_rows {
        let table_name: String = row.try_get(0).map_err(postgres_query_error)?;
        if let Some(index) = indexes.get(&table_name) {
            result[*index].columns.push(EntityRelationColumn {
                name: row.try_get(1).map_err(postgres_query_error)?,
                column_type: row.try_get(2).map_err(postgres_query_error)?,
                primary_key: row.try_get(3).map_err(postgres_query_error)?,
                comment: row.try_get(4).map_err(postgres_query_error)?,
            });
        }
    }
    for row in foreign_rows {
        let foreign_table: String = row.try_get(2).map_err(postgres_query_error)?;
        if let Some(index) = indexes.get(&foreign_table) {
            result[*index].foreign_keys.push(EntityRelationForeignKey {
                primary_table: row.try_get(0).map_err(postgres_query_error)?,
                primary_column: row.try_get(1).map_err(postgres_query_error)?,
                foreign_table,
                foreign_column: row.try_get(3).map_err(postgres_query_error)?,
            });
        }
    }
    Ok(result)
}

async fn table_ddl(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    validate_metadata_table(database_name, schema_name, table_name)?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let connection = open_resolved_connection(&resolved, Some(database_name)).await?;
    let result = build_table_ddl(&connection, database_name, schema_name, table_name).await;
    finish_connection(connection, result).await
}

#[allow(
    clippy::too_many_lines,
    reason = "table DDL reconstruction intentionally assembles all PostgreSQL table clauses in catalog order"
)]
async fn build_table_ddl(
    connection: &ManagedPostgresConnection,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    let header_rows = metadata_query_on(
        connection,
        "SELECT c.relkind::text, c.relpersistence::text, \
                COALESCE(obj_description(c.oid, 'pg_class'), ''), \
                COALESCE(ts.spcname, ''), COALESCE(pg_get_partkeydef(c.oid), ''), \
                COALESCE(fs.srvname, ''), \
                COALESCE(array_to_string(ft.ftoptions, E'\\n'), '') \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace \
         LEFT JOIN pg_foreign_table ft ON ft.ftrelid = c.oid \
         LEFT JOIN pg_foreign_server fs ON fs.oid = ft.ftserver \
         WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p', 'f')",
        &[&schema_name, &table_name],
    )
    .await?;
    let header = header_rows
        .first()
        .ok_or_else(|| metadata_not_found("table", database_name, schema_name, table_name))?;
    let kind: String = header.try_get(0).map_err(postgres_query_error)?;
    let persistence: String = header.try_get(1).map_err(postgres_query_error)?;
    let table_comment: String = header.try_get(2).map_err(postgres_query_error)?;
    let tablespace: String = header.try_get(3).map_err(postgres_query_error)?;
    let partition_key: String = header.try_get(4).map_err(postgres_query_error)?;
    let foreign_server: String = header.try_get(5).map_err(postgres_query_error)?;
    let foreign_options: String = header.try_get(6).map_err(postgres_query_error)?;
    let columns = metadata_query_on(
        connection,
        "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, \
                pg_get_expr(ad.adbin, ad.adrelid), a.attidentity::text, \
                a.attgenerated::text, COALESCE(coll_ns.nspname, ''), \
                COALESCE(coll.collname, ''), COALESCE(col_description(c.oid, a.attnum), '') \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         LEFT JOIN pg_attrdef ad ON ad.adrelid = c.oid AND ad.adnum = a.attnum \
         LEFT JOIN pg_collation coll ON coll.oid = a.attcollation AND a.attcollation <> 0 \
         LEFT JOIN pg_namespace coll_ns ON coll_ns.oid = coll.collnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 ORDER BY a.attnum",
        &[&schema_name, &table_name],
    )
    .await?;
    let constraints = metadata_query_on(
        connection,
        "SELECT con.conname, pg_get_constraintdef(con.oid, true) \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 \
         ORDER BY CASE con.contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 \
                  WHEN 'f' THEN 2 WHEN 'c' THEN 3 ELSE 4 END, con.conname",
        &[&schema_name, &table_name],
    )
    .await?;
    let indexes = metadata_query_on(
        connection,
        "SELECT pg_get_indexdef(i.indexrelid) \
         FROM pg_index i \
         JOIN pg_class c ON c.oid = i.indrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_constraint con ON con.conindid = i.indexrelid \
         WHERE n.nspname = $1 AND c.relname = $2 AND con.oid IS NULL \
         ORDER BY i.indexrelid::regclass::text",
        &[&schema_name, &table_name],
    )
    .await?;

    let qualified = format!(
        "{}.{}",
        quote_identifier(schema_name, "schemaName")?,
        quote_identifier(table_name, "tableName")?
    );
    let mut ddl = String::new();
    let prefix = match kind.as_str() {
        "f" => "CREATE FOREIGN TABLE",
        _ if persistence == "u" => "CREATE UNLOGGED TABLE",
        _ => "CREATE TABLE",
    };
    writeln!(&mut ddl, "{prefix} {qualified} (").map_err(|_| AppError::internal())?;
    let mut definitions = Vec::new();
    let mut column_comments = Vec::new();
    for row in columns {
        let name: String = row.try_get(0).map_err(postgres_query_error)?;
        let data_type: String = row.try_get(1).map_err(postgres_query_error)?;
        let not_null: bool = row.try_get(2).map_err(postgres_query_error)?;
        let default_value: Option<String> = row.try_get(3).map_err(postgres_query_error)?;
        let identity: String = row.try_get(4).map_err(postgres_query_error)?;
        let generated: String = row.try_get(5).map_err(postgres_query_error)?;
        let collation_schema: String = row.try_get(6).map_err(postgres_query_error)?;
        let collation_name: String = row.try_get(7).map_err(postgres_query_error)?;
        let comment: String = row.try_get(8).map_err(postgres_query_error)?;
        let mut definition = format!("    {} {data_type}", quote_identifier(&name, "columnName")?);
        if !collation_name.is_empty() {
            write!(
                &mut definition,
                " COLLATE {}.{}",
                quote_identifier(&collation_schema, "collationSchema")?,
                quote_identifier(&collation_name, "collationName")?
            )
            .map_err(|_| AppError::internal())?;
        }
        if generated == "s" {
            if let Some(expression) = default_value.as_deref() {
                write!(
                    &mut definition,
                    " GENERATED ALWAYS AS ({expression}) STORED"
                )
                .map_err(|_| AppError::internal())?;
            }
        } else if !identity.is_empty() {
            write!(
                &mut definition,
                " GENERATED {} AS IDENTITY",
                if identity == "a" {
                    "ALWAYS"
                } else {
                    "BY DEFAULT"
                }
            )
            .map_err(|_| AppError::internal())?;
        } else if let Some(default_value) = default_value {
            write!(&mut definition, " DEFAULT {default_value}")
                .map_err(|_| AppError::internal())?;
        }
        if not_null {
            definition.push_str(" NOT NULL");
        }
        definitions.push(definition);
        if !comment.is_empty() {
            column_comments.push((name, comment));
        }
    }
    for row in constraints {
        let name: String = row.try_get(0).map_err(postgres_query_error)?;
        let definition: String = row.try_get(1).map_err(postgres_query_error)?;
        definitions.push(format!(
            "    CONSTRAINT {} {definition}",
            quote_identifier(&name, "constraintName")?
        ));
    }
    ddl.push_str(&definitions.join(",\n"));
    ddl.push_str("\n)");
    if kind == "p" && !partition_key.is_empty() {
        write!(&mut ddl, " PARTITION BY {partition_key}").map_err(|_| AppError::internal())?;
    }
    if kind == "f" {
        write!(
            &mut ddl,
            " SERVER {}",
            quote_identifier(&foreign_server, "foreignServer")?
        )
        .map_err(|_| AppError::internal())?;
        let options = render_foreign_options(&foreign_options)?;
        if !options.is_empty() {
            write!(&mut ddl, " OPTIONS ({options})").map_err(|_| AppError::internal())?;
        }
    }
    if !tablespace.is_empty() && kind != "f" {
        write!(
            &mut ddl,
            " TABLESPACE {}",
            quote_identifier(&tablespace, "tablespace")?
        )
        .map_err(|_| AppError::internal())?;
    }
    ddl.push_str(";\n");
    for row in indexes {
        let definition: String = row.try_get(0).map_err(postgres_query_error)?;
        writeln!(&mut ddl, "{};", definition.trim_end_matches(';'))
            .map_err(|_| AppError::internal())?;
    }
    if !table_comment.is_empty() {
        writeln!(
            &mut ddl,
            "COMMENT ON TABLE {qualified} IS {};",
            quote_literal(&table_comment)?
        )
        .map_err(|_| AppError::internal())?;
    }
    for (name, comment) in column_comments {
        writeln!(
            &mut ddl,
            "COMMENT ON COLUMN {qualified}.{} IS {};",
            quote_identifier(&name, "columnName")?,
            quote_literal(&comment)?
        )
        .map_err(|_| AppError::internal())?;
    }
    Ok(ddl.trim_end().to_owned())
}

async fn metadata_query_on(
    connection: &ManagedPostgresConnection,
    sql: &str,
    parameters: &[&(dyn ToSql + Sync)],
) -> Result<Vec<Row>, AppError> {
    postgres_timeout(
        METADATA_TIMEOUT,
        "postgres_metadata_timeout",
        "The PostgreSQL metadata query did not finish in time",
        connection.client().query(sql, parameters),
    )
    .await
}

fn render_foreign_options(options: &str) -> Result<String, AppError> {
    options
        .lines()
        .filter(|line| !line.is_empty())
        .map(|option| {
            let (name, value) = option.split_once('=').ok_or_else(AppError::internal)?;
            Ok(format!(
                "{} {}",
                quote_identifier(name, "foreignOption")?,
                quote_literal(value)?
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()
        .map(|values| values.join(", "))
}

fn quote_literal(value: &str) -> Result<String, AppError> {
    if value.len() > MAX_SCALAR_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_postgres_literal",
            "The PostgreSQL literal is invalid",
        ));
    }
    let escaped_length = value.chars().try_fold(0_usize, |length, character| {
        length.checked_add(match character {
            '\\' | '\'' => 2,
            _ => character.len_utf8(),
        })
    });
    let Some(escaped_length) = escaped_length
        .and_then(|length| length.checked_add(3))
        .filter(|length| *length <= MAX_SCALAR_BYTES)
    else {
        return Err(AppError::invalid(
            "invalid_postgres_literal",
            "The escaped PostgreSQL literal exceeds the scalar byte limit",
        ));
    };
    let mut escaped = String::with_capacity(escaped_length);
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("''"),
            _ => escaped.push(character),
        }
    }
    Ok(format!("E'{escaped}'"))
}

async fn start_table_preview(
    application: &Application,
    request: TablePreviewRequest,
    row_limit: u32,
) -> Result<TablePreviewAccepted, AppError> {
    if row_limit == 0 || row_limit > MAX_CONSOLE_PAGE_SIZE {
        return Err(AppError::invalid(
            "invalid_table_preview_request",
            format!("rowLimit must be between 1 and {MAX_CONSOLE_PAGE_SIZE}"),
        ));
    }
    validate_metadata_table(
        &request.table.scope.database_name,
        &request.table.scope.schema_name,
        &request.table.table_name,
    )?;
    let sql = format!(
        "SELECT * FROM {}.{} LIMIT {row_limit}",
        quote_identifier(&request.table.scope.schema_name, "schemaName")?,
        quote_identifier(&request.table.table_name, "tableName")?
    );
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

fn build_create_schema(request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
    let schema = request.schema;
    let name = quote_identifier(&schema.name, "schemaName")?;
    let mut sql = format!("CREATE SCHEMA {name}");
    if !schema.owner.trim().is_empty() {
        write!(
            &mut sql,
            " AUTHORIZATION {}",
            quote_identifier(&schema.owner, "owner")?
        )
        .map_err(|_| AppError::internal())?;
    }
    sql.push(';');
    if !schema.comment.is_empty() {
        write!(
            &mut sql,
            "\nCOMMENT ON SCHEMA {name} IS {};",
            quote_literal(&schema.comment)?
        )
        .map_err(|_| AppError::internal())?;
    }
    Ok(BuiltSql { sql })
}

fn build_namespace_sql(request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
    let sql = match request.operation {
        NamespaceSqlOperation::CreateDatabase { database } => build_create_database(&database)?,
        NamespaceSqlOperation::AlterDatabase {
            old_database,
            new_database,
        } => build_alter_database(&old_database, &new_database)?,
        NamespaceSqlOperation::DropDatabase { database_name } => format!(
            "DROP DATABASE {};",
            quote_identifier(&database_name, "databaseName")?
        ),
        NamespaceSqlOperation::UseDatabase { .. } => {
            return Err(AppError::invalid(
                "postgres_database_switch_unsupported",
                "PostgreSQL selects a database when opening the connection and cannot switch it with SQL",
            ));
        }
        NamespaceSqlOperation::CreateSchema { schema } => {
            return build_create_schema(CreateSchemaSqlRequest { schema });
        }
        NamespaceSqlOperation::AlterSchema {
            old_schema_name,
            new_schema_name,
        } => format!(
            "ALTER SCHEMA {} RENAME TO {};",
            quote_identifier(&old_schema_name, "schemaName")?,
            quote_identifier(&new_schema_name, "schemaName")?
        ),
        NamespaceSqlOperation::DropSchema { schema_name } => format!(
            "DROP SCHEMA {};",
            quote_identifier(&schema_name, "schemaName")?
        ),
    };
    Ok(BuiltSql { sql })
}

fn build_create_database(
    database: &crate::native_driver_types::DatabaseDefinition,
) -> Result<String, AppError> {
    let name = quote_identifier(&database.name, "databaseName")?;
    let mut sql = format!("CREATE DATABASE {name}");
    if !database.owner.trim().is_empty() {
        write!(
            &mut sql,
            " OWNER {}",
            quote_identifier(&database.owner, "owner")?
        )
        .map_err(|_| AppError::internal())?;
    }
    if !database.charset.trim().is_empty() {
        write!(&mut sql, " ENCODING {}", quote_literal(&database.charset)?)
            .map_err(|_| AppError::internal())?;
    }
    if !database.collation.trim().is_empty() {
        write!(
            &mut sql,
            " LC_COLLATE {} LC_CTYPE {}",
            quote_literal(&database.collation)?,
            quote_literal(&database.collation)?
        )
        .map_err(|_| AppError::internal())?;
    }
    sql.push(';');
    if !database.comment.is_empty() {
        write!(
            &mut sql,
            "\nCOMMENT ON DATABASE {name} IS {};",
            quote_literal(&database.comment)?
        )
        .map_err(|_| AppError::internal())?;
    }
    Ok(sql)
}

fn build_alter_database(
    old_database: &crate::native_driver_types::DatabaseDefinition,
    new_database: &crate::native_driver_types::DatabaseDefinition,
) -> Result<String, AppError> {
    if old_database.charset != new_database.charset
        || old_database.collation != new_database.collation
    {
        return Err(AppError::invalid(
            "postgres_database_alter_unsupported",
            "PostgreSQL cannot alter a database encoding or collation in place",
        ));
    }
    let old_name = quote_identifier(&old_database.name, "databaseName")?;
    let new_name = quote_identifier(&new_database.name, "databaseName")?;
    let mut statements = Vec::new();
    if old_database.name != new_database.name {
        statements.push(format!("ALTER DATABASE {old_name} RENAME TO {new_name};"));
    }
    let active_name = if old_database.name == new_database.name {
        old_name
    } else {
        new_name
    };
    if old_database.owner != new_database.owner && !new_database.owner.trim().is_empty() {
        statements.push(format!(
            "ALTER DATABASE {active_name} OWNER TO {};",
            quote_identifier(&new_database.owner, "owner")?
        ));
    }
    if old_database.comment != new_database.comment {
        statements.push(format!(
            "COMMENT ON DATABASE {active_name} IS {};",
            quote_literal(&new_database.comment)?
        ));
    }
    if statements.is_empty() {
        return Err(AppError::invalid(
            "postgres_database_alter_empty",
            "The PostgreSQL database definition has no supported changes",
        ));
    }
    Ok(statements.join("\n"))
}

fn build_dml(request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
    let target = postgres_dml_target(&request.target)?;
    let sql = match request.statement {
        DmlStatement::SingleInsert { columns, row } => {
            postgres_insert_sql(&target, &columns, std::slice::from_ref(&row))?
        }
        DmlStatement::MultiInsert { columns, rows } => {
            postgres_insert_sql(&target, &columns, &rows)?
        }
        DmlStatement::Update {
            assignments,
            predicates,
        } => postgres_update_sql(&target, &assignments, &predicates)?,
    };
    Ok(BuiltSql { sql })
}

fn postgres_dml_target(target: &DmlTarget) -> Result<String, AppError> {
    let table = quote_identifier(&target.table_name, "tableName")?;
    match target
        .schema_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(schema) => Ok(format!(
            "{}.{}",
            quote_identifier(schema, "schemaName")?,
            table
        )),
        None => Ok(table),
    }
}

fn postgres_insert_sql(
    target: &str,
    columns: &[DmlColumn],
    rows: &[DmlRow],
) -> Result<String, AppError> {
    if columns.is_empty() || rows.is_empty() {
        return Err(AppError::invalid(
            "invalid_postgres_dml",
            "PostgreSQL INSERT requires at least one column and row",
        ));
    }
    let column_sql = columns
        .iter()
        .map(|column| quote_identifier(&column.name, "columnName"))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let value_rows = rows
        .iter()
        .map(|row| {
            if row.values.len() != columns.len() {
                return Err(AppError::invalid(
                    "invalid_postgres_dml",
                    "Each PostgreSQL INSERT row must match the selected column count",
                ));
            }
            row.values
                .iter()
                .zip(columns)
                .map(|(value, column)| postgres_dml_value(value, column))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| format!("({})", values.join(", ")))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(",\n");
    Ok(format!(
        "INSERT INTO {target} ({column_sql}) VALUES\n{value_rows};"
    ))
}

fn postgres_update_sql(
    target: &str,
    assignments: &[DmlAssignment],
    predicates: &[DmlAssignment],
) -> Result<String, AppError> {
    if assignments.is_empty() || predicates.is_empty() {
        return Err(AppError::invalid(
            "invalid_postgres_dml",
            "PostgreSQL UPDATE requires assignments and key predicates",
        ));
    }
    let assignments = assignments
        .iter()
        .map(|assignment| {
            Ok(format!(
                "{} = {}",
                quote_identifier(&assignment.column.name, "columnName")?,
                postgres_dml_value(&assignment.value, &assignment.column)?
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");
    let predicates = predicates
        .iter()
        .map(|predicate| {
            let column = quote_identifier(&predicate.column.name, "columnName")?;
            match predicate.value {
                DmlValue::Null => Ok(format!("{column} IS NULL")),
                _ => Ok(format!(
                    "{column} = {}",
                    postgres_dml_value(&predicate.value, &predicate.column)?
                )),
            }
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(" AND ");
    Ok(format!(
        "UPDATE {target} SET {assignments} WHERE {predicates};"
    ))
}

fn postgres_dml_value(value: &DmlValue, column: &DmlColumn) -> Result<String, AppError> {
    match value {
        DmlValue::Null => Ok("NULL".to_owned()),
        DmlValue::String(value) => quote_literal(value),
        DmlValue::Decimal(value) => {
            validate_decimal(value)?;
            Ok(value.clone())
        }
        DmlValue::Boolean(value) => Ok(if *value { "TRUE" } else { "FALSE" }.to_owned()),
        DmlValue::Temporal { kind, iso8601 } => {
            validate_temporal(*kind, iso8601)?;
            let prefix = match kind {
                DmlTemporalKind::Date => "DATE",
                DmlTemporalKind::Time => "TIME",
                DmlTemporalKind::LocalDatetime => "TIMESTAMP",
                DmlTemporalKind::OffsetDatetime => "TIMESTAMPTZ",
            };
            Ok(format!("{prefix} {}", quote_literal(iso8601)?))
        }
        DmlValue::Binary(value) => {
            if value.len() > MAX_SCALAR_BYTES {
                return Err(AppError::invalid(
                    "invalid_postgres_dml",
                    "The PostgreSQL binary value exceeds the scalar limit",
                ));
            }
            Ok(format!("decode('{}', 'hex')", hex::encode(value)))
        }
    }
    .map(|sql| {
        if matches!(value, DmlValue::String(_))
            && column.data_type_name.eq_ignore_ascii_case("uuid")
        {
            format!("{sql}::uuid")
        } else {
            sql
        }
    })
}

fn is_native_read_candidate(sql: &str) -> Result<bool, AppError> {
    let words = postgres_words(sql)?;
    Ok(matches!(
        words.first().map(String::as_str),
        Some("SELECT" | "WITH" | "VALUES" | "TABLE" | "EXPLAIN")
    ))
}

fn validate_query(query: &PreparedQuery) -> Result<(), AppError> {
    if query.sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid(
            "invalid_query_request",
            format!("SQL cannot exceed {MAX_SQL_BYTES} UTF-8 bytes"),
        ));
    }
    validate_read_sql(&query.sql)?;
    let _ = postgres_query_parameters(&query.parameters)?;
    validate_query_options(query.options)
}

fn validate_read_sql(sql: &str) -> Result<(), AppError> {
    let statements = split_postgres_script(sql)?;
    if statements.len() != 1 {
        return Err(AppError::invalid(
            "postgres_native_query_unsupported",
            "Native PostgreSQL accepts exactly one read statement",
        ));
    }
    let words = postgres_words(&statements[0])?;
    if !matches!(
        words.first().map(String::as_str),
        Some("SELECT" | "WITH" | "VALUES" | "TABLE" | "EXPLAIN")
    ) {
        return Err(AppError::invalid(
            "postgres_native_query_unsupported",
            "Native PostgreSQL supports SELECT, WITH, VALUES, TABLE, and EXPLAIN read statements",
        ));
    }
    let forbidden = words.iter().any(|word| {
        matches!(
            word.as_str(),
            "INSERT"
                | "UPDATE"
                | "DELETE"
                | "MERGE"
                | "CREATE"
                | "ALTER"
                | "DROP"
                | "TRUNCATE"
                | "COPY"
                | "CALL"
                | "DO"
                | "GRANT"
                | "REVOKE"
                | "LOCK"
        )
    }) || words.windows(2).any(|words| {
        matches!(
            words,
            [first, second]
                if (first == "FOR" && matches!(second.as_str(), "UPDATE" | "SHARE"))
                    || (first == "SELECT" && second == "INTO")
        )
    }) || words.windows(3).any(|words| {
        matches!(
            words,
            [first, second, third]
                if first == "FOR"
                    && ((second == "KEY" && third == "SHARE")
                        || (second == "NO" && third == "KEY"))
        )
    });
    if forbidden {
        return Err(AppError::invalid(
            "postgres_native_query_unsupported",
            "Native PostgreSQL read queries must not write data, create objects, or lock rows",
        ));
    }
    Parser::parse_sql(&PostgreSqlDialect {}, &statements[0]).map_err(|_| {
        AppError::invalid(
            "postgres_native_query_unsupported",
            "Native PostgreSQL requires one valid read statement",
        )
    })?;
    Ok(())
}

fn validate_query_options(options: QueryExecutionOptions) -> Result<(), AppError> {
    if options.target_batch_rows > MAX_BATCH_ROWS {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("batchRows cannot exceed {MAX_BATCH_ROWS}"),
        ));
    }
    if options.target_batch_bytes != 0
        && !(1024..=MAX_BATCH_BYTES).contains(&options.target_batch_bytes)
    {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("batchBytes must be zero or between 1024 and {MAX_BATCH_BYTES}"),
        ));
    }
    if options.max_result_bytes > MAX_RESULT_BYTES {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("maxResultBytes cannot exceed {MAX_RESULT_BYTES}"),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct PostgresParameter(Option<String>);

impl ToSql for PostgresParameter {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match &self.0 {
            Some(value) => {
                out.extend_from_slice(value.as_bytes());
                Ok(IsNull::No)
            }
            None => Ok(IsNull::Yes),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Text
    }

    tokio_postgres::types::to_sql_checked!();
}

fn postgres_query_parameters(
    parameters: &[QueryParameter],
) -> Result<Vec<PostgresParameter>, AppError> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(AppError::invalid(
            "invalid_query_parameter_count",
            format!("PostgreSQL queries accept at most {MAX_PARAMETERS} parameters"),
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
                    "PostgreSQL parameter positions must be unique and contiguous from 1",
                ));
            }
            postgres_query_parameter(&parameter.value)
        })
        .collect()
}

fn postgres_query_parameter(value: &DatabaseValue) -> Result<PostgresParameter, AppError> {
    let text = match value {
        DatabaseValue::Null => None,
        DatabaseValue::Boolean(value) => Some(if *value { "true" } else { "false" }.to_owned()),
        DatabaseValue::SignedInteger(value) => Some(value.to_string()),
        DatabaseValue::UnsignedInteger(value) => Some(value.to_string()),
        DatabaseValue::Float32(value) => Some(value.to_string()),
        DatabaseValue::Float64(value) => Some(value.to_string()),
        DatabaseValue::Decimal(value) => {
            validate_decimal(value)?;
            Some(value.clone())
        }
        DatabaseValue::Text(value) => Some(validate_parameter_text(value, "text")?),
        DatabaseValue::Binary(value) => {
            if value.len() > MAX_SCALAR_BYTES {
                return Err(invalid_parameter("binary"));
            }
            Some(format!("\\x{}", hex::encode(value)))
        }
        DatabaseValue::Date(value) => {
            NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| invalid_parameter("date"))?;
            Some(validate_parameter_text(value, "date")?)
        }
        DatabaseValue::Time(value) => {
            parse_postgres_time(value)?;
            Some(validate_parameter_text(value, "time")?)
        }
        DatabaseValue::Timestamp(value) => {
            parse_postgres_timestamp(value)?;
            Some(validate_parameter_text(value, "timestamp")?)
        }
        DatabaseValue::TimestampWithTimeZone(value) => {
            DateTime::parse_from_rfc3339(value)
                .map_err(|_| invalid_parameter("timestamp with time zone"))?;
            Some(validate_parameter_text(value, "timestamp with time zone")?)
        }
        DatabaseValue::Json(value) => {
            serde_json::from_str::<serde_json::Value>(value)
                .map_err(|_| invalid_parameter("JSON"))?;
            Some(validate_parameter_text(value, "JSON")?)
        }
        DatabaseValue::Uuid(value) => {
            uuid::Uuid::parse_str(value).map_err(|_| invalid_parameter("UUID"))?;
            Some(validate_parameter_text(value, "UUID")?)
        }
    };
    Ok(PostgresParameter(text))
}

fn validate_parameter_text(value: &str, label: &str) -> Result<String, AppError> {
    if value.len() > MAX_SCALAR_BYTES || value.contains('\0') {
        return Err(invalid_parameter(label));
    }
    Ok(value.to_owned())
}

fn validate_decimal(value: &str) -> Result<(), AppError> {
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, None), |(mantissa, exponent)| {
            (mantissa, Some(exponent))
        });
    let mut digits = 0_usize;
    let mut points = 0_u8;
    for byte in mantissa.bytes() {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' {
            points += 1;
        } else {
            return Err(invalid_parameter("decimal"));
        }
    }
    if digits == 0 || points > 1 {
        return Err(invalid_parameter("decimal"));
    }
    if let Some(exponent) = exponent {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !exponent.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_parameter("decimal"));
        }
    }
    Ok(())
}

fn validate_temporal(kind: DmlTemporalKind, value: &str) -> Result<(), AppError> {
    match kind {
        DmlTemporalKind::Date => {
            NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| invalid_parameter("date"))?;
        }
        DmlTemporalKind::Time => {
            parse_postgres_time(value)?;
        }
        DmlTemporalKind::LocalDatetime => {
            parse_postgres_timestamp(value)?;
        }
        DmlTemporalKind::OffsetDatetime => {
            DateTime::parse_from_rfc3339(value)
                .map_err(|_| invalid_parameter("timestamp with time zone"))?;
        }
    }
    Ok(())
}

fn parse_postgres_time(value: &str) -> Result<NaiveTime, AppError> {
    ["%H:%M:%S%.f", "%H:%M:%S"]
        .into_iter()
        .find_map(|format| NaiveTime::parse_from_str(value, format).ok())
        .ok_or_else(|| invalid_parameter("time"))
}

fn parse_postgres_timestamp(value: &str) -> Result<NaiveDateTime, AppError> {
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| invalid_parameter("timestamp"))
}

fn invalid_parameter(label: &str) -> AppError {
    AppError::invalid(
        "invalid_query_parameter",
        format!("The PostgreSQL {label} parameter is invalid"),
    )
}

#[derive(Debug)]
enum RawPostgresValue {
    Bytes(Vec<u8>),
    TooLarge { byte_count: usize },
}

impl<'a> FromSql<'a> for RawPostgresValue {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        if raw.len() > MAX_SCALAR_BYTES {
            Ok(Self::TooLarge {
                byte_count: raw.len(),
            })
        } else {
            Ok(Self::Bytes(raw.to_vec()))
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

fn postgres_column(
    index: usize,
    column: &tokio_postgres::Column,
) -> Result<wire::JdbcColumn, AppError> {
    let ordinal = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(AppError::internal)?;
    let data_type = postgres_base_type(column.type_());
    let value_type = postgres_value_type(data_type);
    Ok(wire::JdbcColumn {
        ordinal,
        label: column.name().to_owned(),
        name: column.name().to_owned(),
        jdbc_type: postgres_jdbc_type(data_type),
        jdbc_type_name: column.type_().name().to_ascii_uppercase(),
        value_type: value_type as i32,
        nullability: wire::ColumnNullability::Unknown as i32,
        precision: postgres_type_precision(data_type),
        scale: postgres_type_scale(data_type),
        display_size: postgres_type_display_size(data_type),
        signed: postgres_type_signed(data_type),
        catalog_name: None,
        schema_name: None,
        table_name: None,
    })
}

fn postgres_row(row: &Row, columns: &[tokio_postgres::Column]) -> Result<wire::JdbcRow, AppError> {
    if row.len() != columns.len() {
        return Err(AppError::internal());
    }
    Ok(wire::JdbcRow {
        values: columns
            .iter()
            .enumerate()
            .map(|(index, column)| postgres_wire_value(row, index, column.type_()))
            .collect::<Result<_, _>>()?,
    })
}

fn postgres_wire_value(
    row: &Row,
    index: usize,
    data_type: &Type,
) -> Result<wire::JdbcValue, AppError> {
    use wire::jdbc_value::Value;
    let raw_value = row
        .try_get::<_, Option<RawPostgresValue>>(index)
        .map_err(postgres_query_error)?;
    let value = match raw_value {
        None => Value::NullValue(wire::JdbcNull {}),
        Some(RawPostgresValue::Bytes(bytes)) => decode_postgres_value(data_type, &bytes)?,
        Some(RawPostgresValue::TooLarge { byte_count }) => {
            return Err(postgres_scalar_too_large(byte_count));
        }
    };
    Ok(wire::JdbcValue { value: Some(value) })
}

fn decode_postgres_value(
    data_type: &Type,
    raw: &[u8],
) -> Result<wire::jdbc_value::Value, AppError> {
    use wire::jdbc_value::Value;
    ensure_postgres_scalar_size(raw.len())?;
    if let Kind::Domain(inner) = data_type.kind() {
        return decode_postgres_value(inner, raw);
    }
    if matches!(data_type.kind(), Kind::Array(_)) {
        return Ok(Value::OpaqueValue(wire::OpaqueValue {
            type_name: data_type.name().to_owned(),
            display_value: decode_postgres_array(data_type, raw)?,
        }));
    }
    if matches!(data_type.kind(), Kind::Enum(_)) {
        return Ok(Value::TextValue(postgres_utf8(raw)?));
    }
    let value = if *data_type == Type::BOOL {
        Value::BooleanValue(raw.first().copied() == Some(1))
    } else if *data_type == Type::INT2 {
        Value::SignedIntegerValue(i64::from(read_i16(raw)?))
    } else if *data_type == Type::INT4 {
        Value::SignedIntegerValue(i64::from(read_i32(raw)?))
    } else if *data_type == Type::INT8 {
        Value::SignedIntegerValue(read_i64(raw)?)
    } else if postgres_unsigned_type(data_type) {
        Value::UnsignedIntegerValue(u64::from(read_u32(raw)?))
    } else if *data_type == Type::FLOAT4 {
        Value::Float32Value(f32::from_bits(read_u32(raw)?))
    } else if *data_type == Type::FLOAT8 {
        Value::Float64Value(f64::from_bits(read_u64(raw)?))
    } else if *data_type == Type::NUMERIC {
        Value::DecimalValue(decode_postgres_numeric(raw)?)
    } else if *data_type == Type::MONEY {
        Value::OpaqueValue(wire::OpaqueValue {
            type_name: data_type.name().to_owned(),
            display_value: format!("raw_units={}", read_i64(raw)?),
        })
    } else if *data_type == Type::BYTEA {
        Value::BinaryValue(raw.to_vec())
    } else if *data_type == Type::DATE {
        Value::DateValue(decode_postgres_date(raw)?)
    } else if *data_type == Type::TIME {
        Value::TimeValue(decode_postgres_time(raw)?)
    } else if *data_type == Type::TIMESTAMP {
        Value::TimestampValue(decode_postgres_timestamp(raw)?)
    } else if *data_type == Type::TIMESTAMPTZ {
        Value::TimestampWithTimeZoneValue(format!("{}Z", decode_postgres_timestamp(raw)?))
    } else if *data_type == Type::TIMETZ {
        Value::OpaqueValue(wire::OpaqueValue {
            type_name: data_type.name().to_owned(),
            display_value: decode_postgres_timetz(raw)?,
        })
    } else if *data_type == Type::JSON {
        Value::JsonValue(postgres_utf8(raw)?)
    } else if *data_type == Type::JSONB {
        let body = raw.strip_prefix(&[1]).ok_or_else(result_decode_error)?;
        Value::JsonValue(postgres_utf8(body)?)
    } else if *data_type == Type::UUID {
        Value::UuidValue(
            uuid::Uuid::from_slice(raw)
                .map_err(|_| result_decode_error())?
                .to_string(),
        )
    } else if *data_type == Type::BIT || *data_type == Type::VARBIT {
        Value::OpaqueValue(wire::OpaqueValue {
            type_name: data_type.name().to_owned(),
            display_value: decode_postgres_bits(raw)?,
        })
    } else if *data_type == Type::INET || *data_type == Type::CIDR {
        Value::TextValue(decode_postgres_network(data_type, raw)?)
    } else if postgres_text_type(data_type) {
        Value::TextValue(postgres_utf8(raw)?)
    } else {
        postgres_opaque_value(data_type.name(), raw)?
    };
    Ok(value)
}

fn postgres_base_type(data_type: &Type) -> &Type {
    match data_type.kind() {
        Kind::Domain(inner) => postgres_base_type(inner),
        _ => data_type,
    }
}

fn postgres_value_type(data_type: &Type) -> wire::JdbcValueType {
    if matches!(
        data_type.kind(),
        Kind::Array(_) | Kind::Range(_) | Kind::Multirange(_) | Kind::Composite(_)
    ) {
        return wire::JdbcValueType::Opaque;
    }
    if matches!(data_type.kind(), Kind::Enum(_)) {
        return wire::JdbcValueType::Text;
    }
    if *data_type == Type::BOOL {
        wire::JdbcValueType::Boolean
    } else if matches!(*data_type, Type::INT2 | Type::INT4 | Type::INT8) {
        wire::JdbcValueType::SignedInteger
    } else if postgres_unsigned_type(data_type) {
        wire::JdbcValueType::UnsignedInteger
    } else if *data_type == Type::FLOAT4 {
        wire::JdbcValueType::Float32
    } else if *data_type == Type::FLOAT8 {
        wire::JdbcValueType::Float64
    } else if *data_type == Type::NUMERIC {
        wire::JdbcValueType::Decimal
    } else if *data_type == Type::MONEY {
        wire::JdbcValueType::Opaque
    } else if *data_type == Type::BYTEA {
        wire::JdbcValueType::Binary
    } else if *data_type == Type::DATE {
        wire::JdbcValueType::Date
    } else if *data_type == Type::TIME {
        wire::JdbcValueType::Time
    } else if *data_type == Type::TIMESTAMP {
        wire::JdbcValueType::Timestamp
    } else if *data_type == Type::TIMESTAMPTZ {
        wire::JdbcValueType::TimestampWithTimeZone
    } else if *data_type == Type::JSON || *data_type == Type::JSONB {
        wire::JdbcValueType::Json
    } else if *data_type == Type::UUID {
        wire::JdbcValueType::Uuid
    } else if postgres_text_type(data_type) || *data_type == Type::INET || *data_type == Type::CIDR
    {
        wire::JdbcValueType::Text
    } else {
        wire::JdbcValueType::Opaque
    }
}

fn postgres_jdbc_type(data_type: &Type) -> i32 {
    if matches!(data_type.kind(), Kind::Array(_)) {
        return 2_003;
    }
    if *data_type == Type::BOOL {
        16
    } else if *data_type == Type::INT2 {
        5
    } else if *data_type == Type::INT4 {
        4
    } else if *data_type == Type::INT8 {
        -5
    } else if *data_type == Type::FLOAT4 {
        7
    } else if *data_type == Type::FLOAT8 {
        8
    } else if *data_type == Type::NUMERIC {
        2
    } else if *data_type == Type::MONEY {
        1_111
    } else if *data_type == Type::BYTEA {
        -2
    } else if *data_type == Type::DATE {
        91
    } else if *data_type == Type::TIME {
        92
    } else if *data_type == Type::TIMETZ {
        2_013
    } else if *data_type == Type::TIMESTAMP {
        93
    } else if *data_type == Type::TIMESTAMPTZ {
        2_014
    } else if *data_type == Type::CHAR || *data_type == Type::BPCHAR {
        1
    } else if *data_type == Type::VARCHAR {
        12
    } else if *data_type == Type::TEXT || *data_type == Type::JSON || *data_type == Type::JSONB {
        -1
    } else if *data_type == Type::BIT {
        -7
    } else if *data_type == Type::VARBIT {
        -3
    } else {
        1_111
    }
}

fn postgres_jdbc_type_name(type_name: &str) -> i32 {
    let normalized = type_name.trim().to_ascii_lowercase();
    if normalized.starts_with('_')
        || normalized.ends_with("[]")
        || normalized.eq_ignore_ascii_case("array")
    {
        return 2_003;
    }
    let normalized = normalized.trim_start_matches('_').trim_end_matches("[]");
    let mut without_modifiers = String::with_capacity(normalized.len());
    let mut modifier_depth = 0_u32;
    for character in normalized.chars() {
        match character {
            '(' => modifier_depth = modifier_depth.saturating_add(1),
            ')' if modifier_depth > 0 => modifier_depth -= 1,
            _ if modifier_depth == 0 => without_modifiers.push(character),
            _ => {}
        }
    }
    let canonical = without_modifiers
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    match canonical.as_str() {
        "bool" | "boolean" => 16,
        "int2" | "smallint" | "smallserial" => 5,
        "int4" | "integer" | "serial" => 4,
        "int8" | "bigint" | "bigserial" => -5,
        "float4" | "real" => 7,
        "float8" | "double" | "double precision" => 8,
        "numeric" | "decimal" => 2,
        "bytea" => -2,
        "date" => 91,
        "time" | "time without time zone" => 92,
        "timetz" | "time with time zone" => 2_013,
        "timestamp" | "timestamp without time zone" => 93,
        "timestamptz" | "timestamp with time zone" => 2_014,
        "char" | "bpchar" | "character" => 1,
        "varchar" | "name" | "character varying" => 12,
        "text" | "json" | "jsonb" | "xml" => -1,
        "bit" => -7,
        "varbit" | "bit varying" => -3,
        _ => 1_111,
    }
}

fn postgres_unsigned_type(data_type: &Type) -> bool {
    matches!(
        *data_type,
        Type::OID
            | Type::REGPROC
            | Type::REGPROCEDURE
            | Type::REGOPER
            | Type::REGOPERATOR
            | Type::REGCLASS
            | Type::REGTYPE
            | Type::REGROLE
            | Type::REGNAMESPACE
    )
}

fn postgres_text_type(data_type: &Type) -> bool {
    matches!(
        *data_type,
        Type::CHAR
            | Type::NAME
            | Type::TEXT
            | Type::BPCHAR
            | Type::VARCHAR
            | Type::UNKNOWN
            | Type::XML
    )
}

fn postgres_type_precision(data_type: &Type) -> Option<u32> {
    match *data_type {
        Type::INT2 => Some(5),
        Type::INT4 => Some(10),
        Type::INT8 => Some(19),
        Type::FLOAT4 => Some(8),
        Type::FLOAT8 => Some(17),
        _ => None,
    }
}

fn postgres_type_scale(data_type: &Type) -> Option<i32> {
    matches!(*data_type, Type::INT2 | Type::INT4 | Type::INT8).then_some(0)
}

fn postgres_type_display_size(data_type: &Type) -> Option<u32> {
    match *data_type {
        Type::BOOL => Some(5),
        Type::INT2 => Some(6),
        Type::INT4 => Some(11),
        Type::INT8 => Some(20),
        Type::DATE => Some(10),
        Type::TIME => Some(15),
        Type::TIMESTAMP => Some(29),
        Type::TIMESTAMPTZ => Some(35),
        Type::UUID => Some(36),
        _ => None,
    }
}

fn postgres_type_signed(data_type: &Type) -> Option<bool> {
    if matches!(
        *data_type,
        Type::INT2 | Type::INT4 | Type::INT8 | Type::FLOAT4 | Type::FLOAT8 | Type::NUMERIC
    ) {
        Some(true)
    } else if postgres_unsigned_type(data_type) {
        Some(false)
    } else {
        None
    }
}

fn read_i16(raw: &[u8]) -> Result<i16, AppError> {
    raw.try_into()
        .map(i16::from_be_bytes)
        .map_err(|_| result_decode_error())
}

fn read_u16(raw: &[u8]) -> Result<u16, AppError> {
    raw.try_into()
        .map(u16::from_be_bytes)
        .map_err(|_| result_decode_error())
}

fn read_i32(raw: &[u8]) -> Result<i32, AppError> {
    raw.try_into()
        .map(i32::from_be_bytes)
        .map_err(|_| result_decode_error())
}

fn read_u32(raw: &[u8]) -> Result<u32, AppError> {
    raw.try_into()
        .map(u32::from_be_bytes)
        .map_err(|_| result_decode_error())
}

fn read_i64(raw: &[u8]) -> Result<i64, AppError> {
    raw.try_into()
        .map(i64::from_be_bytes)
        .map_err(|_| result_decode_error())
}

fn read_u64(raw: &[u8]) -> Result<u64, AppError> {
    raw.try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| result_decode_error())
}

fn take_bytes<'a>(raw: &mut &'a [u8], length: usize) -> Result<&'a [u8], AppError> {
    let (head, tail) = raw
        .split_at_checked(length)
        .ok_or_else(result_decode_error)?;
    *raw = tail;
    Ok(head)
}

fn take_i32(raw: &mut &[u8]) -> Result<i32, AppError> {
    read_i32(take_bytes(raw, 4)?)
}

fn take_u32(raw: &mut &[u8]) -> Result<u32, AppError> {
    read_u32(take_bytes(raw, 4)?)
}

fn ensure_postgres_scalar_size(byte_count: usize) -> Result<(), AppError> {
    if byte_count > MAX_SCALAR_BYTES {
        return Err(postgres_scalar_too_large(byte_count));
    }
    Ok(())
}

fn postgres_scalar_too_large(byte_count: usize) -> AppError {
    resource_error(
        "postgres_scalar_too_large",
        format!(
            "A PostgreSQL scalar contains {byte_count} bytes; the limit is {MAX_SCALAR_BYTES} bytes"
        ),
    )
}

fn postgres_opaque_value(type_name: &str, raw: &[u8]) -> Result<wire::jdbc_value::Value, AppError> {
    use wire::jdbc_value::Value;
    ensure_postgres_scalar_size(raw.len())?;
    let display_bytes = raw
        .len()
        .checked_mul(2)
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| postgres_scalar_too_large(raw.len()))?;
    ensure_postgres_scalar_size(display_bytes)?;
    Ok(Value::OpaqueValue(wire::OpaqueValue {
        type_name: type_name.to_owned(),
        display_value: format!("\\x{}", hex::encode(raw)),
    }))
}

fn postgres_utf8(raw: &[u8]) -> Result<String, AppError> {
    ensure_postgres_scalar_size(raw.len())?;
    std::str::from_utf8(raw)
        .map(str::to_owned)
        .map_err(|_| result_decode_error())
}

fn decode_postgres_network(data_type: &Type, raw: &[u8]) -> Result<String, AppError> {
    let [family, prefix_bits, is_cidr, address_length, address @ ..] = raw else {
        return Err(result_decode_error());
    };
    let (address, maximum_bits) = match (*family, usize::from(*address_length), address) {
        (2, 4, address) => {
            let octets: [u8; 4] = address.try_into().map_err(|_| result_decode_error())?;
            (Ipv4Addr::from(octets).to_string(), 32_u8)
        }
        (3, 16, address) => {
            let octets: [u8; 16] = address.try_into().map_err(|_| result_decode_error())?;
            (Ipv6Addr::from(octets).to_string(), 128_u8)
        }
        _ => return Err(result_decode_error()),
    };
    if *prefix_bits > maximum_bits || *is_cidr > 1 {
        return Err(result_decode_error());
    }
    if (*data_type == Type::CIDR && *is_cidr != 1) || (*data_type == Type::INET && *is_cidr != 0) {
        return Err(result_decode_error());
    }
    if *data_type == Type::INET && *prefix_bits == maximum_bits {
        Ok(address)
    } else {
        Ok(format!("{address}/{prefix_bits}"))
    }
}

fn decode_postgres_date(raw: &[u8]) -> Result<String, AppError> {
    let days = read_i32(raw)?;
    match days {
        i32::MAX => Ok("infinity".to_owned()),
        i32::MIN => Ok("-infinity".to_owned()),
        days => postgres_epoch()
            .checked_add_signed(TimeDelta::days(i64::from(days)))
            .map(|date| date.format("%Y-%m-%d").to_string())
            .ok_or_else(result_decode_error),
    }
}

fn decode_postgres_time(raw: &[u8]) -> Result<String, AppError> {
    format_time_micros(read_i64(raw)?)
}

fn decode_postgres_timestamp(raw: &[u8]) -> Result<String, AppError> {
    let micros = read_i64(raw)?;
    match micros {
        i64::MAX => Ok("infinity".to_owned()),
        i64::MIN => Ok("-infinity".to_owned()),
        micros => postgres_epoch()
            .and_hms_opt(0, 0, 0)
            .and_then(|epoch| epoch.checked_add_signed(TimeDelta::microseconds(micros)))
            .map(|value| value.format("%Y-%m-%dT%H:%M:%S%.6f").to_string())
            .ok_or_else(result_decode_error),
    }
}

fn decode_postgres_timetz(raw: &[u8]) -> Result<String, AppError> {
    let (time, zone) = raw.split_at_checked(8).ok_or_else(result_decode_error)?;
    let time = format_time_micros(read_i64(time)?)?;
    let seconds_west = read_i32(zone)?;
    let seconds_east = seconds_west.checked_neg().ok_or_else(result_decode_error)?;
    let sign = if seconds_east < 0 { '-' } else { '+' };
    let absolute = seconds_east.unsigned_abs();
    Ok(format!(
        "{time}{sign}{:02}:{:02}",
        absolute / 3_600,
        (absolute % 3_600) / 60
    ))
}

fn format_time_micros(micros: i64) -> Result<String, AppError> {
    if !(0..=86_400_000_000).contains(&micros) {
        return Err(result_decode_error());
    }
    if micros == 86_400_000_000 {
        return Ok("24:00:00".to_owned());
    }
    let hours = micros / 3_600_000_000;
    let minutes = (micros / 60_000_000) % 60;
    let seconds = (micros / 1_000_000) % 60;
    let fraction = micros % 1_000_000;
    if fraction == 0 {
        Ok(format!("{hours:02}:{minutes:02}:{seconds:02}"))
    } else {
        Ok(format!(
            "{hours:02}:{minutes:02}:{seconds:02}.{fraction:06}"
        ))
    }
}

fn postgres_epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(2000, 1, 1).expect("PostgreSQL epoch is a valid date")
}

fn decode_postgres_bits(raw: &[u8]) -> Result<String, AppError> {
    let (length, bytes) = raw.split_at_checked(4).ok_or_else(result_decode_error)?;
    let length = usize::try_from(read_i32(length)?).map_err(|_| result_decode_error())?;
    if bytes.len() != length.div_ceil(8) || length > MAX_SCALAR_BYTES {
        return Err(result_decode_error());
    }
    let mut result = String::with_capacity(length);
    for index in 0..length {
        let byte = bytes[index / 8];
        let bit = (byte >> (7 - index % 8)) & 1;
        result.push(if bit == 0 { '0' } else { '1' });
    }
    Ok(result)
}

fn decode_postgres_numeric(raw: &[u8]) -> Result<String, AppError> {
    if raw.len() < 8 || !raw.len().is_multiple_of(2) {
        return Err(result_decode_error());
    }
    let digit_count = usize::try_from(read_i16(&raw[0..2])?).map_err(|_| result_decode_error())?;
    let weight = i32::from(read_i16(&raw[2..4])?);
    let sign = read_u16(&raw[4..6])?;
    let scale = usize::from(read_u16(&raw[6..8])?);
    if raw.len() != 8 + digit_count.saturating_mul(2) {
        return Err(result_decode_error());
    }
    match sign {
        0xC000 => return Ok("NaN".to_owned()),
        0xD000 => return Ok("Infinity".to_owned()),
        0xF000 => return Ok("-Infinity".to_owned()),
        0x0000 | 0x4000 => {}
        _ => return Err(result_decode_error()),
    }
    let integer_groups =
        usize::try_from(weight.saturating_add(1).max(0)).map_err(|_| result_decode_error())?;
    let maximum_display_bytes = integer_groups
        .max(1)
        .checked_mul(4)
        .and_then(|length| length.checked_add(scale))
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| postgres_scalar_too_large(raw.len()))?;
    ensure_postgres_scalar_size(maximum_display_bytes)?;
    let digits = raw[8..]
        .chunks_exact(2)
        .map(read_u16)
        .collect::<Result<Vec<_>, _>>()?;
    if digits.iter().any(|digit| *digit > 9_999) {
        return Err(result_decode_error());
    }
    let mut integer = String::new();
    if integer_groups == 0 {
        integer.push('0');
    } else {
        for group in 0..integer_groups {
            let digit = digits.get(group).copied().unwrap_or(0);
            if group == 0 {
                write!(&mut integer, "{digit}").map_err(|_| AppError::internal())?;
            } else {
                write!(&mut integer, "{digit:04}").map_err(|_| AppError::internal())?;
            }
        }
    }
    let mut fraction = String::new();
    if scale > 0 {
        let leading_groups =
            usize::try_from((-weight - 1).max(0)).map_err(|_| result_decode_error())?;
        for _ in 0..leading_groups {
            fraction.push_str("0000");
        }
        let start = if weight >= 0 {
            usize::try_from(weight + 1).map_err(|_| result_decode_error())?
        } else {
            0
        };
        for digit in digits.iter().skip(start) {
            write!(&mut fraction, "{digit:04}").map_err(|_| AppError::internal())?;
        }
        while fraction.len() < scale {
            fraction.push('0');
        }
        fraction.truncate(scale);
    }
    let negative = sign == 0x4000 && (integer != "0" || fraction.bytes().any(|byte| byte != b'0'));
    let normalized_integer = integer.trim_start_matches('0');
    let normalized_integer = if normalized_integer.is_empty() {
        "0"
    } else {
        normalized_integer
    };
    Ok(format!(
        "{}{}{}",
        if negative { "-" } else { "" },
        normalized_integer,
        if scale == 0 {
            String::new()
        } else {
            format!(".{fraction}")
        }
    ))
}

fn decode_postgres_array(data_type: &Type, raw: &[u8]) -> Result<String, AppError> {
    let Kind::Array(element_type) = data_type.kind() else {
        return Err(result_decode_error());
    };
    let mut cursor = raw;
    let dimensions = usize::try_from(take_i32(&mut cursor)?).map_err(|_| result_decode_error())?;
    let _has_null = take_i32(&mut cursor)?;
    let element_oid = take_u32(&mut cursor)?;
    if element_oid != element_type.oid() || dimensions > POSTGRES_ARRAY_MAX_DIMENSIONS {
        return Err(result_decode_error());
    }
    let mut value_count = 1_usize;
    let mut shape = Vec::with_capacity(dimensions);
    for _ in 0..dimensions {
        let length = usize::try_from(take_i32(&mut cursor)?).map_err(|_| result_decode_error())?;
        let lower_bound = take_i32(&mut cursor)?;
        value_count = value_count
            .checked_mul(length)
            .ok_or_else(result_decode_error)?;
        shape.push(PostgresArrayDimension {
            length,
            lower_bound,
        });
    }
    if dimensions == 0 {
        value_count = 0;
    }
    if value_count > MAX_CONSOLE_PAGE_SIZE as usize * MAX_COLUMNS {
        return Err(resource_error(
            "postgres_array_too_large",
            "The PostgreSQL array exceeds the display element limit",
        ));
    }
    if value_count > cursor.len() / size_of::<i32>() {
        return Err(result_decode_error());
    }
    let mut values = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let length = take_i32(&mut cursor)?;
        if length == -1 {
            values.push(None);
            continue;
        }
        if length < -1 {
            return Err(result_decode_error());
        }
        let value = take_bytes(
            &mut cursor,
            usize::try_from(length).map_err(|_| result_decode_error())?,
        )?;
        values.push(Some(postgres_display_value(decode_postgres_value(
            element_type,
            value,
        )?)?));
    }
    if !cursor.is_empty() {
        return Err(result_decode_error());
    }
    render_postgres_array(&shape, &values)
}

#[derive(Debug, Clone, Copy)]
struct PostgresArrayDimension {
    length: usize,
    lower_bound: i32,
}

fn render_postgres_array(
    shape: &[PostgresArrayDimension],
    values: &[Option<String>],
) -> Result<String, AppError> {
    if shape.is_empty() {
        if !values.is_empty() {
            return Err(result_decode_error());
        }
        return Ok("{}".to_owned());
    }
    let mut output = String::new();
    if shape.iter().any(|dimension| dimension.lower_bound != 1) {
        for dimension in shape {
            let upper_bound = if dimension.length == 0 {
                dimension.lower_bound.saturating_sub(1)
            } else {
                dimension
                    .lower_bound
                    .checked_add(
                        i32::try_from(dimension.length - 1).map_err(|_| result_decode_error())?,
                    )
                    .ok_or_else(result_decode_error)?
            };
            push_postgres_array_text(
                &mut output,
                &format!("[{}:{upper_bound}]", dimension.lower_bound),
            )?;
        }
        push_postgres_array_text(&mut output, "=")?;
    }
    let mut value_index = 0_usize;
    render_postgres_array_dimension(&mut output, shape, 0, values, &mut value_index)?;
    if value_index != values.len() {
        return Err(result_decode_error());
    }
    Ok(output)
}

fn render_postgres_array_dimension(
    output: &mut String,
    shape: &[PostgresArrayDimension],
    depth: usize,
    values: &[Option<String>],
    value_index: &mut usize,
) -> Result<(), AppError> {
    push_postgres_array_text(output, "{")?;
    for index in 0..shape[depth].length {
        if index > 0 {
            push_postgres_array_text(output, ",")?;
        }
        if depth + 1 == shape.len() {
            let value = values.get(*value_index).ok_or_else(result_decode_error)?;
            *value_index = value_index.checked_add(1).ok_or_else(AppError::internal)?;
            match value {
                None => push_postgres_array_text(output, "NULL")?,
                Some(value) => push_postgres_array_element(output, value)?,
            }
        } else {
            render_postgres_array_dimension(output, shape, depth + 1, values, value_index)?;
        }
    }
    push_postgres_array_text(output, "}")
}

fn push_postgres_array_element(output: &mut String, value: &str) -> Result<(), AppError> {
    let quote = value.is_empty()
        || value.eq_ignore_ascii_case("NULL")
        || value.bytes().any(|byte| {
            matches!(byte, b',' | b'{' | b'}' | b'"' | b'\\') || byte.is_ascii_whitespace()
        });
    if !quote {
        return push_postgres_array_text(output, value);
    }
    push_postgres_array_text(output, "\"")?;
    for character in value.chars() {
        if matches!(character, '"' | '\\') {
            push_postgres_array_text(output, "\\")?;
        }
        let mut encoded = [0_u8; 4];
        push_postgres_array_text(output, character.encode_utf8(&mut encoded))?;
    }
    push_postgres_array_text(output, "\"")
}

fn push_postgres_array_text(output: &mut String, value: &str) -> Result<(), AppError> {
    if output.len().saturating_add(value.len()) > MAX_SCALAR_BYTES {
        return Err(postgres_scalar_too_large(
            output.len().saturating_add(value.len()),
        ));
    }
    output.push_str(value);
    Ok(())
}

fn postgres_display_value(value: wire::jdbc_value::Value) -> Result<String, AppError> {
    use wire::jdbc_value::Value;
    let display = match value {
        Value::NullValue(_) => "NULL".to_owned(),
        Value::BooleanValue(value) => value.to_string(),
        Value::SignedIntegerValue(value) => value.to_string(),
        Value::UnsignedIntegerValue(value) => value.to_string(),
        Value::Float32Value(value) => value.to_string(),
        Value::Float64Value(value) => value.to_string(),
        Value::DecimalValue(value)
        | Value::TextValue(value)
        | Value::DateValue(value)
        | Value::TimeValue(value)
        | Value::TimestampValue(value)
        | Value::TimestampWithTimeZoneValue(value)
        | Value::JsonValue(value)
        | Value::UuidValue(value) => value,
        Value::BinaryValue(value) => {
            let display_bytes = value
                .len()
                .checked_mul(2)
                .and_then(|length| length.checked_add(2))
                .ok_or_else(|| postgres_scalar_too_large(value.len()))?;
            ensure_postgres_scalar_size(display_bytes)?;
            format!("\\x{}", hex::encode(value))
        }
        Value::OpaqueValue(value) => value.display_value,
    };
    ensure_postgres_scalar_size(display.len())?;
    Ok(display)
}

fn result_decode_error() -> AppError {
    AppError::unavailable(
        "postgres_result_decode_failed",
        "A PostgreSQL result value could not be decoded safely",
    )
}

fn resource_error(code: impl Into<String>, message: impl Into<String>) -> AppError {
    AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new(code, message),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "the retained-query lifecycle keeps dispatch, streaming limits, persistence, and cleanup visible in one transaction"
)]
async fn execute_query_task(
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
    let parameters = postgres_query_parameters(&query.parameters)?;
    let connection = open_query_connection(&resolved, None, &mut cancellation).await?;
    if let Err(error) = connection
        .client()
        .batch_execute("BEGIN TRANSACTION READ ONLY")
        .await
        .map_err(postgres_query_error)
    {
        connection.abort().await;
        return Err(error.into());
    }

    let statement = match cancellable_postgres(
        connection.client().prepare(&query.sql),
        &mut cancellation,
    )
    .await
    {
        Ok(statement) => statement,
        Err(PostgresCancellableError::Cancelled(reason)) => {
            connection.abort().await;
            return Err(QueryTaskError::Cancelled(reason));
        }
        Err(PostgresCancellableError::Failed(error)) => {
            connection.abort().await;
            return Err(error.into());
        }
    };
    if statement.params().len() != parameters.len() {
        connection.abort().await;
        return Err(AppError::invalid(
            "invalid_query_parameter_count",
            format!(
                "The PostgreSQL statement expects {} parameters but {} were supplied",
                statement.params().len(),
                parameters.len()
            ),
        )
        .into());
    }
    let columns = statement.columns();
    if columns.len() > MAX_COLUMNS {
        connection.abort().await;
        return Err(resource_error(
            "postgres_result_too_wide",
            format!("PostgreSQL returned more than {MAX_COLUMNS} columns"),
        )
        .into());
    }
    let schema = wire::QueryStarted {
        columns: columns
            .iter()
            .enumerate()
            .map(|(index, column)| postgres_column(index, column))
            .collect::<Result<_, _>>()?,
    };
    let mut writer = RetainedWriter::begin(storage, schema, query.retention).await?;
    if let Err(error) = application.inner.operations.started(operation_id).await {
        abort_writer(&mut writer).await;
        connection.abort().await;
        return Err(error.into());
    }
    let parameter_refs = parameters
        .iter()
        .map(|parameter| parameter as &(dyn ToSql + Sync))
        .collect::<Vec<_>>();
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
    let consumption = async {
        let stream = match cancellable_postgres(
            connection.client().query_raw(&statement, parameter_refs),
            &mut cancellation,
        )
        .await
        {
            Ok(stream) => stream,
            Err(PostgresCancellableError::Cancelled(reason)) => {
                return Err(QueryTaskError::Cancelled(reason));
            }
            Err(PostgresCancellableError::Failed(error)) => return Err(error.into()),
        };
        tokio::pin!(stream);
        let mut pending_rows = Vec::new();
        let mut pending_bytes = 0_u64;
        let mut row_count = 0_u64;
        let mut result_bytes = 0_u64;
        let mut truncated_rows = false;
        let mut truncated_bytes = false;
        let mut cancellation_open = true;
        loop {
            let next = tokio::select! {
                biased;
                changed = cancellation.changed(), if cancellation_open => {
                    if let Ok(()) = changed {
                        if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                            return Err(QueryTaskError::Cancelled(reason));
                        }
                    } else {
                        cancellation_open = false;
                    }
                    continue;
                }
                row = stream.next() => row,
            };
            let Some(row) = next else {
                break;
            };
            let row = row.map_err(postgres_query_error)?;
            if max_rows != 0 && row_count >= max_rows {
                truncated_rows = true;
                break;
            }
            let row = postgres_row(&row, columns)?;
            let row_bytes = u64::try_from(row.encoded_len()).map_err(|_| AppError::internal())?;
            if result_bytes.saturating_add(row_bytes) > max_result_bytes {
                truncated_bytes = true;
                break;
            }
            let entry_bytes = row_batch_entry_bytes(&row)?;
            let candidate_bytes = pending_bytes
                .saturating_add(if pending_rows.is_empty() {
                    row_batch_prefix_bytes(row_count)
                } else {
                    0
                })
                .saturating_add(entry_bytes);
            if !pending_rows.is_empty()
                && (pending_rows.len() >= usize::try_from(batch_rows).unwrap_or(usize::MAX)
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
            pending_rows.push(row);
            pending_bytes = pending_bytes.saturating_add(entry_bytes);
            row_count = row_count.checked_add(1).ok_or_else(AppError::internal)?;
            result_bytes = result_bytes
                .checked_add(row_bytes)
                .ok_or_else(AppError::internal)?;
        }
        flush_rows(
            application,
            operation_id,
            &mut writer,
            &mut pending_rows,
            row_count,
        )
        .await?;
        Ok::<_, QueryTaskError>((row_count, truncated_rows, truncated_bytes))
    }
    .await;
    let (row_count, truncated_rows, truncated_bytes) = match consumption {
        Ok(outcome) => outcome,
        Err(error) => {
            abort_writer(&mut writer).await;
            connection.abort().await;
            return Err(error);
        }
    };
    let metadata = match writer
        .finish(wire::QueryCompleted {
            row_count,
            truncated_by_max_rows: truncated_rows,
            truncated_by_max_result_bytes: truncated_bytes,
        })
        .await
    {
        Ok(metadata) => metadata,
        Err(error) => {
            abort_writer(&mut writer).await;
            connection.abort().await;
            return Err(error.into());
        }
    };
    if truncated_rows || truncated_bytes {
        connection.abort().await;
    } else {
        let rollback = connection.client().batch_execute("ROLLBACK").await;
        let result = rollback.map_err(postgres_query_error).map(|()| metadata);
        return finish_connection(connection, result)
            .await
            .map_err(QueryTaskError::from);
    }
    Ok(metadata)
}

async fn open_query_connection(
    resolved: &ResolvedDatasourceConnection,
    database_name: Option<&str>,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<ManagedPostgresConnection, QueryTaskError> {
    let open = open_resolved_connection(resolved, database_name);
    tokio::pin!(open);
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if let Ok(()) = changed {
                    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                        return Err(QueryTaskError::Cancelled(reason));
                    }
                } else {
                    cancellation_open = false;
                }
            }
            result = &mut open => return result.map_err(QueryTaskError::from),
        }
    }
}

enum PostgresCancellableError {
    Cancelled(Option<String>),
    Failed(AppError),
}

async fn cancellable_postgres<T, F>(
    future: F,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<T, PostgresCancellableError>
where
    F: Future<Output = Result<T, PostgresError>>,
{
    tokio::pin!(future);
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if let Ok(()) = changed {
                    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                        return Err(PostgresCancellableError::Cancelled(reason));
                    }
                } else {
                    cancellation_open = false;
                }
            }
            result = &mut future => {
                return result
                    .map_err(postgres_query_error)
                    .map_err(PostgresCancellableError::Failed);
            }
        }
    }
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
            "postgres_result_batch_too_large",
            "One PostgreSQL result row exceeds the retained-result batch limit",
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
        tracing::warn!(error = %error, "native PostgreSQL retained-result cleanup failed");
    }
}

async fn execute_update(
    resolved: ResolvedDatasourceConnection,
    sql: String,
    cancellation: CancellationToken,
) -> Result<u64, DatabaseWriteError> {
    if cancellation.is_cancelled() {
        return Err(DatabaseWriteError::not_started(postgres_write_cancelled()));
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
    let connection = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            return Err(DatabaseWriteError::not_started(postgres_write_cancelled()));
        }
        result = open_resolved_connection(&resolved, None) => {
            result.map_err(DatabaseWriteError::not_started)?
        }
    };
    let statement = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            connection.abort().await;
            return Err(DatabaseWriteError::not_started(postgres_write_cancelled()));
        }
        result = connection.client().prepare(&sql) => {
            result.map_err(postgres_query_error).map_err(DatabaseWriteError::not_started)?
        }
    };
    if !statement.params().is_empty() {
        connection.abort().await;
        return Err(DatabaseWriteError::not_started(AppError::invalid(
            "invalid_database_write",
            "The confirmed PostgreSQL write does not accept unbound parameters",
        )));
    }
    let result = tokio::select! {
        biased;
        () = cancellation.cancelled() => None,
        result = connection.client().execute(&statement, &[]) => Some(result),
    };
    let Some(result) = result else {
        connection.abort().await;
        return Err(DatabaseWriteError::unknown(AppError::unavailable(
            "database_write_outcome_unknown",
            "The PostgreSQL write was interrupted after dispatch; do not retry it blindly",
        )));
    };
    match result {
        Ok(affected_rows) => finish_connection(connection, Ok(affected_rows))
            .await
            .map_err(DatabaseWriteError::unknown),
        Err(error) => {
            connection.abort().await;
            tracing::warn!(error = %error, "PostgreSQL rejected a dispatched write");
            Err(DatabaseWriteError::unknown(AppError::unavailable(
                "database_write_outcome_unknown",
                "PostgreSQL reported an error after write dispatch; do not retry it blindly",
            )))
        }
    }
}

fn validate_single_write_sql(sql: &str) -> Result<String, AppError> {
    if sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid(
            "invalid_database_write",
            format!("SQL cannot exceed {MAX_SQL_BYTES} UTF-8 bytes"),
        ));
    }
    let mut statements = split_postgres_script(sql)?;
    if statements.len() != 1 {
        return Err(AppError::invalid(
            "invalid_database_write",
            "Exactly one PostgreSQL write statement is required",
        ));
    }
    let statement = statements.pop().expect("statement length checked");
    let words = postgres_words(&statement)?;
    if !matches!(
        words.first().map(String::as_str),
        Some(
            "INSERT"
                | "UPDATE"
                | "DELETE"
                | "MERGE"
                | "CREATE"
                | "ALTER"
                | "DROP"
                | "TRUNCATE"
                | "GRANT"
                | "REVOKE"
                | "COMMENT"
                | "ANALYZE"
                | "VACUUM"
                | "CALL"
                | "DO"
                | "REFRESH"
                | "REINDEX"
                | "CLUSTER"
        )
    ) {
        return Err(AppError::invalid(
            "database_write_statement_required",
            "The confirmed PostgreSQL write surface accepts one DML, DDL, grant, maintenance, or routine statement",
        ));
    }
    Ok(statement)
}

fn postgres_write_cancelled() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "database_write_cancelled",
            "The PostgreSQL write was cancelled before dispatch",
        ),
    )
}

struct ConsoleStatementExecution {
    result: Option<NativeConsoleResult>,
    failure: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleStatementKind {
    ReadOnly,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleFailure {
    Cancelled,
    TimedOut,
    ResultProcessing,
    Driver { server_rejected: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConsoleDispatchState {
    statement_kind: ConsoleStatementKind,
    dispatched: bool,
}

impl ConsoleDispatchState {
    fn classify(sql: &str) -> Self {
        Self {
            statement_kind: if validate_read_sql(sql).is_ok() {
                ConsoleStatementKind::ReadOnly
            } else {
                ConsoleStatementKind::Write
            },
            dispatched: false,
        }
    }

    fn mark_dispatched(&mut self) {
        self.dispatched = true;
    }

    fn requires_unknown_outcome(self, failure: ConsoleFailure) -> bool {
        self.statement_kind == ConsoleStatementKind::Write
            && self.dispatched
            && !matches!(
                failure,
                ConsoleFailure::Driver {
                    server_rejected: true
                }
            )
    }
}

enum ConsoleExecutionError {
    Cancelled(Option<String>),
    Fatal(AppError),
}

enum ConsolePostgresError {
    Cancelled(Option<String>),
    Failed(PostgresError),
}

async fn execute_console(
    application: &Application,
    request: NativeConsoleRequest,
    mut cancellation: watch::Receiver<CancellationRequest>,
    force_read_only: bool,
) -> Result<Vec<NativeConsoleResult>, AppError> {
    let (statements, page_offset, page_end) = prepare_console_statements(&request)?;
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(postgres_console_cancelled(reason));
    }
    let resolved = resolve_native_connection(application, &request.datasource_id).await?;
    let read_only = force_read_only || resolved.connection.read_only;
    if read_only {
        validate_read_only_console(&statements, force_read_only)?;
    }
    let target_database =
        (!request.database_name.trim().is_empty()).then_some(request.database_name.as_str());
    let connection = match open_query_connection(&resolved, target_database, &mut cancellation)
        .await
    {
        Ok(connection) => connection,
        Err(QueryTaskError::Cancelled(reason)) => return Err(postgres_console_cancelled(reason)),
        Err(QueryTaskError::Failed(error)) => return Err(error),
    };
    if read_only && let Err(error) = begin_read_only_console_transaction(&connection).await {
        connection.abort().await;
        return Err(error);
    }

    let mut results = Vec::new();
    let mut retained_result_bytes = 0_u64;
    let mut dispatched_write = false;
    for (index, statement) in statements.into_iter().enumerate() {
        let mut dispatch = ConsoleDispatchState::classify(&statement);
        let statement_sequence = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or_else(AppError::internal)?;
        let started = Instant::now();
        let Ok(execution) = tokio::time::timeout(
            CONSOLE_STATEMENT_TIMEOUT,
            execute_console_statement(
                &connection,
                &statement,
                statement_sequence,
                page_offset,
                page_end,
                request.result_set_id,
                &mut retained_result_bytes,
                &mut cancellation,
                &mut dispatch,
            ),
        )
        .await
        else {
            connection.abort().await;
            return Err(postgres_console_timeout(dispatch));
        };
        dispatched_write |= dispatch.requires_unknown_outcome(ConsoleFailure::Driver {
            server_rejected: false,
        });
        match execution {
            Ok(execution) => {
                if let Some(result) = execution.result {
                    results.push(result);
                }
                if let Some(error) = execution.failure {
                    results.push(console_failure_result(
                        statement_sequence,
                        statement,
                        &error,
                        elapsed_millis(started),
                    ));
                    if !request.error_continue {
                        break;
                    }
                }
            }
            Err(ConsoleExecutionError::Cancelled(reason)) => {
                connection.abort().await;
                return Err(postgres_console_interrupted(dispatch, reason));
            }
            Err(ConsoleExecutionError::Fatal(error)) => {
                connection.abort().await;
                return Err(postgres_console_fatal(dispatch, error));
            }
        }
    }
    if read_only && let Err(error) = rollback_read_only_console_transaction(&connection).await {
        connection.abort().await;
        return Err(error);
    }
    finish_console_connection(connection, results, dispatched_write).await
}

fn postgres_console_timeout(dispatch: ConsoleDispatchState) -> AppError {
    if dispatch.requires_unknown_outcome(ConsoleFailure::TimedOut) {
        postgres_console_write_outcome_unknown(ConsoleFailure::TimedOut)
    } else {
        postgres_console_statement_timeout()
    }
}

fn postgres_console_interrupted(
    dispatch: ConsoleDispatchState,
    reason: Option<String>,
) -> AppError {
    if dispatch.requires_unknown_outcome(ConsoleFailure::Cancelled) {
        postgres_console_write_outcome_unknown(ConsoleFailure::Cancelled)
    } else {
        postgres_console_cancelled(reason)
    }
}

fn postgres_console_fatal(dispatch: ConsoleDispatchState, error: AppError) -> AppError {
    if dispatch.requires_unknown_outcome(ConsoleFailure::ResultProcessing) {
        postgres_console_write_outcome_unknown(ConsoleFailure::ResultProcessing)
    } else {
        error
    }
}

async fn finish_console_connection(
    connection: ManagedPostgresConnection,
    results: Vec<NativeConsoleResult>,
    dispatched_write: bool,
) -> Result<Vec<NativeConsoleResult>, AppError> {
    match finish_connection(connection, Ok(results)).await {
        Err(error) if dispatched_write => {
            tracing::warn!(error = %error, "PostgreSQL connection failed after a Console write dispatch");
            Err(postgres_console_write_outcome_unknown(
                ConsoleFailure::Driver {
                    server_rejected: false,
                },
            ))
        }
        result => result,
    }
}

async fn begin_read_only_console_transaction(
    connection: &ManagedPostgresConnection,
) -> Result<(), AppError> {
    match tokio::time::timeout(
        CONSOLE_STATEMENT_TIMEOUT,
        connection
            .client()
            .batch_execute("BEGIN TRANSACTION READ ONLY"),
    )
    .await
    {
        Ok(result) => result.map_err(postgres_query_error),
        Err(_) => Err(postgres_console_statement_timeout()),
    }
}

async fn rollback_read_only_console_transaction(
    connection: &ManagedPostgresConnection,
) -> Result<(), AppError> {
    match tokio::time::timeout(
        DISCONNECT_TIMEOUT,
        connection.client().batch_execute("ROLLBACK"),
    )
    .await
    {
        Ok(result) => result.map_err(postgres_query_error),
        Err(_) => Err(AppError::unavailable(
            "postgres_console_rollback_timeout",
            "The PostgreSQL read-only Console transaction did not roll back in time",
        )),
    }
}

fn postgres_console_statement_timeout() -> AppError {
    AppError::unavailable(
        "postgres_console_statement_timeout",
        format!(
            "A PostgreSQL Console statement exceeded the {} second execution limit",
            CONSOLE_STATEMENT_TIMEOUT.as_secs()
        ),
    )
}

fn postgres_console_write_outcome_unknown(failure: ConsoleFailure) -> AppError {
    let message = match failure {
        ConsoleFailure::Cancelled => {
            "The PostgreSQL Console write was cancelled after dispatch; its outcome is unknown, so do not retry it blindly"
        }
        ConsoleFailure::TimedOut => {
            "The PostgreSQL Console write timed out after dispatch; its outcome is unknown, so do not retry it blindly"
        }
        ConsoleFailure::ResultProcessing => {
            "The PostgreSQL Console could not finish processing a write result after dispatch; its outcome is unknown, so do not retry it blindly"
        }
        ConsoleFailure::Driver { .. } => {
            "The PostgreSQL Console connection failed after write dispatch; its outcome is unknown, so do not retry it blindly"
        }
    };
    AppError::new(
        AppErrorKind::Unavailable,
        ApiError::new("database_write_outcome_unknown", message),
    )
}

fn prepare_console_statements(
    request: &NativeConsoleRequest,
) -> Result<(Vec<String>, u64, u64), AppError> {
    let (page_offset, page_end) = validate_console_request(request)?;
    let mut statements = if request.single {
        vec![request.sql.trim().to_owned()]
    } else {
        split_postgres_script(&request.sql)?
    };
    if statements.is_empty() {
        return Err(AppError::invalid(
            "invalid_postgres_console_request",
            "sql must contain at least one PostgreSQL statement",
        ));
    }
    if request.explain {
        for statement in &mut statements {
            *statement = format!("EXPLAIN {statement}");
        }
    }
    Ok((statements, page_offset, page_end))
}

fn validate_console_request(request: &NativeConsoleRequest) -> Result<(u64, u64), AppError> {
    if request.datasource_id.trim().is_empty() || request.sql.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_postgres_console_request",
            "dataSourceId and sql cannot be empty",
        ));
    }
    if request.sql.len() > MAX_SQL_BYTES {
        return Err(resource_error(
            "postgres_console_script_too_large",
            format!("PostgreSQL Console scripts are limited to {MAX_SQL_BYTES} bytes"),
        ));
    }
    if !request.database_name.trim().is_empty() {
        validate_identifier(&request.database_name, "databaseName")?;
    }
    if request.page_no == 0 || request.page_size == 0 || request.page_size > MAX_CONSOLE_PAGE_SIZE {
        return Err(AppError::invalid(
            "invalid_postgres_console_request",
            format!(
                "pageNo must be positive and pageSize must be between 1 and {MAX_CONSOLE_PAGE_SIZE}"
            ),
        ));
    }
    if request.result_set_id == Some(0) {
        return Err(AppError::invalid(
            "invalid_postgres_console_request",
            "resultSetId must be a positive one-based integer",
        ));
    }
    if request.page_size_all {
        Ok((0, u64::from(MAX_CONSOLE_PAGE_SIZE)))
    } else {
        let offset = u64::from(request.page_no - 1) * u64::from(request.page_size);
        let end = offset
            .checked_add(u64::from(request.page_size))
            .ok_or_else(AppError::internal)?;
        Ok((offset, end))
    }
}

fn validate_forced_read_console(statements: &[String]) -> Result<(), AppError> {
    for statement in statements {
        validate_read_sql(statement).map_err(|_| {
            AppError::invalid(
                "chart_query_must_be_read_only",
                "Chart refresh accepts only PostgreSQL read statements without writes or row locks",
            )
        })?;
    }
    Ok(())
}

fn validate_read_only_console(
    statements: &[String],
    force_read_only: bool,
) -> Result<(), AppError> {
    if force_read_only {
        return validate_forced_read_console(statements);
    }
    for statement in statements {
        validate_read_sql(statement).map_err(|_| {
            AppError::invalid(
                "postgres_console_must_be_read_only",
                "This PostgreSQL datasource accepts only read statements without writes or row locks",
            )
        })?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn execute_console_statement(
    connection: &ManagedPostgresConnection,
    sql: &str,
    statement_sequence: u32,
    page_offset: u64,
    page_end: u64,
    selected_result_set_id: Option<u32>,
    retained_result_bytes: &mut u64,
    cancellation: &mut watch::Receiver<CancellationRequest>,
    dispatch: &mut ConsoleDispatchState,
) -> Result<ConsoleStatementExecution, ConsoleExecutionError> {
    let started = Instant::now();
    let statement =
        match cancellable_console_postgres(connection.client().prepare(sql), cancellation).await {
            Ok(statement) => statement,
            Err(ConsolePostgresError::Cancelled(reason)) => {
                return Err(ConsoleExecutionError::Cancelled(reason));
            }
            Err(ConsolePostgresError::Failed(error)) => {
                return console_postgres_failure(*dispatch, error);
            }
        };
    if !statement.params().is_empty() {
        return Ok(ConsoleStatementExecution {
            result: None,
            failure: Some(AppError::invalid(
                "invalid_postgres_console_request",
                "PostgreSQL Console statements cannot contain unbound parameters",
            )),
        });
    }
    if statement.columns().is_empty() {
        dispatch.mark_dispatched();
        let update_count = match cancellable_console_postgres(
            connection.client().execute(&statement, &[]),
            cancellation,
        )
        .await
        {
            Ok(count) => count,
            Err(ConsolePostgresError::Cancelled(reason)) => {
                return Err(ConsoleExecutionError::Cancelled(reason));
            }
            Err(ConsolePostgresError::Failed(error)) => {
                return console_postgres_failure(*dispatch, error);
            }
        };
        return Ok(ConsoleStatementExecution {
            result: Some(NativeConsoleResult {
                statement_sequence,
                result_set_id: None,
                sql: sql.to_owned(),
                success: true,
                message: "Statement executed successfully".to_owned(),
                update_count,
                columns: Vec::new(),
                rows: Vec::new(),
                row_count: 0,
                has_more: false,
                duration_ms: elapsed_millis(started),
                error: None,
            }),
            failure: None,
        });
    }
    let retain = selected_result_set_id.is_none_or(|selected| selected == 1);
    let columns = statement.columns();
    if columns.len() > MAX_COLUMNS {
        return Err(ConsoleExecutionError::Fatal(resource_error(
            "postgres_result_too_wide",
            format!("PostgreSQL returned more than {MAX_COLUMNS} columns"),
        )));
    }
    let converted_columns = if retain {
        columns
            .iter()
            .enumerate()
            .map(|(index, column)| console_column(index, column))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ConsoleExecutionError::Fatal)?
    } else {
        Vec::new()
    };
    dispatch.mark_dispatched();
    let stream = match cancellable_console_postgres(
        connection
            .client()
            .query_raw(&statement, std::iter::empty::<&(dyn ToSql + Sync)>()),
        cancellation,
    )
    .await
    {
        Ok(stream) => stream,
        Err(ConsolePostgresError::Cancelled(reason)) => {
            return Err(ConsoleExecutionError::Cancelled(reason));
        }
        Err(ConsolePostgresError::Failed(error)) => {
            return console_postgres_failure(*dispatch, error);
        }
    };
    tokio::pin!(stream);
    let mut rows = Vec::new();
    let mut row_count = 0_u64;
    let mut scanned_bytes = 0_u64;
    let mut cancellation_open = true;
    loop {
        let next = tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if let Ok(()) = changed {
                    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                        return Err(ConsoleExecutionError::Cancelled(reason));
                    }
                } else {
                    cancellation_open = false;
                }
                continue;
            }
            row = stream.next() => row,
        };
        let Some(row) = next else {
            break;
        };
        let row = match row {
            Ok(row) => row,
            Err(error) => {
                return console_postgres_failure(*dispatch, error);
            }
        };
        let wire_row = postgres_row(&row, columns).map_err(ConsoleExecutionError::Fatal)?;
        let next_row_count = row_count
            .checked_add(1)
            .ok_or_else(|| ConsoleExecutionError::Fatal(AppError::internal()))?;
        if next_row_count > MAX_CONSOLE_SCANNED_ROWS {
            return Err(ConsoleExecutionError::Fatal(resource_error(
                "postgres_console_scan_row_limit_exceeded",
                format!(
                    "A PostgreSQL Console result exceeds the {MAX_CONSOLE_SCANNED_ROWS} row scan limit"
                ),
            )));
        }
        let row_bytes = u64::try_from(wire_row.encoded_len())
            .map_err(|_| ConsoleExecutionError::Fatal(AppError::internal()))?;
        let next_scanned_bytes = scanned_bytes
            .checked_add(row_bytes)
            .ok_or_else(|| ConsoleExecutionError::Fatal(AppError::internal()))?;
        if next_scanned_bytes > MAX_CONSOLE_SCANNED_BYTES {
            return Err(ConsoleExecutionError::Fatal(resource_error(
                "postgres_console_scan_byte_limit_exceeded",
                format!(
                    "A PostgreSQL Console result exceeds the {MAX_CONSOLE_SCANNED_BYTES} byte scan limit"
                ),
            )));
        }
        if retain && (page_offset..page_end).contains(&row_count) {
            let retained_row =
                console_row_from_wire(wire_row).map_err(ConsoleExecutionError::Fatal)?;
            reserve_console_result_bytes(retained_result_bytes, &retained_row)
                .map_err(ConsoleExecutionError::Fatal)?;
            rows.push(retained_row);
        }
        row_count = next_row_count;
        scanned_bytes = next_scanned_bytes;
    }
    Ok(ConsoleStatementExecution {
        result: retain.then_some(NativeConsoleResult {
            statement_sequence,
            result_set_id: Some(1),
            sql: sql.to_owned(),
            success: true,
            message: "Statement executed successfully".to_owned(),
            update_count: 0,
            columns: converted_columns,
            rows,
            row_count,
            has_more: row_count > page_end,
            duration_ms: elapsed_millis(started),
            error: None,
        }),
        failure: None,
    })
}

async fn cancellable_console_postgres<T, F>(
    future: F,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<T, ConsolePostgresError>
where
    F: Future<Output = Result<T, PostgresError>>,
{
    tokio::pin!(future);
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if let Ok(()) = changed {
                    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                        return Err(ConsolePostgresError::Cancelled(reason));
                    }
                } else {
                    cancellation_open = false;
                }
            }
            result = &mut future => return result.map_err(ConsolePostgresError::Failed),
        }
    }
}

fn console_postgres_failure(
    dispatch: ConsoleDispatchState,
    error: PostgresError,
) -> Result<ConsoleStatementExecution, ConsoleExecutionError> {
    let failure = ConsoleFailure::Driver {
        server_rejected: error.as_db_error().is_some(),
    };
    if dispatch.requires_unknown_outcome(failure) {
        return Err(ConsoleExecutionError::Fatal(
            postgres_console_write_outcome_unknown(failure),
        ));
    }
    Ok(ConsoleStatementExecution {
        result: None,
        failure: Some(postgres_query_error(error)),
    })
}

fn console_column(index: usize, column: &tokio_postgres::Column) -> Result<ResultColumn, AppError> {
    let column = postgres_column(index, column)?;
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
    Ok(ResultColumn {
        ordinal: column.ordinal,
        label: column.label,
        name: column.name,
        jdbc_type: column.jdbc_type,
        jdbc_type_name: column.jdbc_type_name,
        value_type,
        nullability: ColumnNullability::Unknown,
        precision: column.precision,
        scale: column.scale,
        display_size: column.display_size,
        signed: column.signed,
        catalog_name: column.catalog_name,
        schema_name: column.schema_name,
        table_name: column.table_name,
    })
}

fn console_row_from_wire(wire: wire::JdbcRow) -> Result<ResultRow, AppError> {
    Ok(ResultRow {
        values: wire
            .values
            .into_iter()
            .map(console_value)
            .collect::<Result<_, _>>()?,
    })
}

fn console_value(value: wire::JdbcValue) -> Result<JdbcValue, AppError> {
    use wire::jdbc_value::Value;
    Ok(match value.value.ok_or_else(AppError::internal)? {
        Value::NullValue(_) => JdbcValue::Null,
        Value::BooleanValue(value) => JdbcValue::Boolean { value },
        Value::SignedIntegerValue(value) => JdbcValue::SignedInteger {
            value: value.to_string(),
        },
        Value::UnsignedIntegerValue(value) => JdbcValue::UnsignedInteger {
            value: value.to_string(),
        },
        Value::Float32Value(value) => JdbcValue::Float32 {
            value: display_float32(value),
        },
        Value::Float64Value(value) => JdbcValue::Float64 {
            value: display_float64(value),
        },
        Value::DecimalValue(value) => JdbcValue::Decimal { value },
        Value::TextValue(value) => JdbcValue::Text { value },
        Value::BinaryValue(value) => JdbcValue::Binary {
            value: BASE64_STANDARD.encode(value),
        },
        Value::DateValue(value) => JdbcValue::Date { value },
        Value::TimeValue(value) => JdbcValue::Time { value },
        Value::TimestampValue(value) => JdbcValue::Timestamp { value },
        Value::TimestampWithTimeZoneValue(value) => JdbcValue::TimestampWithTimeZone { value },
        Value::JsonValue(value) => JdbcValue::Json { value },
        Value::UuidValue(value) => JdbcValue::Uuid { value },
        Value::OpaqueValue(value) => JdbcValue::Opaque {
            type_name: value.type_name,
            display_value: value.display_value,
        },
    })
}

fn reserve_console_result_bytes(total: &mut u64, row: &ResultRow) -> Result<(), AppError> {
    let next = total.saturating_add(console_row_retained_bytes(row));
    if next > MAX_CONSOLE_RESULT_BYTES {
        return Err(resource_error(
            "postgres_console_result_too_large",
            format!(
                "PostgreSQL Console results are limited to {MAX_CONSOLE_RESULT_BYTES} retained bytes"
            ),
        ));
    }
    *total = next;
    Ok(())
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

fn console_failure_result(
    statement_sequence: u32,
    sql: String,
    error: &AppError,
    duration_ms: u64,
) -> NativeConsoleResult {
    let api_error = error.api_error();
    NativeConsoleResult {
        statement_sequence,
        result_set_id: None,
        sql,
        success: false,
        message: api_error.message.clone(),
        update_count: 0,
        columns: Vec::new(),
        rows: Vec::new(),
        row_count: 0,
        has_more: false,
        duration_ms,
        error: Some(api_error),
    }
}

fn postgres_console_cancelled(reason: Option<String>) -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "postgres_console_cancelled",
            reason.unwrap_or_else(|| "The PostgreSQL Console execution was cancelled".to_owned()),
        ),
    )
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn display_float32(value: f32) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f32::INFINITY {
        "Infinity".to_owned()
    } else if value == f32::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        value.to_string()
    }
}

fn display_float64(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Infinity".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PostgresScriptState {
    Normal,
    SingleQuote,
    DoubleQuote,
    DollarQuote(String),
    LineComment,
    BlockComment(usize),
}

fn split_postgres_script(script: &str) -> Result<Vec<String>, AppError> {
    let bytes = script.as_bytes();
    let mut statements = Vec::new();
    let mut state = PostgresScriptState::Normal;
    let mut statement_start = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        match &mut state {
            PostgresScriptState::Normal => match bytes[index] {
                b'\'' => state = PostgresScriptState::SingleQuote,
                b'"' => state = PostgresScriptState::DoubleQuote,
                b'$' => {
                    if let Some(tag) = postgres_dollar_tag(script, index) {
                        index = index.saturating_add(tag.len().saturating_sub(1));
                        state = PostgresScriptState::DollarQuote(tag.to_owned());
                    }
                }
                b'-' if bytes.get(index + 1) == Some(&b'-') => {
                    index += 1;
                    state = PostgresScriptState::LineComment;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    index += 1;
                    state = PostgresScriptState::BlockComment(1);
                }
                b';' => {
                    push_postgres_statement(&mut statements, &script[statement_start..index])?;
                    statement_start = index + 1;
                }
                _ => {}
            },
            PostgresScriptState::SingleQuote => {
                if bytes[index] == b'\\' {
                    index = index.saturating_add(1);
                } else if bytes[index] == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 1;
                    } else {
                        state = PostgresScriptState::Normal;
                    }
                }
            }
            PostgresScriptState::DoubleQuote => {
                if bytes[index] == b'"' {
                    if bytes.get(index + 1) == Some(&b'"') {
                        index += 1;
                    } else {
                        state = PostgresScriptState::Normal;
                    }
                }
            }
            PostgresScriptState::DollarQuote(tag) => {
                if script[index..].starts_with(tag.as_str()) {
                    index = index.saturating_add(tag.len().saturating_sub(1));
                    state = PostgresScriptState::Normal;
                }
            }
            PostgresScriptState::LineComment => {
                if bytes[index] == b'\n' {
                    state = PostgresScriptState::Normal;
                }
            }
            PostgresScriptState::BlockComment(depth) => {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    *depth = depth.saturating_add(1);
                    index += 1;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    *depth -= 1;
                    index += 1;
                    if *depth == 0 {
                        state = PostgresScriptState::Normal;
                    }
                }
            }
        }
        index += 1;
    }
    match state {
        PostgresScriptState::Normal | PostgresScriptState::LineComment => {}
        PostgresScriptState::SingleQuote => {
            return Err(invalid_console_script("unterminated string literal"));
        }
        PostgresScriptState::DoubleQuote => {
            return Err(invalid_console_script("unterminated quoted identifier"));
        }
        PostgresScriptState::DollarQuote(_) => {
            return Err(invalid_console_script("unterminated dollar-quoted body"));
        }
        PostgresScriptState::BlockComment(_) => {
            return Err(invalid_console_script("unterminated block comment"));
        }
    }
    push_postgres_statement(&mut statements, &script[statement_start..])?;
    Ok(statements)
}

fn postgres_words(sql: &str) -> Result<Vec<String>, AppError> {
    let _ = split_postgres_script(sql)?;
    let bytes = sql.as_bytes();
    let mut words = Vec::new();
    let mut state = PostgresScriptState::Normal;
    let mut index = 0_usize;
    while index < bytes.len() {
        match &mut state {
            PostgresScriptState::Normal => match bytes[index] {
                b'\'' => state = PostgresScriptState::SingleQuote,
                b'"' => state = PostgresScriptState::DoubleQuote,
                b'$' => {
                    if let Some(tag) = postgres_dollar_tag(sql, index) {
                        index = index.saturating_add(tag.len().saturating_sub(1));
                        state = PostgresScriptState::DollarQuote(tag.to_owned());
                    } else {
                        index += 1;
                        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                            index += 1;
                        }
                        index = index.saturating_sub(1);
                    }
                }
                b'-' if bytes.get(index + 1) == Some(&b'-') => {
                    index += 1;
                    state = PostgresScriptState::LineComment;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    index += 1;
                    state = PostgresScriptState::BlockComment(1);
                }
                byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                    let start = index;
                    index += 1;
                    while bytes.get(index).is_some_and(|byte| {
                        byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$'
                    }) {
                        index += 1;
                    }
                    words.push(sql[start..index].to_ascii_uppercase());
                    index -= 1;
                }
                _ => {}
            },
            PostgresScriptState::SingleQuote => {
                if bytes[index] == b'\\' {
                    index = index.saturating_add(1);
                } else if bytes[index] == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 1;
                    } else {
                        state = PostgresScriptState::Normal;
                    }
                }
            }
            PostgresScriptState::DoubleQuote => {
                if bytes[index] == b'"' {
                    if bytes.get(index + 1) == Some(&b'"') {
                        index += 1;
                    } else {
                        state = PostgresScriptState::Normal;
                    }
                }
            }
            PostgresScriptState::DollarQuote(tag) => {
                if sql[index..].starts_with(tag.as_str()) {
                    index = index.saturating_add(tag.len().saturating_sub(1));
                    state = PostgresScriptState::Normal;
                }
            }
            PostgresScriptState::LineComment => {
                if bytes[index] == b'\n' {
                    state = PostgresScriptState::Normal;
                }
            }
            PostgresScriptState::BlockComment(depth) => {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    *depth = depth.saturating_add(1);
                    index += 1;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    *depth -= 1;
                    index += 1;
                    if *depth == 0 {
                        state = PostgresScriptState::Normal;
                    }
                }
            }
        }
        index += 1;
    }
    Ok(words)
}

fn postgres_dollar_tag(sql: &str, start: usize) -> Option<&str> {
    let bytes = sql.as_bytes();
    if bytes.get(start) != Some(&b'$') {
        return None;
    }
    let mut end = start + 1;
    while let Some(byte) = bytes.get(end) {
        if *byte == b'$' {
            return Some(&sql[start..=end]);
        }
        if end - start > 64
            || !(*byte == b'_' || byte.is_ascii_alphanumeric())
            || (end == start + 1 && byte.is_ascii_digit())
        {
            return None;
        }
        end += 1;
    }
    None
}

fn push_postgres_statement(statements: &mut Vec<String>, statement: &str) -> Result<(), AppError> {
    let statement = statement.trim();
    if statement.is_empty() {
        return Ok(());
    }
    if statements.len() >= MAX_CONSOLE_STATEMENTS {
        return Err(resource_error(
            "postgres_console_too_many_statements",
            format!(
                "PostgreSQL Console scripts are limited to {MAX_CONSOLE_STATEMENTS} statements"
            ),
        ));
    }
    statements.push(statement.to_owned());
    Ok(())
}

fn invalid_console_script(detail: &str) -> AppError {
    AppError::invalid(
        "invalid_postgres_console_script",
        format!("The PostgreSQL Console script contains an {detail}"),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chat2db_contract::DatasourceConnectionProperty;

    use super::*;
    use crate::native_driver_types::{CreateSchemaSqlRequest, SchemaDefinition};

    fn datasource_connection(jdbc_url: impl Into<String>) -> DatasourceConnection {
        DatasourceConnection {
            jdbc_url: jdbc_url.into(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "postgres".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "postgres".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: false,
            ssh: None,
        }
    }

    #[test]
    fn normalizes_jdbc_urls_and_rejects_unrelated_schemes() {
        assert_eq!(
            normalize_postgres_url("JDBC:POSTGRESQL://db.example/app").expect("JDBC URL"),
            "postgresql://db.example/app"
        );
        assert_eq!(
            normalize_postgres_url("postgres://db.example/app").expect("Postgres URL"),
            "postgresql://db.example/app"
        );
        assert!(normalize_postgres_url("jdbc:mysql://db.example/app").is_err());
    }

    #[test]
    fn connection_config_replaces_the_original_port_for_tunnels() {
        let mut connection = datasource_connection(
            "jdbc:postgresql://url_user@db.example:6432/app?sslmode=require&currentSchema=tenant",
        );
        connection.properties.push(DatasourceConnectionProperty {
            key: "user".to_owned(),
            value: "property_user".to_owned(),
            sensitive: false,
        });
        let (config, tls_mode, target_host, target_port) =
            connection_config(&connection, Some("selected_db"), Some(15_432))
                .expect("connection config");

        assert_eq!(target_host, "db.example");
        assert_eq!(target_port, 6_432);
        assert_eq!(tls_mode, PostgresTlsMode::Require);
        assert_eq!(config.get_ports(), &[15_432]);
        assert_eq!(config.get_user(), Some("property_user"));
        assert_eq!(config.get_dbname(), Some("selected_db"));
        assert_eq!(config.get_options(), Some("-c search_path=tenant"));
    }

    #[test]
    fn splits_dollar_quoted_scripts_and_nested_comments() {
        let script = r"
            CREATE FUNCTION public.answer() RETURNS integer AS $body$
            BEGIN
                /* inner ; /* nested ; */ still inner */
                RETURN 42;
            END;
            $body$ LANGUAGE plpgsql;
            SELECT ';' AS semicolon;
            -- trailing comment ;
            SELECT 2;
        ";
        let statements = split_postgres_script(script).expect("PostgreSQL script");

        assert_eq!(statements.len(), 3);
        assert!(statements[0].contains("RETURN 42;"));
        assert!(statements[1].contains("SELECT ';'"));
        assert!(statements[2].ends_with("SELECT 2"));
        assert!(split_postgres_script("SELECT $broken$body").is_err());
    }

    #[test]
    fn read_only_validation_blocks_writes_and_row_locks() {
        assert!(validate_read_sql("WITH rows AS (SELECT 1) SELECT * FROM rows").is_ok());
        assert!(validate_read_sql("VALUES (1), (2)").is_ok());
        assert!(validate_read_sql("SELECT * FROM example FOR UPDATE").is_err());
        assert!(
            validate_read_sql("WITH removed AS (DELETE FROM t RETURNING *) SELECT * FROM removed")
                .is_err()
        );
        assert!(validate_read_sql("SELECT 1; SELECT 2").is_err());
        assert!(
            validate_forced_read_console(&[
                "SELECT 1".to_owned(),
                "WITH row AS (SELECT 2) SELECT * FROM row".to_owned(),
            ])
            .is_ok()
        );
        assert!(
            validate_forced_read_console(&["SELECT 1".to_owned(), "COMMIT".to_owned()]).is_err()
        );
        assert!(validate_read_sql("SELECT * FROM example FOR KEY SHARE").is_err());
    }

    #[test]
    fn console_dispatch_tracks_unknown_write_outcomes_fail_closed() {
        for sql in ["SELECT 1", "VALUES (1)", "EXPLAIN SELECT 1"] {
            assert_eq!(
                ConsoleDispatchState::classify(sql).statement_kind,
                ConsoleStatementKind::ReadOnly,
                "{sql} must remain a read-only Console statement"
            );
        }
        for sql in [
            "INSERT INTO example VALUES (1)",
            "UPDATE example SET value = 1",
            "INSERT INTO example VALUES (1) RETURNING value",
        ] {
            assert_eq!(
                ConsoleDispatchState::classify(sql).statement_kind,
                ConsoleStatementKind::Write,
                "{sql} must be treated as a Console write"
            );
        }

        let mut write = ConsoleDispatchState::classify("INSERT INTO example VALUES (1)");
        assert!(!write.requires_unknown_outcome(ConsoleFailure::Cancelled));
        assert!(!write.requires_unknown_outcome(ConsoleFailure::TimedOut));
        write.mark_dispatched();
        assert!(write.requires_unknown_outcome(ConsoleFailure::Cancelled));
        assert!(write.requires_unknown_outcome(ConsoleFailure::TimedOut));
        assert!(write.requires_unknown_outcome(ConsoleFailure::ResultProcessing));
        assert!(write.requires_unknown_outcome(ConsoleFailure::Driver {
            server_rejected: false,
        }));
        assert!(!write.requires_unknown_outcome(ConsoleFailure::Driver {
            server_rejected: true,
        }));

        let mut read = ConsoleDispatchState::classify("SELECT 1");
        read.mark_dispatched();
        assert!(!read.requires_unknown_outcome(ConsoleFailure::Cancelled));
        assert!(!read.requires_unknown_outcome(ConsoleFailure::TimedOut));
        assert!(!read.requires_unknown_outcome(ConsoleFailure::ResultProcessing));
        assert!(!read.requires_unknown_outcome(ConsoleFailure::Driver {
            server_rejected: false,
        }));
    }

    #[test]
    fn maps_postgres_type_names_without_losing_time_zone_qualifiers() {
        assert_eq!(
            postgres_jdbc_type_name("timestamp(6) with time zone"),
            2_014
        );
        assert_eq!(postgres_jdbc_type_name("timestamp without time zone"), 93);
        assert_eq!(postgres_jdbc_type_name("time with time zone"), 2_013);
        assert_eq!(postgres_jdbc_type_name("character varying(64)"), 12);
        assert_eq!(postgres_jdbc_type_name("integer[]"), 2_003);
        assert_eq!(postgres_jdbc_type(&Type::TIMETZ), 2_013);
    }

    #[test]
    fn decodes_binary_numeric_and_array_values() {
        let mut numeric = Vec::new();
        numeric.extend_from_slice(&3_i16.to_be_bytes());
        numeric.extend_from_slice(&1_i16.to_be_bytes());
        numeric.extend_from_slice(&0_u16.to_be_bytes());
        numeric.extend_from_slice(&4_u16.to_be_bytes());
        for digit in [1_u16, 2_345, 6_789] {
            numeric.extend_from_slice(&digit.to_be_bytes());
        }
        assert_eq!(
            decode_postgres_numeric(&numeric).expect("numeric value"),
            "12345.6789"
        );

        let mut array = Vec::new();
        array.extend_from_slice(&1_i32.to_be_bytes());
        array.extend_from_slice(&1_i32.to_be_bytes());
        array.extend_from_slice(&Type::INT4.oid().to_be_bytes());
        array.extend_from_slice(&3_i32.to_be_bytes());
        array.extend_from_slice(&1_i32.to_be_bytes());
        for value in [Some(1_i32), Some(-2_i32), None] {
            match value {
                Some(value) => {
                    array.extend_from_slice(&4_i32.to_be_bytes());
                    array.extend_from_slice(&value.to_be_bytes());
                }
                None => array.extend_from_slice(&(-1_i32).to_be_bytes()),
            }
        }
        assert_eq!(
            decode_postgres_array(&Type::INT4_ARRAY, &array).expect("integer array"),
            "{1,-2,NULL}"
        );
    }

    #[test]
    fn decodes_multidimensional_arrays_with_unambiguous_escaping() {
        let mut array = Vec::new();
        array.extend_from_slice(&2_i32.to_be_bytes());
        array.extend_from_slice(&0_i32.to_be_bytes());
        array.extend_from_slice(&Type::TEXT.oid().to_be_bytes());
        for dimension in [2_i32, 2_i32] {
            array.extend_from_slice(&dimension.to_be_bytes());
            array.extend_from_slice(&1_i32.to_be_bytes());
        }
        for value in ["a,b", "NULL", "quote\"slash\\", "white space"] {
            array.extend_from_slice(
                &i32::try_from(value.len())
                    .expect("small array value")
                    .to_be_bytes(),
            );
            array.extend_from_slice(value.as_bytes());
        }

        assert_eq!(
            decode_postgres_array(&Type::TEXT_ARRAY, &array).expect("text array"),
            "{{\"a,b\",\"NULL\"},{\"quote\\\"slash\\\\\",\"white space\"}}"
        );

        let mut lower_bound = Vec::new();
        lower_bound.extend_from_slice(&1_i32.to_be_bytes());
        lower_bound.extend_from_slice(&0_i32.to_be_bytes());
        lower_bound.extend_from_slice(&Type::INT4.oid().to_be_bytes());
        lower_bound.extend_from_slice(&1_i32.to_be_bytes());
        lower_bound.extend_from_slice(&0_i32.to_be_bytes());
        lower_bound.extend_from_slice(&4_i32.to_be_bytes());
        lower_bound.extend_from_slice(&7_i32.to_be_bytes());
        assert_eq!(
            decode_postgres_array(&Type::INT4_ARRAY, &lower_bound).expect("lower-bound array"),
            "[0:0]={7}"
        );

        let mut excessive_dimensions = Vec::new();
        excessive_dimensions.extend_from_slice(&7_i32.to_be_bytes());
        excessive_dimensions.extend_from_slice(&0_i32.to_be_bytes());
        excessive_dimensions.extend_from_slice(&Type::INT4.oid().to_be_bytes());
        assert!(decode_postgres_array(&Type::INT4_ARRAY, &excessive_dimensions).is_err());
    }

    #[test]
    fn network_and_money_binary_values_match_their_declared_schema() {
        let inet = [2, 24, 0, 4, 192, 168, 4, 7];
        assert_eq!(
            decode_postgres_network(&Type::INET, &inet).expect("IPv4 inet"),
            "192.168.4.7/24"
        );
        let mut cidr = vec![3, 64, 1, 16];
        cidr.extend_from_slice(&Ipv6Addr::from_str("2001:db8::").expect("IPv6").octets());
        assert_eq!(
            decode_postgres_network(&Type::CIDR, &cidr).expect("IPv6 cidr"),
            "2001:db8::/64"
        );

        assert_eq!(
            postgres_value_type(&Type::MONEY),
            wire::JdbcValueType::Opaque
        );
        assert_eq!(postgres_jdbc_type(&Type::MONEY), 1_111);
        let decoded =
            decode_postgres_value(&Type::MONEY, &12_345_i64.to_be_bytes()).expect("money value");
        assert!(matches!(
            decoded,
            wire::jdbc_value::Value::OpaqueValue(value)
                if value.type_name == "money" && value.display_value == "raw_units=12345"
        ));
    }

    #[test]
    fn scalar_limits_are_checked_before_driver_owned_copies_and_hex_expansion() {
        let oversized = vec![0_u8; MAX_SCALAR_BYTES + 1];
        let raw = RawPostgresValue::from_sql(&Type::BYTEA, &oversized)
            .expect("oversized values are represented without cloning");
        assert!(matches!(
            raw,
            RawPostgresValue::TooLarge { byte_count } if byte_count == MAX_SCALAR_BYTES + 1
        ));

        let opaque = vec![0_u8; MAX_SCALAR_BYTES / 2 + 1];
        assert!(postgres_opaque_value("opaque", &opaque).is_err());
    }

    #[test]
    fn quotes_identifiers_literals_and_builds_schema_sql() {
        assert_eq!(
            quote_identifier("Mixed\"Name", "name").expect("identifier"),
            "\"Mixed\"\"Name\""
        );
        assert_eq!(quote_literal("owner's").expect("literal"), "E'owner''s'");
        assert_eq!(
            quote_literal("path\\owner's").expect("escaped literal"),
            "E'path\\\\owner''s'"
        );
        assert!(quote_identifier(&"a".repeat(63), "name").is_ok());
        assert!(quote_identifier(&"a".repeat(64), "name").is_err());
        assert!(quote_identifier(&"é".repeat(31), "name").is_ok());
        assert!(quote_identifier(&"é".repeat(32), "name").is_err());
        let built = build_create_schema(CreateSchemaSqlRequest {
            schema: SchemaDefinition {
                database_name: "app".to_owned(),
                name: "Team Data".to_owned(),
                comment: "owner's schema".to_owned(),
                owner: "postgres".to_owned(),
                system: false,
            },
        })
        .expect("schema SQL");
        assert_eq!(
            built.sql,
            "CREATE SCHEMA \"Team Data\" AUTHORIZATION \"postgres\";\nCOMMENT ON SCHEMA \"Team Data\" IS E'owner''s schema';"
        );
    }

    #[test]
    fn unknown_connection_properties_fail_closed() {
        let mut connection = datasource_connection("jdbc:postgresql://db.example/app");
        connection.properties.push(DatasourceConnectionProperty {
            key: "vendorMagic".to_owned(),
            value: "enabled".to_owned(),
            sensitive: false,
        });
        assert!(connection_config(&connection, None, None).is_err());
    }

    #[test]
    fn rustls_provider_initialization_is_panic_free_and_preserves_existing_provider() {
        let before = rustls::crypto::CryptoProvider::get_default().map(std::sync::Arc::as_ptr);
        let initialization = std::panic::catch_unwind(ensure_postgres_rustls_provider);
        assert!(
            initialization.is_ok(),
            "provider initialization must not panic"
        );
        initialization
            .expect("caught provider initialization")
            .expect("a Rustls provider must be available");
        let after = rustls::crypto::CryptoProvider::get_default().map(std::sync::Arc::as_ptr);
        assert!(after.is_some());
        if before.is_some() {
            assert_eq!(before, after, "an installed provider must not be replaced");
        }
    }

    #[tokio::test]
    async fn closed_cancellation_sender_does_not_spin() {
        let (sender, mut receiver) = watch::channel(CancellationRequest::Waiting);
        drop(sender);
        let completed = tokio::time::timeout(
            Duration::from_secs(1),
            cancellable_postgres(async { Ok::<u8, PostgresError>(7) }, &mut receiver),
        )
        .await
        .expect("closed cancellation channels must not starve the database future");
        match completed {
            Ok(value) => assert_eq!(value, 7),
            Err(PostgresCancellableError::Cancelled(reason)) => {
                panic!("unexpected cancellation after sender closed: {reason:?}");
            }
            Err(PostgresCancellableError::Failed(error)) => {
                panic!("unexpected database failure after sender closed: {error}");
            }
        }
    }

    fn local_smoke_connection() -> DatasourceConnection {
        let mut connection =
            datasource_connection(std::env::var("CHAT2DB_POSTGRES_URL").unwrap_or_else(|_| {
                "jdbc:postgresql://127.0.0.1:5432/app?sslmode=disable".to_owned()
            }));
        connection.properties[0].value =
            std::env::var("CHAT2DB_POSTGRES_USER").unwrap_or_else(|_| "postgres".to_owned());
        connection.properties[1].value =
            std::env::var("CHAT2DB_POSTGRES_PASSWORD").unwrap_or_else(|_| "postgres".to_owned());
        connection
    }

    #[tokio::test]
    #[ignore = "requires a PostgreSQL server; run explicitly for native-driver verification"]
    #[allow(
        clippy::too_many_lines,
        reason = "the real smoke intentionally verifies the complete PostgreSQL fixture lifecycle in one test"
    )]
    async fn real_postgres_driver_smoke() {
        let connection = open_connection(&local_smoke_connection())
            .await
            .expect("PostgreSQL connection");
        connection
            .client()
            .simple_query("SELECT 1")
            .await
            .expect("connection test");
        connection
            .client()
            .batch_execute("SET standard_conforming_strings = off")
            .await
            .expect("legacy string setting");
        let literal_value = "path\\segment's value";
        let literal_sql = format!(
            "SELECT {}::text",
            quote_literal(literal_value).expect("escaped PostgreSQL literal")
        );
        let literal_round_trip: String = connection
            .client()
            .query_one(&literal_sql, &[])
            .await
            .expect("escaped PostgreSQL literal query")
            .try_get(0)
            .expect("escaped PostgreSQL literal value");
        assert_eq!(literal_round_trip, literal_value);
        let schema = format!("chat2db_native_smoke_{}", std::process::id());
        let quoted_schema = quote_identifier(&schema, "schemaName").expect("smoke schema");
        let fixture_sql = format!(
            r"
            CREATE SCHEMA {quoted_schema};
            CREATE TABLE {quoted_schema}.parent (
                id BIGSERIAL PRIMARY KEY,
                label TEXT NOT NULL
            );
            CREATE TABLE {quoted_schema}.child (
                id BIGSERIAL PRIMARY KEY,
                parent_id BIGINT NOT NULL REFERENCES {quoted_schema}.parent(id),
                payload NUMERIC(12, 4),
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );
            CREATE INDEX child_payload_idx ON {quoted_schema}.child(payload);
            CREATE VIEW {quoted_schema}.child_view AS SELECT id, payload FROM {quoted_schema}.child;
            CREATE FUNCTION {quoted_schema}.add_one(value integer) RETURNS integer
                LANGUAGE SQL IMMUTABLE AS $function$ SELECT value + 1 $function$;
            CREATE PROCEDURE {quoted_schema}.no_op()
                LANGUAGE plpgsql AS $procedure$ BEGIN NULL; END $procedure$;
            CREATE FUNCTION {quoted_schema}.touch_child() RETURNS trigger
                LANGUAGE plpgsql AS $trigger_function$
                BEGIN NEW.created_at = clock_timestamp(); RETURN NEW; END
                $trigger_function$;
            CREATE TRIGGER child_touch BEFORE UPDATE ON {quoted_schema}.child
                FOR EACH ROW EXECUTE FUNCTION {quoted_schema}.touch_child();
            INSERT INTO {quoted_schema}.parent(label) VALUES ('parent');
            INSERT INTO {quoted_schema}.child(parent_id, payload) VALUES (1, 12345.6789);
            "
        );
        connection
            .client()
            .batch_execute(&fixture_sql)
            .await
            .expect("PostgreSQL smoke fixture");

        let verification = async {
            let database_name: String = connection
                .client()
                .query_one("SELECT current_database()", &[])
                .await
                .map_err(postgres_query_error)?
                .try_get(0)
                .map_err(postgres_query_error)?;
            let databases: i64 = connection
                .client()
                .query_one("SELECT count(*) FROM pg_database", &[])
                .await
                .map_err(postgres_query_error)?
                .try_get(0)
                .map_err(postgres_query_error)?;
            let catalog_checks = [
                ("schema", "SELECT count(*) FROM pg_namespace WHERE nspname = $1"),
                ("table", "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relkind IN ('r', 'p')"),
                ("column", "SELECT count(*) FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND a.attnum > 0 AND NOT a.attisdropped"),
                ("index", "SELECT count(*) FROM pg_index i JOIN pg_class c ON c.oid = i.indrelid JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1"),
                ("view", "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relkind IN ('v', 'm')"),
                ("key", "SELECT count(*) FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace WHERE n.nspname = $1 AND c.contype IN ('p', 'f')"),
                ("function", "SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = $1 AND p.prokind = 'f'"),
                ("procedure", "SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = $1 AND p.prokind = 'p'"),
                ("trigger", "SELECT count(*) FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND NOT t.tgisinternal"),
                ("ER foreign key", "SELECT count(*) FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace WHERE n.nspname = $1 AND c.contype = 'f'"),
            ];
            for (label, sql) in catalog_checks {
                let count: i64 = connection
                    .client()
                    .query_one(sql, &[&schema])
                    .await
                    .map_err(postgres_query_error)?
                    .try_get(0)
                    .map_err(postgres_query_error)?;
                if count == 0 {
                    return Err(resource_error(
                        "postgres_smoke_metadata_missing",
                        format!("PostgreSQL {label} metadata returned no rows"),
                    ));
                }
            }
            let ddl = build_table_ddl(&connection, &database_name, &schema, "child").await?;
            let preview = connection
                .client()
                .query(
                    &format!("SELECT * FROM {quoted_schema}.child LIMIT 10"),
                    &[],
                )
                .await
                .map_err(postgres_query_error)?;
            let preview_row = preview.first().ok_or_else(|| {
                resource_error("postgres_smoke_preview_empty", "PostgreSQL preview returned no rows")
            })?;
            let decoded_preview = postgres_row(preview_row, preview_row.columns())?;
            Ok::<_, AppError>((databases, ddl, decoded_preview.values.len()))
        }
        .await;

        let cleanup_result = connection
            .client()
            .batch_execute(&format!("DROP SCHEMA {quoted_schema} CASCADE"))
            .await
            .map_err(postgres_query_error);
        finish_connection(connection, cleanup_result)
            .await
            .expect("PostgreSQL smoke cleanup");
        let (databases, ddl, preview_columns) =
            verification.expect("PostgreSQL smoke verification");
        assert!(databases > 0);
        assert!(ddl.contains("CREATE TABLE"));
        assert!(ddl.contains("FOREIGN KEY"));
        assert!(ddl.contains("CREATE INDEX"));
        assert_eq!(preview_columns, 4);
    }
}
