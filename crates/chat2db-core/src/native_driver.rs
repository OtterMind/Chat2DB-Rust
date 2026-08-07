use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use chat2db_contract::{
    DatasourceConnection, ImportFileRequest, ResultMetadata, SqlFileExportRequest, TransferArtifact,
};
use chat2db_storage::Storage;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, Application,
    datasource_session::ResolvedDatasourceConnection,
    native_administration_types::{
        AdministrationCapability, AdministrationCommand, AdministrationExecution,
        AdministrationPreview, PrincipalGrantList, PrincipalGrantsRequest, PrincipalList,
    },
    native_dm,
    native_driver_types::{
        BuiltSql, ColumnList, CreateSchemaSqlRequest, DatabaseList, DmlSqlRequest,
        EntityRelationTable, ForeignKeyList, FunctionList, FunctionMetadata, FunctionParameterList,
        IndexList, ListColumnsRequest, ListDatabasesRequest, ListIndexesRequest,
        ListRoutinesRequest, ListSchemasRequest, ListTableKeysRequest, ListTablesRequest,
        ListTriggersRequest, ListViewsRequest, MetadataObjectRef, NamespaceSqlRequest,
        NativeDriverDescriptor, PrimaryKeyList, ProcedureList, ProcedureMetadata,
        ProcedureParameterList, RoutineInvocationPreview, RoutineInvocationRequest,
        RoutineMigrationExecution, RoutineMigrationRequest, SchemaList, TableList, TableMetadata,
        TablePreviewAccepted, TablePreviewRequest, TriggerList, TriggerMetadata, ViewList,
    },
    native_mysql,
    native_oracle::OracleNativeDriver,
    native_postgres::PostgresNativeDriver,
    native_schema_diff_types::{SchemaDiffRequest, SchemaDiffSql},
    native_sqlserver::SqlServerNativeDriver,
    operation::CancellationRequest,
    query::{
        DatabaseWriteError, NativeConsoleRequest, NativeConsoleResult, PreparedQuery,
        QueryTaskError,
    },
    transfer::{QueryResultExportRequest, TableFileExportRequest},
};

/// Database connection operations implemented by one native Rust driver.
#[async_trait]
pub(crate) trait NativeConnectionDriver: Send + Sync {
    async fn test_connection(&self, connection: &DatasourceConnection) -> Result<(), AppError>;

    async fn test_connection_with_local_port(
        &self,
        _connection: &DatasourceConnection,
    ) -> Result<Option<u16>, AppError> {
        Err(AppError::invalid(
            "ssh_driver_not_supported",
            "SSH forwarding is not supported by this native Rust driver",
        ))
    }
}

/// Query and Console operations implemented by one native Rust driver.
#[async_trait]
pub(crate) trait NativeQueryDriver: Send + Sync {
    fn is_read_candidate(&self, sql: &str) -> Result<bool, AppError>;

    fn validate_query(&self, query: &PreparedQuery) -> Result<(), AppError>;

    async fn execute_query_task(
        &self,
        application: &Application,
        operation_id: &str,
        cancellation: watch::Receiver<CancellationRequest>,
        query: PreparedQuery,
        storage: Storage,
        resolved: ResolvedDatasourceConnection,
    ) -> Result<ResultMetadata, QueryTaskError>;

    async fn execute_update(
        &self,
        resolved: ResolvedDatasourceConnection,
        sql: String,
        cancellation: CancellationToken,
    ) -> Result<u64, DatabaseWriteError>;

    async fn execute_console(
        &self,
        application: &Application,
        request: NativeConsoleRequest,
        cancellation: watch::Receiver<CancellationRequest>,
        force_read_only: bool,
    ) -> Result<Vec<NativeConsoleResult>, AppError>;
}

/// Relational metadata operations exposed through the native driver SPI.
#[async_trait]
pub(crate) trait NativeMetadataDriver: Send + Sync {
    async fn list_schemas(
        &self,
        application: &Application,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError>;

    async fn list_databases(
        &self,
        application: &Application,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError>;

    async fn list_tables(
        &self,
        application: &Application,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError>;

    async fn list_columns(
        &self,
        application: &Application,
        request: ListColumnsRequest,
    ) -> Result<ColumnList, AppError>;

    async fn list_indexes(
        &self,
        application: &Application,
        request: ListIndexesRequest,
    ) -> Result<IndexList, AppError>;

    async fn list_views(
        &self,
        application: &Application,
        request: ListViewsRequest,
    ) -> Result<ViewList, AppError>;

    async fn get_view(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TableMetadata, AppError>;

    async fn list_imported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError>;

    async fn list_exported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError>;

    async fn list_primary_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<PrimaryKeyList, AppError>;

    async fn list_functions(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<FunctionList, AppError>;

    async fn get_function(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionMetadata, AppError>;

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionParameterList, AppError>;

    async fn list_procedures(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError>;

    async fn get_procedure(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureMetadata, AppError>;

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureParameterList, AppError>;

    async fn list_triggers(
        &self,
        application: &Application,
        request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError>;

    async fn get_trigger(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TriggerMetadata, AppError>;
}

/// Table-specific native capabilities that are not plain metadata listings.
#[async_trait]
pub(crate) trait NativeTableDriver: Send + Sync {
    async fn load_er_tables(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Vec<EntityRelationTable>, AppError>;

    async fn validate_column_reorder(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        table_name: &str,
        column_names: &[String],
    ) -> Result<(), AppError>;

    async fn table_ddl(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<String, AppError>;

    async fn start_table_preview(
        &self,
        application: &Application,
        request: TablePreviewRequest,
        row_limit: u32,
    ) -> Result<TablePreviewAccepted, AppError>;
}

/// Stored-routine operations implemented by a native Rust driver.
#[async_trait]
pub(crate) trait NativeRoutineDriver: Send + Sync {
    async fn preview_invocation(
        &self,
        application: &Application,
        request: RoutineInvocationRequest,
    ) -> Result<RoutineInvocationPreview, AppError>;

    fn preview_migration(
        &self,
        request: RoutineMigrationRequest,
    ) -> Result<RoutineInvocationPreview, AppError>;

    async fn execute_migration(
        &self,
        application: &Application,
        request: RoutineMigrationRequest,
    ) -> Result<RoutineMigrationExecution, AppError>;
}

/// Import and export operations implemented by a native Rust driver.
#[async_trait]
pub(crate) trait NativeTransferDriver: Send + Sync {
    async fn import_file(
        &self,
        application: &Application,
        request: ImportFileRequest,
    ) -> Result<crate::transfer::TransferJobSpec, AppError>;

    async fn export_sql_file(
        &self,
        application: &Application,
        request: SqlFileExportRequest,
    ) -> Result<crate::transfer::TransferJobSpec, AppError>;

    async fn export_table_file(
        &self,
        application: &Application,
        request: TableFileExportRequest,
    ) -> Result<crate::transfer::TransferJobSpec, AppError>;

    async fn export_query_result(
        &self,
        application: &Application,
        request: QueryResultExportRequest,
    ) -> Result<TransferArtifact, AppError>;
}

/// Structured SQL builders supplied by one native database dialect.
pub(crate) trait NativeDialectDriver: Send + Sync {
    fn build_create_schema(&self, request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError>;

    fn build_namespace_sql(&self, request: NamespaceSqlRequest) -> Result<BuiltSql, AppError>;

    fn build_dml(&self, request: DmlSqlRequest) -> Result<BuiltSql, AppError>;
}

/// Database account and role administration implemented by a native driver.
#[async_trait]
pub(crate) trait NativeAdministrationDriver: Send + Sync {
    async fn administration_capability(
        &self,
        application: &Application,
        datasource_id: &str,
    ) -> Result<AdministrationCapability, AppError>;

    async fn list_principals(
        &self,
        application: &Application,
        datasource_id: &str,
    ) -> Result<PrincipalList, AppError>;

    async fn principal_grants(
        &self,
        application: &Application,
        request: &PrincipalGrantsRequest,
    ) -> Result<PrincipalGrantList, AppError>;

    fn preview_administration(
        &self,
        application: &Application,
        request: &AdministrationCommand,
    ) -> Result<AdministrationPreview, AppError>;

    async fn execute_administration(
        &self,
        application: &Application,
        request: &AdministrationCommand,
    ) -> Result<AdministrationExecution, AppError>;
}

/// Schema-comparison operations implemented by a native Rust driver.
#[async_trait]
pub(crate) trait NativeSchemaDiffDriver: Send + Sync {
    async fn preview_schema_diff(
        &self,
        application: &Application,
        request: &SchemaDiffRequest,
    ) -> Result<SchemaDiffSql, AppError>;
}

/// Runtime-polymorphic native Rust database driver.
///
/// Optional capability accessors allow a driver to participate only in the
/// product surfaces it implements. Additional capability traits are attached
/// here as native metadata and dialect services are migrated.
pub(crate) trait NativeDriver: Send + Sync {
    fn descriptor(&self) -> &'static NativeDriverDescriptor;

    /// Whether this driver may transparently replace a persisted managed JDBC datasource.
    ///
    /// Native drivers default to an explicit opt-in migration boundary so adding a Rust driver
    /// cannot silently change the execution engine of existing JDBC datasources.
    fn can_replace_managed_jdbc_datasource(&self) -> bool {
        false
    }

    fn connection(&self) -> Option<&dyn NativeConnectionDriver> {
        None
    }

    fn query(&self) -> Option<&dyn NativeQueryDriver> {
        None
    }

    fn metadata(&self) -> Option<&dyn NativeMetadataDriver> {
        None
    }

    fn tables(&self) -> Option<&dyn NativeTableDriver> {
        None
    }

    fn routines(&self) -> Option<&dyn NativeRoutineDriver> {
        None
    }

    fn transfer(&self) -> Option<&dyn NativeTransferDriver> {
        None
    }

    fn dialect(&self) -> Option<&dyn NativeDialectDriver> {
        None
    }

    fn administration(&self) -> Option<&dyn NativeAdministrationDriver> {
        None
    }

    fn schema_diff(&self) -> Option<&dyn NativeSchemaDiffDriver> {
        None
    }
}

/// Immutable registry used to select native implementations at runtime.
#[derive(Clone)]
pub(crate) struct NativeDriverRegistry {
    drivers: Arc<[Arc<dyn NativeDriver>]>,
}

impl NativeDriverRegistry {
    pub(crate) fn built_in() -> Self {
        Self::try_new(vec![
            Arc::new(MysqlNativeDriver),
            Arc::new(DmNativeDriver),
            Arc::new(PostgresNativeDriver),
            Arc::new(SqlServerNativeDriver),
            Arc::new(OracleNativeDriver),
        ])
        .expect("built-in native drivers must have unique identities")
    }

    pub(crate) fn try_new(drivers: Vec<Arc<dyn NativeDriver>>) -> Result<Self, AppError> {
        let mut driver_ids = HashSet::new();
        let mut identifier_owners = HashMap::<String, String>::new();
        for driver in &drivers {
            let descriptor = driver.descriptor();
            let trimmed_driver_id = descriptor.id.trim();
            let driver_id = trimmed_driver_id.to_ascii_lowercase();
            if driver_id.is_empty()
                || descriptor.id != trimmed_driver_id
                || !driver_ids.insert(driver_id.clone())
            {
                return Err(AppError::invalid(
                    "invalid_native_driver_registry",
                    "native driver ids must be trimmed, non-empty, and unique",
                ));
            }
            if descriptor.implementation.is_empty()
                || descriptor.implementation != descriptor.implementation.trim()
            {
                return Err(AppError::invalid(
                    "invalid_native_driver_registry",
                    "native driver implementation names must be trimmed and non-empty",
                ));
            }
            if descriptor.database_types.is_empty() {
                return Err(AppError::invalid(
                    "invalid_native_driver_registry",
                    "native drivers must declare at least one database type",
                ));
            }

            for identifier in std::iter::once(descriptor.id)
                .chain(descriptor.database_types.iter().copied())
                .chain(descriptor.compatibility_aliases.iter().copied())
            {
                let trimmed_identifier = identifier.trim();
                if trimmed_identifier.is_empty() || identifier != trimmed_identifier {
                    return Err(AppError::invalid(
                        "invalid_native_driver_registry",
                        "native driver identifiers and aliases must be trimmed and non-empty",
                    ));
                }
                let identifier = trimmed_identifier.to_ascii_lowercase();
                if let Some(owner) = identifier_owners.get(&identifier) {
                    if owner != &driver_id {
                        return Err(AppError::invalid(
                            "invalid_native_driver_registry",
                            "native driver identifiers and aliases must have one owner",
                        ));
                    }
                } else {
                    identifier_owners.insert(identifier, driver_id.clone());
                }
            }
        }
        Ok(Self {
            drivers: drivers.into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn descriptors(&self) -> impl Iterator<Item = &'static NativeDriverDescriptor> + '_ {
        self.drivers.iter().map(|driver| driver.descriptor())
    }

    pub(crate) fn standalone_descriptors(
        &self,
    ) -> impl Iterator<Item = &'static NativeDriverDescriptor> + '_ {
        self.drivers
            .iter()
            .filter(|driver| driver.connection().is_some())
            .map(|driver| driver.descriptor())
    }

    pub(crate) fn managed_jdbc_replacement_descriptors(
        &self,
    ) -> impl Iterator<Item = &'static NativeDriverDescriptor> + '_ {
        self.drivers
            .iter()
            .filter(|driver| driver.can_replace_managed_jdbc_datasource())
            .map(|driver| driver.descriptor())
    }

    /// Resolves a persisted datasource driver ID to its native implementation.
    pub(crate) fn driver_for_datasource_driver_id(
        &self,
        datasource_driver_id: &str,
    ) -> Option<Arc<dyn NativeDriver>> {
        let datasource_driver_id = datasource_driver_id.trim();
        self.drivers
            .iter()
            .find(|driver| {
                let descriptor = driver.descriptor();
                descriptor.id.eq_ignore_ascii_case(datasource_driver_id)
                    || descriptor
                        .compatibility_aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(datasource_driver_id))
            })
            .cloned()
    }

    pub(crate) fn driver_for_database_type(
        &self,
        database_type: &str,
    ) -> Option<Arc<dyn NativeDriver>> {
        self.drivers
            .iter()
            .find(|driver| {
                driver
                    .descriptor()
                    .database_types
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(database_type.trim()))
            })
            .cloned()
    }
}

impl Application {
    /// Lists databases through the runtime-selected Rust Driver SPI.
    ///
    /// # Errors
    ///
    /// Returns driver selection, capability, datasource, JDBC, or metadata errors.
    pub async fn list_native_databases(
        &self,
        database_type: &str,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError> {
        let driver = self.require_native_driver_capability(database_type)?;
        let metadata = driver
            .metadata()
            .ok_or_else(|| native_capability_not_supported(database_type, "database metadata"))?;
        metadata.list_databases(self, request).await
    }

    /// Lists schemas through the runtime-selected Rust Driver SPI.
    ///
    /// # Errors
    ///
    /// Returns driver selection, capability, datasource, JDBC, or metadata errors.
    pub async fn list_native_schemas(
        &self,
        database_type: &str,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError> {
        let driver = self.require_native_driver_capability(database_type)?;
        let metadata = driver
            .metadata()
            .ok_or_else(|| native_capability_not_supported(database_type, "schema metadata"))?;
        metadata.list_schemas(self, request).await
    }

    /// Lists tables through the runtime-selected Rust Driver SPI.
    ///
    /// # Errors
    ///
    /// Returns driver selection, capability, datasource, JDBC, or metadata errors.
    pub async fn list_native_tables(
        &self,
        database_type: &str,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError> {
        let driver = self.require_native_driver_capability(database_type)?;
        let metadata = driver
            .metadata()
            .ok_or_else(|| native_capability_not_supported(database_type, "table metadata"))?;
        metadata.list_tables(self, request).await
    }

    /// Lists table columns through the runtime-selected Rust Driver SPI.
    ///
    /// # Errors
    ///
    /// Returns driver selection, capability, datasource, JDBC, or metadata errors.
    pub async fn list_native_columns(
        &self,
        database_type: &str,
        request: ListColumnsRequest,
    ) -> Result<ColumnList, AppError> {
        let driver = self.require_native_driver_capability(database_type)?;
        let metadata = driver
            .metadata()
            .ok_or_else(|| native_capability_not_supported(database_type, "column metadata"))?;
        metadata.list_columns(self, request).await
    }

    /// Starts a bounded table preview through the runtime-selected Rust Driver SPI.
    ///
    /// # Errors
    ///
    /// Returns driver selection, capability, validation, datasource, or query errors.
    pub async fn start_native_table_preview(
        &self,
        database_type: &str,
        request: TablePreviewRequest,
        row_limit: u32,
    ) -> Result<TablePreviewAccepted, AppError> {
        let driver = self.require_native_driver_capability(database_type)?;
        let tables = driver
            .tables()
            .ok_or_else(|| native_capability_not_supported(database_type, "table preview"))?;
        tables.start_table_preview(self, request, row_limit).await
    }

    fn require_native_driver_capability(
        &self,
        database_type: &str,
    ) -> Result<Arc<dyn NativeDriver>, AppError> {
        self.native_driver_for_database_type(database_type)
            .ok_or_else(|| {
                AppError::invalid(
                    "native_driver_not_available",
                    format!("No Rust Driver SPI implementation is registered for {database_type}"),
                )
            })
    }
}

pub(crate) fn native_capability_not_supported(database_type: &str, capability: &str) -> AppError {
    AppError::invalid(
        "native_driver_capability_not_supported",
        format!("The {database_type} driver does not implement {capability}"),
    )
}

struct MysqlNativeDriver;
struct DmNativeDriver;

const MYSQL_DRIVER_DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
    id: "mysql",
    implementation: "mysql_async",
    database_types: &["MYSQL"],
    compatibility_aliases: &["mysql", "mysql_async", "com.mysql"],
};

impl NativeDriver for MysqlNativeDriver {
    fn descriptor(&self) -> &'static NativeDriverDescriptor {
        &MYSQL_DRIVER_DESCRIPTOR
    }

    fn can_replace_managed_jdbc_datasource(&self) -> bool {
        true
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

    fn routines(&self) -> Option<&dyn NativeRoutineDriver> {
        Some(self)
    }

    fn transfer(&self) -> Option<&dyn NativeTransferDriver> {
        Some(self)
    }

    fn dialect(&self) -> Option<&dyn NativeDialectDriver> {
        Some(self)
    }

    fn administration(&self) -> Option<&dyn NativeAdministrationDriver> {
        Some(self)
    }

    fn schema_diff(&self) -> Option<&dyn NativeSchemaDiffDriver> {
        Some(self)
    }
}

impl NativeDriver for DmNativeDriver {
    fn descriptor(&self) -> &'static NativeDriverDescriptor {
        &native_dm::DM_DRIVER_DESCRIPTOR
    }

    fn can_replace_managed_jdbc_datasource(&self) -> bool {
        true
    }

    fn metadata(&self) -> Option<&dyn NativeMetadataDriver> {
        Some(self)
    }

    fn tables(&self) -> Option<&dyn NativeTableDriver> {
        Some(self)
    }
}

#[async_trait]
impl NativeMetadataDriver for DmNativeDriver {
    async fn list_schemas(
        &self,
        application: &Application,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError> {
        native_dm::list_schemas(application, &request.datasource_id, &request.database_name).await
    }

    async fn list_databases(
        &self,
        application: &Application,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError> {
        native_dm::list_databases(application, &request.datasource_id).await
    }

    async fn list_tables(
        &self,
        application: &Application,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError> {
        native_dm::list_tables(
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
        native_dm::list_columns(
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
        _application: &Application,
        _request: ListIndexesRequest,
    ) -> Result<IndexList, AppError> {
        Err(dm_capability_not_supported("index metadata"))
    }

    async fn list_views(
        &self,
        _application: &Application,
        _request: ListViewsRequest,
    ) -> Result<ViewList, AppError> {
        Err(dm_capability_not_supported("view metadata"))
    }

    async fn get_view(
        &self,
        _application: &Application,
        _request: MetadataObjectRef,
    ) -> Result<TableMetadata, AppError> {
        Err(dm_capability_not_supported("view detail metadata"))
    }

    async fn list_imported_keys(
        &self,
        _application: &Application,
        _request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        Err(dm_capability_not_supported("imported key metadata"))
    }

    async fn list_exported_keys(
        &self,
        _application: &Application,
        _request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        Err(dm_capability_not_supported("exported key metadata"))
    }

    async fn list_primary_keys(
        &self,
        _application: &Application,
        _request: ListTableKeysRequest,
    ) -> Result<PrimaryKeyList, AppError> {
        Err(dm_capability_not_supported("primary key metadata"))
    }

    async fn list_functions(
        &self,
        _application: &Application,
        _request: ListRoutinesRequest,
    ) -> Result<FunctionList, AppError> {
        Err(dm_capability_not_supported("function metadata"))
    }

    async fn get_function(
        &self,
        _application: &Application,
        _request: MetadataObjectRef,
    ) -> Result<FunctionMetadata, AppError> {
        Err(dm_capability_not_supported("function detail metadata"))
    }

    async fn list_function_parameters(
        &self,
        _application: &Application,
        _request: MetadataObjectRef,
    ) -> Result<FunctionParameterList, AppError> {
        Err(dm_capability_not_supported("function parameter metadata"))
    }

    async fn list_procedures(
        &self,
        _application: &Application,
        _request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError> {
        Err(dm_capability_not_supported("procedure metadata"))
    }

    async fn get_procedure(
        &self,
        _application: &Application,
        _request: MetadataObjectRef,
    ) -> Result<ProcedureMetadata, AppError> {
        Err(dm_capability_not_supported("procedure detail metadata"))
    }

    async fn list_procedure_parameters(
        &self,
        _application: &Application,
        _request: MetadataObjectRef,
    ) -> Result<ProcedureParameterList, AppError> {
        Err(dm_capability_not_supported("procedure parameter metadata"))
    }

    async fn list_triggers(
        &self,
        _application: &Application,
        _request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError> {
        Err(dm_capability_not_supported("trigger metadata"))
    }

    async fn get_trigger(
        &self,
        _application: &Application,
        _request: MetadataObjectRef,
    ) -> Result<TriggerMetadata, AppError> {
        Err(dm_capability_not_supported("trigger detail metadata"))
    }
}

#[async_trait]
impl NativeTableDriver for DmNativeDriver {
    async fn load_er_tables(
        &self,
        _application: &Application,
        _datasource_id: &str,
        _database_name: &str,
        _schema_name: &str,
    ) -> Result<Vec<EntityRelationTable>, AppError> {
        Err(dm_capability_not_supported("entity relation metadata"))
    }

    async fn validate_column_reorder(
        &self,
        _application: &Application,
        _datasource_id: &str,
        _database_name: &str,
        _table_name: &str,
        _column_names: &[String],
    ) -> Result<(), AppError> {
        Err(dm_capability_not_supported("column reordering"))
    }

    async fn table_ddl(
        &self,
        _application: &Application,
        _datasource_id: &str,
        _database_name: &str,
        _schema_name: &str,
        _table_name: &str,
    ) -> Result<String, AppError> {
        Err(dm_capability_not_supported("table DDL"))
    }

    async fn start_table_preview(
        &self,
        application: &Application,
        request: TablePreviewRequest,
        row_limit: u32,
    ) -> Result<TablePreviewAccepted, AppError> {
        native_dm::start_table_preview(application, request, row_limit).await
    }
}

fn dm_capability_not_supported(capability: &'static str) -> AppError {
    AppError::invalid(
        "native_driver_capability_not_supported",
        format!("The DM driver does not implement {capability}"),
    )
}

#[async_trait]
impl NativeConnectionDriver for MysqlNativeDriver {
    async fn test_connection(&self, connection: &DatasourceConnection) -> Result<(), AppError> {
        native_mysql::test_connection(connection).await
    }

    async fn test_connection_with_local_port(
        &self,
        connection: &DatasourceConnection,
    ) -> Result<Option<u16>, AppError> {
        native_mysql::test_connection_with_local_port(connection).await
    }
}

#[async_trait]
impl NativeQueryDriver for MysqlNativeDriver {
    fn is_read_candidate(&self, sql: &str) -> Result<bool, AppError> {
        native_mysql::is_native_read_candidate(sql)
    }

    fn validate_query(&self, query: &PreparedQuery) -> Result<(), AppError> {
        native_mysql::validate_query(query)
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
        native_mysql::execute_query_task(
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
        native_mysql::execute_update(resolved, sql, cancellation).await
    }

    async fn execute_console(
        &self,
        application: &Application,
        request: NativeConsoleRequest,
        cancellation: watch::Receiver<CancellationRequest>,
        force_read_only: bool,
    ) -> Result<Vec<NativeConsoleResult>, AppError> {
        native_mysql::execute_console(application, request, cancellation, force_read_only).await
    }
}

#[async_trait]
impl NativeMetadataDriver for MysqlNativeDriver {
    async fn list_schemas(
        &self,
        application: &Application,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError> {
        native_mysql::list_schemas(application, &request.datasource_id).await
    }

    async fn list_databases(
        &self,
        application: &Application,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError> {
        native_mysql::list_databases(application, &request.datasource_id).await
    }

    async fn list_tables(
        &self,
        application: &Application,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError> {
        native_mysql::list_tables(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.name_pattern,
        )
        .await
    }

    async fn list_columns(
        &self,
        application: &Application,
        request: ListColumnsRequest,
    ) -> Result<ColumnList, AppError> {
        native_mysql::list_columns(
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
        native_mysql::list_indexes(
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
        native_mysql::list_views(
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
        native_mysql::get_view(
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
        native_mysql::list_imported_keys(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_exported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        native_mysql::list_exported_keys(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_primary_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<PrimaryKeyList, AppError> {
        native_mysql::list_primary_keys(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.scope.schema_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_functions(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<FunctionList, AppError> {
        native_mysql::list_functions(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
        )
        .await
    }

    async fn get_function(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionMetadata, AppError> {
        native_mysql::get_function(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionParameterList, AppError> {
        native_mysql::list_function_parameters(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }

    async fn list_procedures(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError> {
        native_mysql::list_procedures(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
        )
        .await
    }

    async fn get_procedure(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureMetadata, AppError> {
        native_mysql::get_procedure(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureParameterList, AppError> {
        native_mysql::list_procedure_parameters(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }

    async fn list_triggers(
        &self,
        application: &Application,
        request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError> {
        native_mysql::list_triggers(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
        )
        .await
    }

    async fn get_trigger(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TriggerMetadata, AppError> {
        native_mysql::get_trigger(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }
}

#[async_trait]
impl NativeTableDriver for MysqlNativeDriver {
    async fn load_er_tables(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Vec<EntityRelationTable>, AppError> {
        native_mysql::load_er_tables(application, datasource_id, database_name, schema_name).await
    }

    async fn validate_column_reorder(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        table_name: &str,
        column_names: &[String],
    ) -> Result<(), AppError> {
        native_mysql::validate_column_reorder(
            application,
            datasource_id,
            database_name,
            table_name,
            column_names,
        )
        .await
    }

    async fn table_ddl(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        native_mysql::table_ddl(
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
        native_mysql::start_table_preview(application, request, row_limit).await
    }
}

#[async_trait]
impl NativeRoutineDriver for MysqlNativeDriver {
    async fn preview_invocation(
        &self,
        application: &Application,
        request: RoutineInvocationRequest,
    ) -> Result<RoutineInvocationPreview, AppError> {
        native_mysql::preview_routine_invocation(application, request).await
    }

    fn preview_migration(
        &self,
        request: RoutineMigrationRequest,
    ) -> Result<RoutineInvocationPreview, AppError> {
        native_mysql::preview_routine_migration(&request)
    }

    async fn execute_migration(
        &self,
        application: &Application,
        request: RoutineMigrationRequest,
    ) -> Result<RoutineMigrationExecution, AppError> {
        native_mysql::execute_routine_migration(application, request).await
    }
}

#[async_trait]
impl NativeTransferDriver for MysqlNativeDriver {
    async fn import_file(
        &self,
        application: &Application,
        request: ImportFileRequest,
    ) -> Result<crate::transfer::TransferJobSpec, AppError> {
        crate::transfer::mysql_driver::import_file(application, request).await
    }

    async fn export_sql_file(
        &self,
        application: &Application,
        request: SqlFileExportRequest,
    ) -> Result<crate::transfer::TransferJobSpec, AppError> {
        crate::transfer::mysql_driver::export_sql_file(application, request).await
    }

    async fn export_table_file(
        &self,
        application: &Application,
        request: TableFileExportRequest,
    ) -> Result<crate::transfer::TransferJobSpec, AppError> {
        crate::transfer::mysql_driver::export_table_file(application, request).await
    }

    async fn export_query_result(
        &self,
        application: &Application,
        request: QueryResultExportRequest,
    ) -> Result<TransferArtifact, AppError> {
        crate::transfer::mysql_driver::export_query_result(application, request).await
    }
}

impl NativeDialectDriver for MysqlNativeDriver {
    fn build_create_schema(&self, request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
        crate::mysql_ddl::build_mysql_create_schema_request(request)
    }

    fn build_namespace_sql(&self, request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
        crate::mysql_ddl::build_mysql_namespace_request(request)
    }

    fn build_dml(&self, request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
        crate::mysql_ddl::build_mysql_dml_request(request)
    }
}

#[async_trait]
impl NativeAdministrationDriver for MysqlNativeDriver {
    async fn administration_capability(
        &self,
        application: &Application,
        datasource_id: &str,
    ) -> Result<AdministrationCapability, AppError> {
        crate::mysql_account::mysql_account_capability(application, datasource_id).await
    }

    async fn list_principals(
        &self,
        application: &Application,
        datasource_id: &str,
    ) -> Result<PrincipalList, AppError> {
        crate::mysql_account::list_mysql_accounts(application, datasource_id).await
    }

    async fn principal_grants(
        &self,
        application: &Application,
        request: &PrincipalGrantsRequest,
    ) -> Result<PrincipalGrantList, AppError> {
        crate::mysql_account::mysql_account_grants(application, request).await
    }

    fn preview_administration(
        &self,
        application: &Application,
        request: &AdministrationCommand,
    ) -> Result<AdministrationPreview, AppError> {
        crate::mysql_account::preview_mysql_account(application, request)
    }

    async fn execute_administration(
        &self,
        application: &Application,
        request: &AdministrationCommand,
    ) -> Result<AdministrationExecution, AppError> {
        crate::mysql_account::execute_mysql_account(application, request).await
    }
}

#[async_trait]
impl NativeSchemaDiffDriver for MysqlNativeDriver {
    async fn preview_schema_diff(
        &self,
        application: &Application,
        request: &SchemaDiffRequest,
    ) -> Result<SchemaDiffSql, AppError> {
        crate::mysql_schema_diff::preview_mysql_schema_diff(application, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_driver_types::NamespaceSqlOperation;

    #[test]
    fn native_spi_sources_do_not_depend_on_the_compatibility_wire_namespace() {
        let compatibility_prefix = ["Commu", "nity"].concat();
        let compatibility_identifier = ["commu", "nity_"].concat();
        for (path, source) in [
            ("native_driver.rs", include_str!("native_driver.rs")),
            (
                "native_driver_types.rs",
                include_str!("native_driver_types.rs"),
            ),
            ("native_mysql.rs", include_str!("native_mysql.rs")),
            ("native_oracle.rs", include_str!("native_oracle.rs")),
            ("native_postgres.rs", include_str!("native_postgres.rs")),
            ("native_sqlserver.rs", include_str!("native_sqlserver.rs")),
            (
                "native_administration_types.rs",
                include_str!("native_administration_types.rs"),
            ),
            (
                "native_schema_diff_types.rs",
                include_str!("native_schema_diff_types.rs"),
            ),
            (
                "transfer/class_generation.rs",
                include_str!("transfer/class_generation.rs"),
            ),
            (
                "transfer/mysql_driver.rs",
                include_str!("transfer/mysql_driver.rs"),
            ),
        ] {
            assert!(
                !source.contains(&compatibility_prefix)
                    && !source.contains(&compatibility_identifier),
                "{path} must stay independent from the compatibility wire namespace"
            );
        }
    }

    struct FakePostgresDriver;

    struct DescriptorOnlyDriver(&'static NativeDriverDescriptor);

    const FAKE_POSTGRES_DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
        id: "postgresql",
        implementation: "fake_postgres",
        database_types: &["POSTGRESQL", "POSTGRES"],
        compatibility_aliases: &["postgresql", "org.postgresql.Driver"],
    };

    impl NativeDriver for FakePostgresDriver {
        fn descriptor(&self) -> &'static NativeDriverDescriptor {
            &FAKE_POSTGRES_DESCRIPTOR
        }

        fn connection(&self) -> Option<&dyn NativeConnectionDriver> {
            Some(self)
        }

        fn dialect(&self) -> Option<&dyn NativeDialectDriver> {
            Some(self)
        }
    }

    impl NativeDriver for DescriptorOnlyDriver {
        fn descriptor(&self) -> &'static NativeDriverDescriptor {
            self.0
        }

        fn connection(&self) -> Option<&dyn NativeConnectionDriver> {
            Some(self)
        }
    }

    #[async_trait]
    impl NativeConnectionDriver for FakePostgresDriver {
        async fn test_connection(
            &self,
            _connection: &DatasourceConnection,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    #[async_trait]
    impl NativeConnectionDriver for DescriptorOnlyDriver {
        async fn test_connection(
            &self,
            _connection: &DatasourceConnection,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    impl NativeDialectDriver for FakePostgresDriver {
        fn build_create_schema(
            &self,
            request: CreateSchemaSqlRequest,
        ) -> Result<BuiltSql, AppError> {
            Ok(BuiltSql {
                sql: format!("fake-postgres:create-schema:{}", request.schema.name),
            })
        }

        fn build_namespace_sql(&self, _request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
            Ok(BuiltSql {
                sql: "fake-postgres:namespace".to_owned(),
            })
        }

        fn build_dml(&self, _request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
            Ok(BuiltSql {
                sql: "fake-postgres:dml".to_owned(),
            })
        }
    }

    #[test]
    fn registry_selects_runtime_driver_by_database_type_and_driver_id() {
        let registry = NativeDriverRegistry::try_new(vec![Arc::new(FakePostgresDriver)])
            .expect("registry is valid");

        assert_eq!(
            registry
                .driver_for_database_type("postgres")
                .expect("database type resolves")
                .descriptor()
                .id,
            "postgresql"
        );
        assert_eq!(
            registry
                .driver_for_datasource_driver_id("POSTGRESQL")
                .expect("driver id resolves")
                .descriptor()
                .implementation,
            "fake_postgres"
        );
        assert!(
            registry
                .driver_for_datasource_driver_id("org.postgresql.Driver")
                .is_some(),
            "persisted compatibility aliases must resolve to their native implementation"
        );
    }

    #[test]
    fn built_in_registry_exposes_every_owned_database_driver() {
        let registry = NativeDriverRegistry::built_in();
        let ids = registry
            .descriptors()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, ["mysql", "dm", "postgresql", "sqlserver", "oracle"]);
        let replacement_ids = registry
            .managed_jdbc_replacement_descriptors()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>();
        assert_eq!(replacement_ids, ["mysql", "dm"]);
    }

    #[test]
    fn application_dispatches_a_postgres_capability_through_the_registry() {
        let registry = NativeDriverRegistry::try_new(vec![Arc::new(FakePostgresDriver)])
            .expect("registry is valid");
        let application = Application::with_native_drivers_for_test(registry);

        let driver = application
            .native_driver_for_database_type("POSTGRESQL")
            .expect("application must resolve the fake PostgreSQL driver");
        let dialect = driver
            .dialect()
            .expect("fake PostgreSQL must expose its dialect capability");
        let built = dialect
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::UseDatabase {
                    database_name: "inventory".to_owned(),
                },
            })
            .expect("application must dispatch to the fake PostgreSQL capability");

        assert_eq!(built.sql, "fake-postgres:namespace");
    }

    #[test]
    fn registry_rejects_duplicate_database_type_ownership() {
        struct DuplicatePostgresDriver;

        impl NativeDriver for DuplicatePostgresDriver {
            fn descriptor(&self) -> &'static NativeDriverDescriptor {
                static DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
                    id: "duplicate-postgresql",
                    implementation: "duplicate",
                    database_types: &["postgresql"],
                    compatibility_aliases: &[],
                };
                &DESCRIPTOR
            }

            fn connection(&self) -> Option<&dyn NativeConnectionDriver> {
                Some(self)
            }
        }

        #[async_trait]
        impl NativeConnectionDriver for DuplicatePostgresDriver {
            async fn test_connection(
                &self,
                _connection: &DatasourceConnection,
            ) -> Result<(), AppError> {
                Ok(())
            }
        }

        let result = NativeDriverRegistry::try_new(vec![
            Arc::new(FakePostgresDriver),
            Arc::new(DuplicatePostgresDriver),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn registry_rejects_duplicate_driver_ids() {
        let result = NativeDriverRegistry::try_new(vec![
            Arc::new(FakePostgresDriver),
            Arc::new(FakePostgresDriver),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn registry_rejects_non_canonical_descriptor_text() {
        static DESCRIPTORS: [NativeDriverDescriptor; 4] = [
            NativeDriverDescriptor {
                id: " postgresql",
                implementation: "fake_postgres",
                database_types: &["POSTGRESQL"],
                compatibility_aliases: &[],
            },
            NativeDriverDescriptor {
                id: "postgresql",
                implementation: "fake_postgres ",
                database_types: &["POSTGRESQL"],
                compatibility_aliases: &[],
            },
            NativeDriverDescriptor {
                id: "postgresql",
                implementation: "fake_postgres",
                database_types: &[" POSTGRESQL"],
                compatibility_aliases: &[],
            },
            NativeDriverDescriptor {
                id: "postgresql",
                implementation: "fake_postgres",
                database_types: &["POSTGRESQL"],
                compatibility_aliases: &["org.postgresql.Driver "],
            },
        ];

        for descriptor in &DESCRIPTORS {
            let result =
                NativeDriverRegistry::try_new(vec![Arc::new(DescriptorOnlyDriver(descriptor))]);
            assert!(
                result.is_err(),
                "descriptor must be canonical: {descriptor:?}"
            );
        }
    }
}
