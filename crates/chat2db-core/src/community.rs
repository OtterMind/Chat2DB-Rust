use std::future::Future;

use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, CommunityBuiltSql, CommunityDatabase, CommunityDatabaseList,
    CommunityDriverConfig, CommunityForeignKey, CommunityForeignKeyList, CommunityParsedStatement,
    CommunityPlugin, CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices,
    CommunityPrimaryKey, CommunityPrimaryKeyList, CommunitySchema, CommunitySchemaList,
    CommunitySqlAnalysis, CommunityTable, CommunityTableColumn, CommunityTableColumnList,
    CommunityTableIndex, CommunityTableIndexColumn, CommunityTableIndexList, CommunityTableList,
    CommunityViewList, ListCommunityColumnsRequest, ListCommunityDatabasesRequest,
    ListCommunityIndexesRequest, ListCommunitySchemasRequest, ListCommunityTableKeysRequest,
    ListCommunityTablesRequest, ListCommunityViewsRequest, ParseCommunitySqlRequest,
};
use chat2db_java_bridge::{
    BridgeError, CommunityClasspath, CommunityDatabase as BridgeCommunityDatabase,
    CommunityDriverConfig as BridgeCommunityDriverConfig,
    CommunityForeignKey as BridgeCommunityForeignKey,
    CommunityParsedStatement as BridgeCommunityParsedStatement,
    CommunityPlugin as BridgeCommunityPlugin,
    CommunityPluginCatalog as BridgeCommunityPluginCatalog,
    CommunityPrimaryKey as BridgeCommunityPrimaryKey, CommunitySchema as BridgeCommunitySchema,
    CommunitySqlAnalysis as BridgeCommunitySqlAnalysis, CommunityTable as BridgeCommunityTable,
    CommunityTableColumn as BridgeCommunityTableColumn,
    CommunityTableIndex as BridgeCommunityTableIndex,
    CommunityTableIndexColumn as BridgeCommunityTableIndexColumn, EngineClient, Session,
};
use chat2db_storage::Storage;

const FIXED_COMMUNITY_CLASSPATH_LOCK: &str =
    include_str!("../../../third_party/community-h2-classpath.lock");

/// Loads the product's fixed Community 5.3.0 classpath only when every
/// filename, byte length, and SHA-256 digest matches the embedded lock.
///
/// # Errors
///
/// Returns an error when the directory or embedded lock is invalid or when
/// the artifact set differs from the fixed distribution inventory.
pub fn load_fixed_community_classpath(
    directory: impl AsRef<std::path::Path>,
) -> Result<CommunityClasspath, BridgeError> {
    CommunityClasspath::from_locked_directory(directory, FIXED_COMMUNITY_CLASSPATH_LOCK)
}

use crate::{
    AppError, Application,
    datasource_session::{SessionReadOnly, open_datasource_session, resolve_datasource_connection},
};

impl Application {
    /// Lists the plugins discovered from the fixed Community classpath.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, protocol, or Community
    /// discovery error.
    pub async fn list_community_plugins(&self) -> Result<CommunityPluginCatalog, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .list_plugins()
            .await
            .map(community_plugin_catalog)
            .map_err(AppError::from)
    }

    /// Lists schemas through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_schemas(
        &self,
        request: ListCommunitySchemasRequest,
    ) -> Result<CommunitySchemaList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunitySchemasRequest {
            datasource_id,
            database_type,
            database_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_schema_session",
            move |session| async move {
                client
                    .list_schemas(&session, database_type, database_name, None)
                    .await
                    .map(|schemas| CommunitySchemaList {
                        items: schemas.into_iter().map(community_schema).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists databases through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_databases(
        &self,
        request: ListCommunityDatabasesRequest,
    ) -> Result<CommunityDatabaseList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityDatabasesRequest {
            datasource_id,
            database_type,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_database_session",
            move |session| async move {
                client
                    .list_databases(&session, database_type, None)
                    .await
                    .map(|databases| CommunityDatabaseList {
                        items: databases.into_iter().map(community_database).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists tables through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_tables(
        &self,
        request: ListCommunityTablesRequest,
    ) -> Result<CommunityTableList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityTablesRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            table_name_pattern,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_table_session",
            move |session| async move {
                client
                    .list_tables(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        table_name_pattern,
                        None,
                    )
                    .await
                    .map(|tables| CommunityTableList {
                        items: tables.into_iter().map(community_table).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists columns through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_columns(
        &self,
        request: ListCommunityColumnsRequest,
    ) -> Result<CommunityTableColumnList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityColumnsRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            table_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_column_session",
            move |session| async move {
                client
                    .list_columns(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        table_name,
                        None,
                    )
                    .await
                    .map(|columns| CommunityTableColumnList {
                        items: columns.into_iter().map(community_table_column).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists indexes through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_indexes(
        &self,
        request: ListCommunityIndexesRequest,
    ) -> Result<CommunityTableIndexList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityIndexesRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            table_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_index_session",
            move |session| async move {
                client
                    .list_indexes(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        table_name,
                        None,
                    )
                    .await
                    .map(|indexes| CommunityTableIndexList {
                        items: indexes.into_iter().map(community_table_index).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists views through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_views(
        &self,
        request: ListCommunityViewsRequest,
    ) -> Result<CommunityViewList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityViewsRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            view_name_pattern,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_view_session",
            move |session| async move {
                client
                    .list_views(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        view_name_pattern,
                        None,
                    )
                    .await
                    .map(|views| CommunityViewList {
                        items: views.into_iter().map(community_table).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists foreign keys imported by one table using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_imported_keys(
        &self,
        request: ListCommunityTableKeysRequest,
    ) -> Result<CommunityForeignKeyList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityTableKeysRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            table_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_imported_key_session",
            move |session| async move {
                client
                    .list_imported_keys(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        table_name,
                        None,
                    )
                    .await
                    .map(|keys| CommunityForeignKeyList {
                        items: keys.into_iter().map(community_foreign_key).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists foreign keys exported by one table using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_exported_keys(
        &self,
        request: ListCommunityTableKeysRequest,
    ) -> Result<CommunityForeignKeyList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityTableKeysRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            table_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_exported_key_session",
            move |session| async move {
                client
                    .list_exported_keys(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        table_name,
                        None,
                    )
                    .await
                    .map(|keys| CommunityForeignKeyList {
                        items: keys.into_iter().map(community_foreign_key).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists primary-key columns for one table using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_primary_keys(
        &self,
        request: ListCommunityTableKeysRequest,
    ) -> Result<CommunityPrimaryKeyList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityTableKeysRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            table_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_primary_key_session",
            move |session| async move {
                client
                    .list_primary_keys(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        table_name,
                        None,
                    )
                    .await
                    .map(|keys| CommunityPrimaryKeyList {
                        items: keys.into_iter().map(community_primary_key).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Builds dialect-specific `CREATE SCHEMA` SQL through Community.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, validation, protocol, or
    /// Community SQL-builder error.
    pub async fn build_community_create_schema(
        &self,
        request: BuildCommunityCreateSchemaRequest,
    ) -> Result<CommunityBuiltSql, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .build_create_schema(request.database_type, bridge_schema(request.schema))
            .await
            .map(|sql| CommunityBuiltSql { sql })
            .map_err(AppError::from)
    }

    /// Parses SQL through the retained Community dialect parser.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, validation, protocol, or
    /// Community parser error.
    pub async fn parse_community_sql(
        &self,
        request: ParseCommunitySqlRequest,
    ) -> Result<CommunitySqlAnalysis, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .parse_sql(request.database_type, request.sql)
            .await
            .map(community_sql_analysis)
            .map_err(AppError::from)
    }

    fn require_community_engine(&self) -> Result<chat2db_java_bridge::EngineClient, AppError> {
        let engine = self.require_engine()?;
        if !engine.community_compatibility_configured() {
            return Err(AppError::unavailable(
                "community_compatibility_disabled",
                "The fixed Community compatibility classpath is not configured",
            ));
        }
        Ok(engine)
    }
}

async fn run_cancellation_safe<T, F>(operation: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: Future<Output = Result<T, AppError>> + Send + 'static,
{
    match tokio::spawn(operation).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(
                cancelled = error.is_cancelled(),
                panicked = error.is_panic(),
                "Community metadata task ended without a product result"
            );
            Err(AppError::internal())
        }
    }
}

async fn run_community_metadata_session<T, F, Fut>(
    storage: Storage,
    engine: EngineClient,
    datasource_id: String,
    cleanup_phase: &'static str,
    operation: F,
) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(Session) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, AppError>> + Send + 'static,
{
    run_cancellation_safe_with_cleanup(
        async move {
            let resolved = resolve_datasource_connection(&storage, &datasource_id).await?;
            open_datasource_session(&engine, resolved, SessionReadOnly::Forced).await
        },
        cleanup_phase,
        operation,
        |session| async move { session.close().await.map_err(AppError::from) },
    )
    .await
}

async fn run_cancellation_safe_with_cleanup<T, R, OpenFut, F, Fut, C, CleanupFut>(
    open: OpenFut,
    cleanup_phase: &'static str,
    operation: F,
    cleanup: C,
) -> Result<T, AppError>
where
    T: Send + 'static,
    R: Clone + Send + 'static,
    OpenFut: Future<Output = Result<R, AppError>> + Send + 'static,
    F: FnOnce(R) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, AppError>> + Send + 'static,
    C: FnOnce(R) -> CleanupFut + Send + 'static,
    CleanupFut: Future<Output = Result<(), AppError>> + Send + 'static,
{
    run_cancellation_safe(async move {
        let resource = open.await?;
        let outcome = operation(resource.clone()).await;
        let cleanup = cleanup(resource).await;
        preserve_primary_result(cleanup_phase, outcome, cleanup)
    })
    .await
}

fn community_plugin_catalog(catalog: BridgeCommunityPluginCatalog) -> CommunityPluginCatalog {
    CommunityPluginCatalog {
        source_commit: catalog.source_commit,
        plugins: catalog.plugins.into_iter().map(community_plugin).collect(),
    }
}

fn community_plugin(plugin: BridgeCommunityPlugin) -> CommunityPlugin {
    CommunityPlugin {
        database_type: plugin.database_type,
        name: plugin.name,
        behavior: CommunityPluginBehavior {
            supports_database: plugin.behavior.supports_database,
            supports_schema: plugin.behavior.supports_schema,
            preserves_script_batch_execution: plugin.behavior.preserves_script_batch_execution,
        },
        drivers: plugin
            .drivers
            .into_iter()
            .map(community_driver_config)
            .collect(),
        services: CommunityPluginServices {
            metadata_available: plugin.services.metadata_available,
            sql_builder_available: plugin.services.sql_builder_available,
            sql_parser_available: plugin.services.sql_parser_available,
        },
    }
}

fn community_driver_config(driver: BridgeCommunityDriverConfig) -> CommunityDriverConfig {
    CommunityDriverConfig {
        url: driver.url,
        jdbc_driver: driver.jdbc_driver,
        jdbc_driver_class: driver.jdbc_driver_class,
        download_urls: driver.download_urls,
        custom: driver.custom,
        default_driver: driver.default_driver,
    }
}

fn community_schema(schema: BridgeCommunitySchema) -> CommunitySchema {
    CommunitySchema {
        database_name: schema.database_name,
        name: schema.name,
        comment: schema.comment,
        owner: schema.owner,
        system: schema.system,
    }
}

fn community_database(database: BridgeCommunityDatabase) -> CommunityDatabase {
    CommunityDatabase {
        name: database.name,
        comment: database.comment,
        charset: database.charset,
        collation: database.collation,
        owner: database.owner,
        system: database.system,
    }
}

fn community_table(table: BridgeCommunityTable) -> CommunityTable {
    CommunityTable {
        database_name: table.database_name,
        schema_name: table.schema_name,
        name: table.name,
        table_type: table.table_type,
        comment: table.comment,
        database_type: table.database_type,
        pinned: table.pinned,
        ddl: table.ddl,
        engine: table.engine,
        charset: table.charset,
        collation: table.collation,
        increment_value: table.increment_value.map(|value| value.to_string()),
        partition: table.partition,
        tablespace: table.tablespace,
        rows: table.rows.map(|value| value.to_string()),
        data_length: table.data_length.map(|value| value.to_string()),
        create_time: table.create_time,
        update_time: table.update_time,
    }
}

fn community_table_column(column: BridgeCommunityTableColumn) -> CommunityTableColumn {
    CommunityTableColumn {
        database_name: column.database_name,
        schema_name: column.schema_name,
        table_name: column.table_name,
        name: column.name,
        column_type: column.column_type,
        data_type: column.data_type,
        default_value: column.default_value,
        auto_increment: column.auto_increment,
        comment: column.comment,
        primary_key: column.primary_key,
        primary_key_name: column.primary_key_name,
        primary_key_order: column.primary_key_order,
        column_size: column.column_size,
        buffer_length: column.buffer_length,
        decimal_digits: column.decimal_digits,
        num_prec_radix: column.num_prec_radix,
        sql_data_type: column.sql_data_type,
        sql_datetime_sub: column.sql_datetime_sub,
        char_octet_length: column.char_octet_length,
        ordinal_position: column.ordinal_position,
        nullable: column.nullable,
        generated_column: column.generated_column,
        extent: column.extent,
        charset: column.charset,
        collation: column.collation,
        unit: column.unit,
        sparse: column.sparse,
        default_constraint_name: column.default_constraint_name,
        seed: column.seed,
        increment: column.increment,
        on_update_current_timestamp: column.on_update_current_timestamp,
    }
}

fn community_table_index(index: BridgeCommunityTableIndex) -> CommunityTableIndex {
    CommunityTableIndex {
        database_name: index.database_name,
        schema_name: index.schema_name,
        table_name: index.table_name,
        name: index.name,
        index_type: index.index_type,
        unique: index.unique,
        comment: index.comment,
        columns: index
            .columns
            .into_iter()
            .map(community_table_index_column)
            .collect(),
        concurrently: index.concurrently,
        method: index.method,
        foreign_schema_name: index.foreign_schema_name,
        foreign_table_name: index.foreign_table_name,
        foreign_column_names: index.foreign_column_names,
    }
}

fn community_table_index_column(
    column: BridgeCommunityTableIndexColumn,
) -> CommunityTableIndexColumn {
    CommunityTableIndexColumn {
        database_name: column.database_name,
        schema_name: column.schema_name,
        table_name: column.table_name,
        index_name: column.index_name,
        column_name: column.column_name,
        column_type: column.column_type,
        comment: column.comment,
        ordinal_position: column.ordinal_position,
        collation: column.collation,
        non_unique: column.non_unique,
        index_qualifier: column.index_qualifier,
        sort_order: column.sort_order,
        cardinality: column.cardinality.map(|value| value.to_string()),
        pages: column.pages.map(|value| value.to_string()),
        filter_condition: column.filter_condition,
        sub_part: column.sub_part.map(|value| value.to_string()),
    }
}

fn community_foreign_key(key: BridgeCommunityForeignKey) -> CommunityForeignKey {
    CommunityForeignKey {
        primary_table_database: key.primary_table_database,
        primary_table_schema: key.primary_table_schema,
        primary_table_name: key.primary_table_name,
        primary_column_name: key.primary_column_name,
        foreign_table_database: key.foreign_table_database,
        foreign_table_schema: key.foreign_table_schema,
        foreign_table_name: key.foreign_table_name,
        foreign_column_name: key.foreign_column_name,
        key_sequence: key.key_sequence,
        update_rule: key.update_rule,
        delete_rule: key.delete_rule,
        foreign_key_name: key.foreign_key_name,
        primary_key_name: key.primary_key_name,
        deferrability: key.deferrability,
    }
}

fn community_primary_key(key: BridgeCommunityPrimaryKey) -> CommunityPrimaryKey {
    CommunityPrimaryKey {
        database_name: key.database_name,
        schema_name: key.schema_name,
        table_name: key.table_name,
        column_name: key.column_name,
        name: key.name,
    }
}

fn bridge_schema(schema: CommunitySchema) -> BridgeCommunitySchema {
    BridgeCommunitySchema {
        database_name: schema.database_name,
        name: schema.name,
        comment: schema.comment,
        owner: schema.owner,
        system: schema.system,
    }
}

fn community_sql_analysis(analysis: BridgeCommunitySqlAnalysis) -> CommunitySqlAnalysis {
    CommunitySqlAnalysis {
        is_select: analysis.is_select,
        statements: analysis
            .statements
            .into_iter()
            .map(community_parsed_statement)
            .collect(),
    }
}

fn community_parsed_statement(
    statement: BridgeCommunityParsedStatement,
) -> CommunityParsedStatement {
    CommunityParsedStatement {
        sql: statement.sql,
        statement_type: statement.statement_type,
        kind: statement.kind,
    }
}

fn preserve_primary_result<T>(
    cleanup_phase: &'static str,
    outcome: Result<T, AppError>,
    cleanup: Result<(), AppError>,
) -> Result<T, AppError> {
    match cleanup {
        Ok(()) => outcome,
        Err(cleanup_error) => match outcome {
            Ok(_) => Err(cleanup_error),
            Err(primary_error) => {
                tracing::warn!(
                    cleanup_phase,
                    cleanup_error = %cleanup_error,
                    "Community session cleanup failed after the primary outcome was determined"
                );
                Err(primary_error)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{
        CommunityDatabase, CommunityDriverConfig, CommunityForeignKey, CommunityParsedStatement,
        CommunityPlugin, CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices,
        CommunityPrimaryKey, CommunitySchema, CommunitySqlAnalysis, CommunityTable,
        CommunityTableColumn, CommunityTableIndex, CommunityTableIndexColumn,
        ListCommunityColumnsRequest, ListCommunityDatabasesRequest, ListCommunityIndexesRequest,
        ListCommunitySchemasRequest, ListCommunityTableKeysRequest, ListCommunityTablesRequest,
        ListCommunityViewsRequest,
    };
    use chat2db_java_bridge::{
        CommunityDatabase as BridgeCommunityDatabase,
        CommunityDriverConfig as BridgeCommunityDriverConfig,
        CommunityForeignKey as BridgeCommunityForeignKey,
        CommunityParsedStatement as BridgeCommunityParsedStatement,
        CommunityPlugin as BridgeCommunityPlugin,
        CommunityPluginBehavior as BridgeCommunityPluginBehavior,
        CommunityPluginCatalog as BridgeCommunityPluginCatalog,
        CommunityPluginServices as BridgeCommunityPluginServices,
        CommunityPrimaryKey as BridgeCommunityPrimaryKey, CommunitySchema as BridgeCommunitySchema,
        CommunitySqlAnalysis as BridgeCommunitySqlAnalysis, CommunityTable as BridgeCommunityTable,
        CommunityTableColumn as BridgeCommunityTableColumn,
        CommunityTableIndex as BridgeCommunityTableIndex,
        CommunityTableIndexColumn as BridgeCommunityTableIndexColumn,
    };
    use chat2db_storage::{EncryptedFileVault, Storage};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::{sync::oneshot, time};

    use super::{
        Application, bridge_schema, community_database, community_foreign_key,
        community_plugin_catalog, community_primary_key, community_schema, community_sql_analysis,
        community_table, community_table_column, community_table_index, preserve_primary_result,
        run_cancellation_safe, run_cancellation_safe_with_cleanup,
    };
    use crate::{AppError, AppErrorKind};

    #[test]
    fn bridge_plugin_catalog_mapping_preserves_every_field() {
        let bridge = BridgeCommunityPluginCatalog {
            source_commit: "f63cbf4".to_owned(),
            plugins: vec![BridgeCommunityPlugin {
                database_type: "H2".to_owned(),
                name: "H2 Database".to_owned(),
                behavior: BridgeCommunityPluginBehavior {
                    supports_database: true,
                    supports_schema: true,
                    preserves_script_batch_execution: false,
                },
                drivers: vec![BridgeCommunityDriverConfig {
                    url: "jdbc:h2:mem:test".to_owned(),
                    jdbc_driver: "h2.jar".to_owned(),
                    jdbc_driver_class: "org.h2.Driver".to_owned(),
                    download_urls: vec!["https://example.invalid/h2.jar".to_owned()],
                    custom: false,
                    default_driver: true,
                }],
                services: BridgeCommunityPluginServices {
                    metadata_available: true,
                    sql_builder_available: true,
                    sql_parser_available: true,
                },
            }],
        };
        let expected = CommunityPluginCatalog {
            source_commit: "f63cbf4".to_owned(),
            plugins: vec![CommunityPlugin {
                database_type: "H2".to_owned(),
                name: "H2 Database".to_owned(),
                behavior: CommunityPluginBehavior {
                    supports_database: true,
                    supports_schema: true,
                    preserves_script_batch_execution: false,
                },
                drivers: vec![CommunityDriverConfig {
                    url: "jdbc:h2:mem:test".to_owned(),
                    jdbc_driver: "h2.jar".to_owned(),
                    jdbc_driver_class: "org.h2.Driver".to_owned(),
                    download_urls: vec!["https://example.invalid/h2.jar".to_owned()],
                    custom: false,
                    default_driver: true,
                }],
                services: CommunityPluginServices {
                    metadata_available: true,
                    sql_builder_available: true,
                    sql_parser_available: true,
                },
            }],
        };

        assert_eq!(community_plugin_catalog(bridge), expected);
    }

    #[test]
    fn schema_mapping_preserves_every_field_in_both_directions() {
        let contract = CommunitySchema {
            database_name: "inventory".to_owned(),
            name: "reporting".to_owned(),
            comment: "Reporting objects".to_owned(),
            owner: "app".to_owned(),
            system: false,
        };
        let bridge = BridgeCommunitySchema {
            database_name: "inventory".to_owned(),
            name: "reporting".to_owned(),
            comment: "Reporting objects".to_owned(),
            owner: "app".to_owned(),
            system: false,
        };

        assert_eq!(bridge_schema(contract.clone()), bridge);
        assert_eq!(community_schema(bridge), contract);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn object_metadata_mapping_preserves_every_field() {
        assert_eq!(
            community_database(BridgeCommunityDatabase {
                name: "inventory".to_owned(),
                comment: "Inventory catalog".to_owned(),
                charset: "UTF-8".to_owned(),
                collation: "en_US".to_owned(),
                owner: "app".to_owned(),
                system: true,
            }),
            CommunityDatabase {
                name: "inventory".to_owned(),
                comment: "Inventory catalog".to_owned(),
                charset: "UTF-8".to_owned(),
                collation: "en_US".to_owned(),
                owner: "app".to_owned(),
                system: true,
            }
        );

        assert_eq!(
            community_table(BridgeCommunityTable {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "items".to_owned(),
                table_type: "TABLE".to_owned(),
                comment: "Inventory items".to_owned(),
                database_type: "H2".to_owned(),
                pinned: true,
                ddl: "CREATE TABLE APP.items (...)".to_owned(),
                engine: "MVStore".to_owned(),
                charset: "UTF-8".to_owned(),
                collation: "en_US".to_owned(),
                increment_value: Some(9_007_199_254_740_993),
                partition: "HASH(id)".to_owned(),
                tablespace: "main".to_owned(),
                rows: Some(9_007_199_254_740_994),
                data_length: Some(i64::MAX),
                create_time: "created".to_owned(),
                update_time: "updated".to_owned(),
            }),
            CommunityTable {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "items".to_owned(),
                table_type: "TABLE".to_owned(),
                comment: "Inventory items".to_owned(),
                database_type: "H2".to_owned(),
                pinned: true,
                ddl: "CREATE TABLE APP.items (...)".to_owned(),
                engine: "MVStore".to_owned(),
                charset: "UTF-8".to_owned(),
                collation: "en_US".to_owned(),
                increment_value: Some("9007199254740993".to_owned()),
                partition: "HASH(id)".to_owned(),
                tablespace: "main".to_owned(),
                rows: Some("9007199254740994".to_owned()),
                data_length: Some("9223372036854775807".to_owned()),
                create_time: "created".to_owned(),
                update_time: "updated".to_owned(),
            }
        );

        assert_eq!(
            community_table_column(BridgeCommunityTableColumn {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "items".to_owned(),
                name: "id".to_owned(),
                column_type: "BIGINT".to_owned(),
                data_type: Some(-5),
                default_value: "NEXT VALUE FOR seq_items".to_owned(),
                auto_increment: Some(true),
                comment: "Primary identifier".to_owned(),
                primary_key: Some(true),
                primary_key_name: "pk_items".to_owned(),
                primary_key_order: 1,
                column_size: Some(64),
                buffer_length: Some(8),
                decimal_digits: Some(0),
                num_prec_radix: Some(10),
                sql_data_type: Some(-5),
                sql_datetime_sub: Some(0),
                char_octet_length: Some(8),
                ordinal_position: Some(1),
                nullable: Some(0),
                generated_column: Some(false),
                extent: "8".to_owned(),
                charset: "UTF-8".to_owned(),
                collation: "en_US".to_owned(),
                unit: "bytes".to_owned(),
                sparse: Some(false),
                default_constraint_name: "df_items_id".to_owned(),
                seed: Some(1),
                increment: Some(2),
                on_update_current_timestamp: Some(false),
            }),
            CommunityTableColumn {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "items".to_owned(),
                name: "id".to_owned(),
                column_type: "BIGINT".to_owned(),
                data_type: Some(-5),
                default_value: "NEXT VALUE FOR seq_items".to_owned(),
                auto_increment: Some(true),
                comment: "Primary identifier".to_owned(),
                primary_key: Some(true),
                primary_key_name: "pk_items".to_owned(),
                primary_key_order: 1,
                column_size: Some(64),
                buffer_length: Some(8),
                decimal_digits: Some(0),
                num_prec_radix: Some(10),
                sql_data_type: Some(-5),
                sql_datetime_sub: Some(0),
                char_octet_length: Some(8),
                ordinal_position: Some(1),
                nullable: Some(0),
                generated_column: Some(false),
                extent: "8".to_owned(),
                charset: "UTF-8".to_owned(),
                collation: "en_US".to_owned(),
                unit: "bytes".to_owned(),
                sparse: Some(false),
                default_constraint_name: "df_items_id".to_owned(),
                seed: Some(1),
                increment: Some(2),
                on_update_current_timestamp: Some(false),
            }
        );

        let bridge_index_column = BridgeCommunityTableIndexColumn {
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
            index_name: "idx_items_label".to_owned(),
            column_name: "label".to_owned(),
            column_type: "VARCHAR".to_owned(),
            comment: "Indexed label".to_owned(),
            ordinal_position: Some(1),
            collation: "A".to_owned(),
            non_unique: Some(false),
            index_qualifier: "inventory".to_owned(),
            sort_order: "ASC".to_owned(),
            cardinality: Some(9_007_199_254_740_995),
            pages: Some(9_007_199_254_740_996),
            filter_condition: "label IS NOT NULL".to_owned(),
            sub_part: Some(9_007_199_254_740_997),
        };
        let contract_index_column = CommunityTableIndexColumn {
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
            index_name: "idx_items_label".to_owned(),
            column_name: "label".to_owned(),
            column_type: "VARCHAR".to_owned(),
            comment: "Indexed label".to_owned(),
            ordinal_position: Some(1),
            collation: "A".to_owned(),
            non_unique: Some(false),
            index_qualifier: "inventory".to_owned(),
            sort_order: "ASC".to_owned(),
            cardinality: Some("9007199254740995".to_owned()),
            pages: Some("9007199254740996".to_owned()),
            filter_condition: "label IS NOT NULL".to_owned(),
            sub_part: Some("9007199254740997".to_owned()),
        };
        assert_eq!(
            community_table_index(BridgeCommunityTableIndex {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "items".to_owned(),
                name: "idx_items_label".to_owned(),
                index_type: "BTREE".to_owned(),
                unique: Some(true),
                comment: "Unique item label".to_owned(),
                columns: vec![bridge_index_column],
                concurrently: Some(false),
                method: "BTREE".to_owned(),
                foreign_schema_name: "PUBLIC".to_owned(),
                foreign_table_name: "labels".to_owned(),
                foreign_column_names: vec!["id".to_owned()],
            }),
            CommunityTableIndex {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "items".to_owned(),
                name: "idx_items_label".to_owned(),
                index_type: "BTREE".to_owned(),
                unique: Some(true),
                comment: "Unique item label".to_owned(),
                columns: vec![contract_index_column],
                concurrently: Some(false),
                method: "BTREE".to_owned(),
                foreign_schema_name: "PUBLIC".to_owned(),
                foreign_table_name: "labels".to_owned(),
                foreign_column_names: vec!["id".to_owned()],
            }
        );
    }

    #[test]
    fn relation_metadata_mapping_preserves_every_field() {
        assert_eq!(
            community_foreign_key(BridgeCommunityForeignKey {
                primary_table_database: "inventory".to_owned(),
                primary_table_schema: "APP".to_owned(),
                primary_table_name: "parent".to_owned(),
                primary_column_name: "id".to_owned(),
                foreign_table_database: "inventory".to_owned(),
                foreign_table_schema: "APP".to_owned(),
                foreign_table_name: "child".to_owned(),
                foreign_column_name: "parent_id".to_owned(),
                key_sequence: 1,
                update_rule: 3,
                delete_rule: 1,
                foreign_key_name: "fk_child_parent".to_owned(),
                primary_key_name: "pk_parent".to_owned(),
                deferrability: 7,
            }),
            CommunityForeignKey {
                primary_table_database: "inventory".to_owned(),
                primary_table_schema: "APP".to_owned(),
                primary_table_name: "parent".to_owned(),
                primary_column_name: "id".to_owned(),
                foreign_table_database: "inventory".to_owned(),
                foreign_table_schema: "APP".to_owned(),
                foreign_table_name: "child".to_owned(),
                foreign_column_name: "parent_id".to_owned(),
                key_sequence: 1,
                update_rule: 3,
                delete_rule: 1,
                foreign_key_name: "fk_child_parent".to_owned(),
                primary_key_name: "pk_parent".to_owned(),
                deferrability: 7,
            }
        );
        assert_eq!(
            community_primary_key(BridgeCommunityPrimaryKey {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "parent".to_owned(),
                column_name: "id".to_owned(),
                name: "pk_parent".to_owned(),
            }),
            CommunityPrimaryKey {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "parent".to_owned(),
                column_name: "id".to_owned(),
                name: "pk_parent".to_owned(),
            }
        );
    }

    #[test]
    fn sql_analysis_mapping_preserves_every_field() {
        let analysis = BridgeCommunitySqlAnalysis {
            is_select: true,
            statements: vec![BridgeCommunityParsedStatement {
                sql: "select 1".to_owned(),
                statement_type: "SELECT".to_owned(),
                kind: "Select".to_owned(),
            }],
        };

        assert_eq!(
            community_sql_analysis(analysis),
            CommunitySqlAnalysis {
                is_select: true,
                statements: vec![CommunityParsedStatement {
                    sql: "select 1".to_owned(),
                    statement_type: "SELECT".to_owned(),
                    kind: "Select".to_owned(),
                }],
            }
        );
    }

    #[test]
    fn cleanup_success_preserves_the_primary_outcome() {
        assert_eq!(
            preserve_primary_result("test_cleanup", Ok(7), Ok(()))
                .expect("successful work remains successful"),
            7
        );
        let primary = AppError::invalid("primary_failure", "primary");
        assert_eq!(
            preserve_primary_result::<()>("test_cleanup", Err(primary.clone()), Ok(()))
                .expect_err("primary failure remains visible"),
            primary
        );
    }

    #[test]
    fn cleanup_failure_does_not_replace_a_primary_failure() {
        let primary = AppError::invalid("primary_failure", "primary");
        let cleanup = AppError::invalid("cleanup_failure", "cleanup");

        assert_eq!(
            preserve_primary_result::<()>("test_cleanup", Err(primary.clone()), Err(cleanup))
                .expect_err("primary failure remains visible"),
            primary
        );
    }

    #[test]
    fn cleanup_failure_fails_otherwise_successful_work() {
        let cleanup = AppError::invalid("cleanup_failure", "cleanup");

        assert_eq!(
            preserve_primary_result("test_cleanup", Ok(7), Err(cleanup.clone()))
                .expect_err("cleanup failure must be visible"),
            cleanup
        );
    }

    #[tokio::test]
    async fn cancellation_safe_work_finishes_after_its_waiter_is_aborted() {
        let (started_sender, started_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = oneshot::channel();
        let (finished_sender, finished_receiver) = oneshot::channel();
        let waiter = tokio::spawn(async move {
            run_cancellation_safe(async move {
                started_sender.send(()).expect("waiter must observe start");
                release_receiver.await.expect("test must release work");
                finished_sender
                    .send(())
                    .expect("test must observe detached completion");
                Ok::<(), AppError>(())
            })
            .await
        });

        started_receiver.await.expect("work must start");
        waiter.abort();
        assert!(
            waiter
                .await
                .expect_err("waiter must be aborted")
                .is_cancelled()
        );
        release_sender.send(()).expect("work must still be alive");
        time::timeout(std::time::Duration::from_secs(1), finished_receiver)
            .await
            .expect("detached work must finish promptly")
            .expect("detached work must report completion");
    }

    #[tokio::test]
    async fn cancellation_after_resource_open_still_runs_cleanup() {
        let (opened_sender, opened_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = oneshot::channel();
        let (closed_sender, closed_receiver) = oneshot::channel();
        let waiter = tokio::spawn(run_cancellation_safe_with_cleanup(
            async move {
                opened_sender.send(()).expect("test must observe open");
                Ok::<_, AppError>(())
            },
            "test_cleanup",
            move |()| async move {
                release_receiver.await.expect("test must release work");
                Ok::<_, AppError>(7)
            },
            move |()| async move {
                closed_sender.send(()).expect("test must observe cleanup");
                Ok::<_, AppError>(())
            },
        ));

        opened_receiver.await.expect("resource must open");
        waiter.abort();
        assert!(
            waiter
                .await
                .expect_err("waiter must be aborted")
                .is_cancelled()
        );
        release_sender
            .send(())
            .expect("detached operation must still be alive");
        time::timeout(std::time::Duration::from_secs(1), closed_receiver)
            .await
            .expect("cleanup must run promptly")
            .expect("cleanup must report completion");
    }

    #[tokio::test]
    async fn community_services_report_unconfigured_dependencies_safely() {
        let application = Application::new();

        let engine_error = application
            .list_community_plugins()
            .await
            .expect_err("plugin discovery requires the engine");
        assert_eq!(engine_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(engine_error.api_error().code, "database_engine_unavailable");

        let schema_storage_error = application
            .list_community_schemas(ListCommunitySchemasRequest {
                datasource_id: "datasource-1".to_owned(),
                database_type: "H2".to_owned(),
                database_name: "inventory".to_owned(),
            })
            .await
            .expect_err("schema metadata requires storage");
        assert_eq!(schema_storage_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(schema_storage_error.api_error().code, "storage_unavailable");

        let database_storage_error = application
            .list_community_databases(database_request())
            .await
            .expect_err("database metadata requires storage");
        assert_eq!(database_storage_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(
            database_storage_error.api_error().code,
            "storage_unavailable"
        );
        let table_storage_error = application
            .list_community_tables(table_request())
            .await
            .expect_err("table metadata requires storage");
        assert_eq!(table_storage_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(table_storage_error.api_error().code, "storage_unavailable");
        let column_storage_error = application
            .list_community_columns(column_request())
            .await
            .expect_err("column metadata requires storage");
        assert_eq!(column_storage_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(column_storage_error.api_error().code, "storage_unavailable");
        let index_storage_error = application
            .list_community_indexes(index_request())
            .await
            .expect_err("index metadata requires storage");
        assert_eq!(index_storage_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(index_storage_error.api_error().code, "storage_unavailable");
        let directory = TempDir::new().expect("temporary data directory must open");
        let vault = Arc::new(
            EncryptedFileVault::new(directory.path(), [0x4d; 32]).expect("test vault must open"),
        );
        let storage = Storage::open(directory.path(), vault).expect("test storage must open");
        let application = Application::with_storage(storage);
        let database_engine_error = application
            .list_community_databases(database_request())
            .await
            .expect_err("database metadata requires the engine");
        assert_eq!(database_engine_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(
            database_engine_error.api_error().code,
            "database_engine_unavailable"
        );
        let table_engine_error = application
            .list_community_tables(table_request())
            .await
            .expect_err("table metadata requires the engine");
        assert_eq!(table_engine_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(
            table_engine_error.api_error().code,
            "database_engine_unavailable"
        );
        let column_engine_error = application
            .list_community_columns(column_request())
            .await
            .expect_err("column metadata requires the engine");
        assert_eq!(column_engine_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(
            column_engine_error.api_error().code,
            "database_engine_unavailable"
        );
        let index_engine_error = application
            .list_community_indexes(index_request())
            .await
            .expect_err("index metadata requires the engine");
        assert_eq!(index_engine_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(
            index_engine_error.api_error().code,
            "database_engine_unavailable"
        );
    }

    #[tokio::test]
    async fn community_relation_services_report_unconfigured_dependencies_safely() {
        let application = Application::new();
        let view_error = application
            .list_community_views(view_request())
            .await
            .expect_err("view metadata requires storage");
        assert_unavailable(&view_error, "storage_unavailable");
        for error in [
            application
                .list_community_imported_keys(key_request())
                .await
                .expect_err("imported-key metadata requires storage"),
            application
                .list_community_exported_keys(key_request())
                .await
                .expect_err("exported-key metadata requires storage"),
            application
                .list_community_primary_keys(key_request())
                .await
                .expect_err("primary-key metadata requires storage"),
        ] {
            assert_unavailable(&error, "storage_unavailable");
        }

        let directory = TempDir::new().expect("temporary data directory must open");
        let vault = Arc::new(
            EncryptedFileVault::new(directory.path(), [0x5d; 32]).expect("test vault must open"),
        );
        let storage = Storage::open(directory.path(), vault).expect("test storage must open");
        let application = Application::with_storage(storage);
        let view_error = application
            .list_community_views(view_request())
            .await
            .expect_err("view metadata requires the engine");
        assert_unavailable(&view_error, "database_engine_unavailable");
        for error in [
            application
                .list_community_imported_keys(key_request())
                .await
                .expect_err("imported-key metadata requires the engine"),
            application
                .list_community_exported_keys(key_request())
                .await
                .expect_err("exported-key metadata requires the engine"),
            application
                .list_community_primary_keys(key_request())
                .await
                .expect_err("primary-key metadata requires the engine"),
        ] {
            assert_unavailable(&error, "database_engine_unavailable");
        }
    }

    fn assert_unavailable(error: &AppError, code: &str) {
        assert_eq!(error.kind(), AppErrorKind::Unavailable);
        assert_eq!(error.api_error().code, code);
    }

    fn database_request() -> ListCommunityDatabasesRequest {
        ListCommunityDatabasesRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
        }
    }

    fn table_request() -> ListCommunityTablesRequest {
        ListCommunityTablesRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name_pattern: "%".to_owned(),
        }
    }

    fn column_request() -> ListCommunityColumnsRequest {
        ListCommunityColumnsRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
        }
    }

    fn index_request() -> ListCommunityIndexesRequest {
        ListCommunityIndexesRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
        }
    }

    fn view_request() -> ListCommunityViewsRequest {
        ListCommunityViewsRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            view_name_pattern: "%".to_owned(),
        }
    }

    fn key_request() -> ListCommunityTableKeysRequest {
        ListCommunityTableKeysRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "child".to_owned(),
        }
    }
}
