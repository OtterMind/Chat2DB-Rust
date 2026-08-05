use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use chat2db_contract::{DatasourceConnection, JdbcDriver, ResultMetadata};
use chat2db_storage::Storage;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, Application,
    datasource_session::ResolvedDatasourceConnection,
    native_driver_types::{
        ColumnList, DatabaseList, EntityRelationTable, ForeignKeyList, FunctionList,
        FunctionMetadata, FunctionParameterList, IndexList, ListColumnsRequest,
        ListDatabasesRequest, ListIndexesRequest, ListRoutinesRequest, ListSchemasRequest,
        ListTableKeysRequest, ListTablesRequest, ListTriggersRequest, ListViewsRequest, ObjectRef,
        PrimaryKeyList, ProcedureList, ProcedureMetadata, ProcedureParameterList,
        RoutineInvocationPreview, RoutineInvocationRequest, RoutineMigrationExecution,
        RoutineMigrationRequest, SchemaList, TableList, TableMetadata, TablePreviewAccepted,
        TablePreviewRequest, TriggerList, TriggerMetadata, ViewList,
    },
    native_mysql,
    operation::CancellationRequest,
    query::{
        DatabaseWriteError, NativeConsoleRequest, NativeConsoleResult, PreparedQuery,
        QueryTaskError,
    },
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
        request: ObjectRef,
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
        request: ObjectRef,
    ) -> Result<FunctionMetadata, AppError>;

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: ObjectRef,
    ) -> Result<FunctionParameterList, AppError>;

    async fn list_procedures(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError>;

    async fn get_procedure(
        &self,
        application: &Application,
        request: ObjectRef,
    ) -> Result<ProcedureMetadata, AppError>;

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: ObjectRef,
    ) -> Result<ProcedureParameterList, AppError>;

    async fn list_triggers(
        &self,
        application: &Application,
        request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError>;

    async fn get_trigger(
        &self,
        application: &Application,
        request: ObjectRef,
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

/// Runtime-polymorphic native Rust database driver.
///
/// Optional capability accessors allow a driver to participate only in the
/// product surfaces it implements. Additional capability traits are attached
/// here as native metadata and dialect services are migrated.
pub(crate) trait NativeDriver: Send + Sync {
    fn id(&self) -> &'static str;

    fn implementation(&self) -> &'static str;

    fn database_types(&self) -> &'static [&'static str];

    fn descriptor(&self) -> JdbcDriver;

    fn matches_driver(&self, driver_id: &str, descriptor: Option<&JdbcDriver>) -> bool;

    fn connection(&self) -> &dyn NativeConnectionDriver;

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
}

/// Immutable registry used to select native implementations at runtime.
#[derive(Clone)]
pub(crate) struct NativeDriverRegistry {
    drivers: Arc<[Arc<dyn NativeDriver>]>,
}

impl NativeDriverRegistry {
    pub(crate) fn built_in() -> Self {
        Self::try_new(vec![Arc::new(MysqlNativeDriver)])
            .expect("built-in native drivers must have unique identities")
    }

    fn try_new(drivers: Vec<Arc<dyn NativeDriver>>) -> Result<Self, AppError> {
        let mut ids = HashSet::new();
        let mut database_types = HashSet::new();
        for driver in &drivers {
            let id = driver.id().trim().to_ascii_lowercase();
            if id.is_empty() || !ids.insert(id) {
                return Err(AppError::invalid(
                    "invalid_native_driver_registry",
                    "native driver ids must be non-empty and unique",
                ));
            }
            if driver.database_types().is_empty() {
                return Err(AppError::invalid(
                    "invalid_native_driver_registry",
                    "native drivers must declare at least one database type",
                ));
            }
            for database_type in driver.database_types() {
                let database_type = database_type.trim().to_ascii_lowercase();
                if database_type.is_empty() || !database_types.insert(database_type) {
                    return Err(AppError::invalid(
                        "invalid_native_driver_registry",
                        "native database types must be non-empty and unique",
                    ));
                }
            }
        }
        Ok(Self {
            drivers: drivers.into(),
        })
    }

    pub(crate) fn descriptors(&self) -> impl Iterator<Item = JdbcDriver> + '_ {
        self.drivers.iter().map(|driver| driver.descriptor())
    }

    pub(crate) fn driver_for_database_type(
        &self,
        database_type: &str,
    ) -> Option<Arc<dyn NativeDriver>> {
        self.drivers
            .iter()
            .find(|driver| {
                driver
                    .database_types()
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(database_type.trim()))
            })
            .cloned()
    }

    pub(crate) fn driver_for_driver_id(
        &self,
        driver_id: &str,
        managed_drivers: &[JdbcDriver],
    ) -> Option<Arc<dyn NativeDriver>> {
        let descriptor = managed_drivers
            .iter()
            .find(|driver| driver.driver_id == driver_id);
        self.drivers
            .iter()
            .find(|driver| driver.matches_driver(driver_id, descriptor))
            .cloned()
    }
}

struct MysqlNativeDriver;

impl NativeDriver for MysqlNativeDriver {
    fn id(&self) -> &'static str {
        "mysql"
    }

    fn implementation(&self) -> &'static str {
        "mysql_async"
    }

    fn database_types(&self) -> &'static [&'static str] {
        &["MYSQL"]
    }

    fn descriptor(&self) -> JdbcDriver {
        crate::datasource_compatibility::native_mysql_driver()
    }

    fn matches_driver(&self, driver_id: &str, descriptor: Option<&JdbcDriver>) -> bool {
        if driver_id.eq_ignore_ascii_case(self.id()) {
            return true;
        }
        descriptor.is_some_and(|driver| {
            format!(
                "{} {} {} {}",
                driver.pack_id, driver.name, driver.driver_id, driver.driver_class
            )
            .to_ascii_lowercase()
            .contains("mysql")
        })
    }

    fn connection(&self) -> &dyn NativeConnectionDriver {
        self
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
        request: ObjectRef,
    ) -> Result<TableMetadata, AppError> {
        native_mysql::get_view(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name,
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
        request: ObjectRef,
    ) -> Result<FunctionMetadata, AppError> {
        native_mysql::get_function(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name,
        )
        .await
    }

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: ObjectRef,
    ) -> Result<FunctionParameterList, AppError> {
        native_mysql::list_function_parameters(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name,
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
        request: ObjectRef,
    ) -> Result<ProcedureMetadata, AppError> {
        native_mysql::get_procedure(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name,
        )
        .await
    }

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: ObjectRef,
    ) -> Result<ProcedureParameterList, AppError> {
        native_mysql::list_procedure_parameters(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name,
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
        request: ObjectRef,
    ) -> Result<TriggerMetadata, AppError> {
        native_mysql::get_trigger(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name,
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePostgresDriver;

    impl NativeDriver for FakePostgresDriver {
        fn id(&self) -> &'static str {
            "postgresql"
        }

        fn implementation(&self) -> &'static str {
            "fake_postgres"
        }

        fn database_types(&self) -> &'static [&'static str] {
            &["POSTGRESQL", "POSTGRES"]
        }

        fn descriptor(&self) -> JdbcDriver {
            JdbcDriver {
                pack_id: "native:fake_postgres".to_owned(),
                name: "PostgreSQL (test native Rust)".to_owned(),
                version: "test".to_owned(),
                driver_id: self.id().to_owned(),
                driver_class: "rust:fake_postgres".to_owned(),
                artifact_count: 0,
                artifact_bytes: "0".to_owned(),
            }
        }

        fn matches_driver(&self, driver_id: &str, descriptor: Option<&JdbcDriver>) -> bool {
            driver_id.eq_ignore_ascii_case(self.id())
                || descriptor.is_some_and(|driver| {
                    driver
                        .driver_class
                        .eq_ignore_ascii_case("org.postgresql.Driver")
                })
        }

        fn connection(&self) -> &dyn NativeConnectionDriver {
            self
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

    #[test]
    fn registry_selects_runtime_driver_by_database_type_and_driver_id() {
        let registry = NativeDriverRegistry::try_new(vec![Arc::new(FakePostgresDriver)])
            .expect("registry is valid");

        assert_eq!(
            registry
                .driver_for_database_type("postgres")
                .expect("database type resolves")
                .id(),
            "postgresql"
        );
        assert_eq!(
            registry
                .driver_for_driver_id("POSTGRESQL", &[])
                .expect("driver id resolves")
                .implementation(),
            "fake_postgres"
        );
    }

    #[test]
    fn registry_uses_managed_descriptor_aliases_without_owning_driver_jars() {
        let registry = NativeDriverRegistry::try_new(vec![Arc::new(FakePostgresDriver)])
            .expect("registry is valid");
        let managed = vec![JdbcDriver {
            pack_id: "postgresql-42".to_owned(),
            name: "PostgreSQL JDBC".to_owned(),
            version: "42".to_owned(),
            driver_id: "managed-pg".to_owned(),
            driver_class: "org.postgresql.Driver".to_owned(),
            artifact_count: 1,
            artifact_bytes: "1".to_owned(),
        }];

        assert_eq!(
            registry
                .driver_for_driver_id("managed-pg", &managed)
                .expect("managed descriptor resolves")
                .id(),
            "postgresql"
        );
    }

    #[test]
    fn registry_rejects_duplicate_database_type_ownership() {
        struct DuplicatePostgresDriver;

        impl NativeDriver for DuplicatePostgresDriver {
            fn id(&self) -> &'static str {
                "duplicate-postgresql"
            }

            fn implementation(&self) -> &'static str {
                "duplicate"
            }

            fn database_types(&self) -> &'static [&'static str] {
                &["postgresql"]
            }

            fn descriptor(&self) -> JdbcDriver {
                FakePostgresDriver.descriptor()
            }

            fn matches_driver(&self, _driver_id: &str, _descriptor: Option<&JdbcDriver>) -> bool {
                false
            }

            fn connection(&self) -> &dyn NativeConnectionDriver {
                self
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
}
