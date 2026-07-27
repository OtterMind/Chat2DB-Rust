use std::future::Future;

use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, CommunityBuiltSql, CommunityDatabase, CommunityDatabaseList,
    CommunityDriverConfig, CommunityForeignKey, CommunityForeignKeyList, CommunityFormattedSql,
    CommunityFunction, CommunityFunctionList, CommunityFunctionParameter,
    CommunityFunctionParameterList, CommunityParsedStatement, CommunityPlugin,
    CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices, CommunityPrimaryKey,
    CommunityPrimaryKeyList, CommunityProcedure, CommunityProcedureList,
    CommunityProcedureParameter, CommunityProcedureParameterList, CommunitySchema,
    CommunitySchemaList, CommunitySqlAnalysis, CommunitySqlCompletion,
    CommunitySqlCompletionActiveSnippetSlot, CommunitySqlCompletionCandidate,
    CommunitySqlCompletionEditorHint, CommunitySqlCompletionEditorHintItem,
    CommunitySqlCompletionRange, CommunitySqlDiagnostic, CommunitySqlValidation, CommunityTable,
    CommunityTableColumn, CommunityTableColumnList, CommunityTableIndex, CommunityTableIndexColumn,
    CommunityTableIndexList, CommunityTableList, CommunityTrigger, CommunityTriggerList,
    CommunityViewList, CompleteCommunitySqlRequest, FormatCommunitySqlRequest,
    GetCommunityFunctionRequest, GetCommunityProcedureRequest, GetCommunityTriggerRequest,
    ListCommunityColumnsRequest, ListCommunityDatabasesRequest, ListCommunityFunctionsRequest,
    ListCommunityIndexesRequest, ListCommunityProceduresRequest, ListCommunitySchemasRequest,
    ListCommunityTableKeysRequest, ListCommunityTablesRequest, ListCommunityTriggersRequest,
    ListCommunityViewsRequest, ParseCommunitySqlRequest, ValidateCommunitySqlRequest,
};
use chat2db_java_bridge::{
    BridgeError, CommunityClasspath, CommunityDatabase as BridgeCommunityDatabase,
    CommunityDriverConfig as BridgeCommunityDriverConfig,
    CommunityForeignKey as BridgeCommunityForeignKey,
    CommunityFormattedSql as BridgeCommunityFormattedSql,
    CommunityFunction as BridgeCommunityFunction,
    CommunityFunctionParameter as BridgeCommunityFunctionParameter,
    CommunityParsedStatement as BridgeCommunityParsedStatement,
    CommunityPlugin as BridgeCommunityPlugin,
    CommunityPluginCatalog as BridgeCommunityPluginCatalog,
    CommunityPrimaryKey as BridgeCommunityPrimaryKey,
    CommunityProcedure as BridgeCommunityProcedure,
    CommunityProcedureParameter as BridgeCommunityProcedureParameter,
    CommunitySchema as BridgeCommunitySchema, CommunitySqlAnalysis as BridgeCommunitySqlAnalysis,
    CommunitySqlCompletion as BridgeCommunitySqlCompletion,
    CommunitySqlCompletionActiveSnippetSlot as BridgeCommunitySqlCompletionActiveSnippetSlot,
    CommunitySqlCompletionCandidate as BridgeCommunitySqlCompletionCandidate,
    CommunitySqlCompletionEditorHint as BridgeCommunitySqlCompletionEditorHint,
    CommunitySqlCompletionEditorHintItem as BridgeCommunitySqlCompletionEditorHintItem,
    CommunitySqlCompletionRange as BridgeCommunitySqlCompletionRange,
    CommunitySqlDiagnostic as BridgeCommunitySqlDiagnostic,
    CommunitySqlValidation as BridgeCommunitySqlValidation, CommunityTable as BridgeCommunityTable,
    CommunityTableColumn as BridgeCommunityTableColumn,
    CommunityTableIndex as BridgeCommunityTableIndex,
    CommunityTableIndexColumn as BridgeCommunityTableIndexColumn,
    CommunityTrigger as BridgeCommunityTrigger,
    CompleteCommunitySqlRequest as BridgeCompleteCommunitySqlRequest, EngineClient, Session,
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

    /// Lists functions through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_functions(
        &self,
        request: ListCommunityFunctionsRequest,
    ) -> Result<CommunityFunctionList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityFunctionsRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_function_list_session",
            move |session| async move {
                client
                    .list_functions(&session, database_type, database_name, schema_name, None)
                    .await
                    .map(|items| CommunityFunctionList {
                        items: items.into_iter().map(community_function).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Reads one function through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn get_community_function(
        &self,
        request: GetCommunityFunctionRequest,
    ) -> Result<CommunityFunction, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let GetCommunityFunctionRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            function_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_function_session",
            move |session| async move {
                client
                    .get_function(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        function_name,
                        None,
                    )
                    .await
                    .map(community_function)
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists one function's parameters using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_function_parameters(
        &self,
        request: GetCommunityFunctionRequest,
    ) -> Result<CommunityFunctionParameterList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let GetCommunityFunctionRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            function_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_function_parameter_session",
            move |session| async move {
                client
                    .list_function_parameters(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        function_name,
                        None,
                    )
                    .await
                    .map(|items| CommunityFunctionParameterList {
                        items: items
                            .into_iter()
                            .map(community_function_parameter)
                            .collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists procedures through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_procedures(
        &self,
        request: ListCommunityProceduresRequest,
    ) -> Result<CommunityProcedureList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityProceduresRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_procedure_list_session",
            move |session| async move {
                client
                    .list_procedures(&session, database_type, database_name, schema_name, None)
                    .await
                    .map(|items| CommunityProcedureList {
                        items: items.into_iter().map(community_procedure).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Reads one procedure through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn get_community_procedure(
        &self,
        request: GetCommunityProcedureRequest,
    ) -> Result<CommunityProcedure, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let GetCommunityProcedureRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            procedure_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_procedure_session",
            move |session| async move {
                client
                    .get_procedure(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        procedure_name,
                        None,
                    )
                    .await
                    .map(community_procedure)
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists one procedure's parameters using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_procedure_parameters(
        &self,
        request: GetCommunityProcedureRequest,
    ) -> Result<CommunityProcedureParameterList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let GetCommunityProcedureRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            procedure_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_procedure_parameter_session",
            move |session| async move {
                client
                    .list_procedure_parameters(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        procedure_name,
                        None,
                    )
                    .await
                    .map(|items| CommunityProcedureParameterList {
                        items: items
                            .into_iter()
                            .map(community_procedure_parameter)
                            .collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Lists triggers through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_triggers(
        &self,
        request: ListCommunityTriggersRequest,
    ) -> Result<CommunityTriggerList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunityTriggersRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_trigger_list_session",
            move |session| async move {
                client
                    .list_triggers(&session, database_type, database_name, schema_name, None)
                    .await
                    .map(|items| CommunityTriggerList {
                        items: items.into_iter().map(community_trigger).collect(),
                    })
                    .map_err(AppError::from)
            },
        )
        .await
    }

    /// Reads one trigger through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn get_community_trigger(
        &self,
        request: GetCommunityTriggerRequest,
    ) -> Result<CommunityTrigger, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let GetCommunityTriggerRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            trigger_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_trigger_session",
            move |session| async move {
                client
                    .get_trigger(
                        &session,
                        database_type,
                        database_name,
                        schema_name,
                        trigger_name,
                        None,
                    )
                    .await
                    .map(community_trigger)
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

    /// Validates SQL through the retained Community dialect parser.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, validation, protocol, or
    /// Community parser error.
    pub async fn validate_community_sql(
        &self,
        request: ValidateCommunitySqlRequest,
    ) -> Result<CommunitySqlValidation, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .validate_sql(request.database_type, request.sql)
            .await
            .map(community_sql_validation)
            .map_err(AppError::from)
    }

    /// Formats SQL through the retained Community dialect formatter.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, validation, protocol, or
    /// Community formatter error.
    pub async fn format_community_sql(
        &self,
        request: FormatCommunitySqlRequest,
    ) -> Result<CommunityFormattedSql, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .format_sql(request.database_type, request.sql)
            .await
            .map(community_formatted_sql)
            .map_err(AppError::from)
    }

    /// Completes SQL through Community against a forced read-only datasource session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, completion, protocol, or
    /// session-cleanup errors.
    pub async fn complete_community_sql(
        &self,
        request: CompleteCommunitySqlRequest,
    ) -> Result<CommunitySqlCompletion, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let CompleteCommunitySqlRequest {
            datasource_id,
            database_type,
            database_name,
            schema_name,
            sql,
            cursor_utf16,
            min_prefix_length,
            need_full_name,
            keyword_case,
            active_snippet_slot,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_community_named_metadata_session(
            storage,
            engine,
            datasource_id,
            "close_community_sql_completion_session",
            move |session, datasource_name| async move {
                client
                    .complete_sql(
                        &session,
                        BridgeCompleteCommunitySqlRequest {
                            database_type,
                            database_name,
                            schema_name,
                            datasource_name,
                            sql,
                            cursor_utf16,
                            min_prefix_length,
                            need_full_name,
                            keyword_case,
                            active_snippet_slot: active_snippet_slot
                                .map(bridge_completion_active_snippet_slot),
                            transaction_id: None,
                        },
                    )
                    .await
                    .map(community_sql_completion)
                    .map_err(AppError::from)
            },
        )
        .await
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

async fn run_community_named_metadata_session<T, F, Fut>(
    storage: Storage,
    engine: EngineClient,
    datasource_id: String,
    cleanup_phase: &'static str,
    operation: F,
) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(Session, String) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, AppError>> + Send + 'static,
{
    run_cancellation_safe_with_cleanup(
        async move {
            let resolved = resolve_datasource_connection(&storage, &datasource_id).await?;
            let datasource_name = resolved.datasource_name.clone();
            let session =
                open_datasource_session(&engine, resolved, SessionReadOnly::Forced).await?;
            Ok((session, datasource_name))
        },
        cleanup_phase,
        move |(session, datasource_name)| operation(session, datasource_name),
        |(session, _)| async move { session.close().await.map_err(AppError::from) },
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

fn community_function(function: BridgeCommunityFunction) -> CommunityFunction {
    CommunityFunction {
        database_name: function.database_name,
        schema_name: function.schema_name,
        name: function.name,
        remarks: function.remarks,
        function_type: function.function_type,
        specific_name: function.specific_name,
        body: function.body,
        template: function.template,
    }
}

fn community_function_parameter(
    parameter: BridgeCommunityFunctionParameter,
) -> CommunityFunctionParameter {
    CommunityFunctionParameter {
        function_database: parameter.function_database,
        function_schema: parameter.function_schema,
        function_name: parameter.function_name,
        column_name: parameter.column_name,
        column_type: parameter.column_type,
        data_type: parameter.data_type,
        type_name: parameter.type_name,
        precision: parameter.precision,
        length: parameter.length,
        scale: parameter.scale,
        radix: parameter.radix,
        nullable: parameter.nullable,
        remarks: parameter.remarks,
        char_octet_length: parameter.char_octet_length,
        ordinal_position: parameter.ordinal_position,
        is_nullable: parameter.is_nullable,
        specific_name: parameter.specific_name,
    }
}

fn community_procedure(procedure: BridgeCommunityProcedure) -> CommunityProcedure {
    CommunityProcedure {
        database_name: procedure.database_name,
        schema_name: procedure.schema_name,
        name: procedure.name,
        remarks: procedure.remarks,
        procedure_type: procedure.procedure_type,
        specific_name: procedure.specific_name,
        body: procedure.body,
    }
}

fn community_procedure_parameter(
    parameter: BridgeCommunityProcedureParameter,
) -> CommunityProcedureParameter {
    CommunityProcedureParameter {
        procedure_database: parameter.procedure_database,
        procedure_schema: parameter.procedure_schema,
        procedure_name: parameter.procedure_name,
        column_name: parameter.column_name,
        column_type: parameter.column_type,
        data_type: parameter.data_type,
        type_name: parameter.type_name,
        precision: parameter.precision,
        length: parameter.length,
        scale: parameter.scale,
        radix: parameter.radix,
        nullable: parameter.nullable,
        remarks: parameter.remarks,
        column_default: parameter.column_default,
        sql_data_type: parameter.sql_data_type,
        sql_datetime_sub: parameter.sql_datetime_sub,
        char_octet_length: parameter.char_octet_length,
        ordinal_position: parameter.ordinal_position,
        is_nullable: parameter.is_nullable,
        specific_name: parameter.specific_name,
    }
}

fn community_trigger(trigger: BridgeCommunityTrigger) -> CommunityTrigger {
    CommunityTrigger {
        database_name: trigger.database_name,
        schema_name: trigger.schema_name,
        name: trigger.name,
        event_manipulation: trigger.event_manipulation,
        body: trigger.body,
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

fn community_sql_validation(validation: BridgeCommunitySqlValidation) -> CommunitySqlValidation {
    CommunitySqlValidation {
        valid: validation.valid,
        statements: validation
            .statements
            .into_iter()
            .map(community_parsed_statement)
            .collect(),
        diagnostics: validation
            .diagnostics
            .into_iter()
            .map(community_sql_diagnostic)
            .collect(),
    }
}

fn community_sql_diagnostic(diagnostic: BridgeCommunitySqlDiagnostic) -> CommunitySqlDiagnostic {
    CommunitySqlDiagnostic {
        start_line: diagnostic.start_line,
        start_column: diagnostic.start_column,
        end_line: diagnostic.end_line,
        end_column: diagnostic.end_column,
        token_text: diagnostic.token_text,
        message: diagnostic.message,
    }
}

fn community_formatted_sql(formatted: BridgeCommunityFormattedSql) -> CommunityFormattedSql {
    CommunityFormattedSql { sql: formatted.sql }
}

fn bridge_completion_active_snippet_slot(
    slot: CommunitySqlCompletionActiveSnippetSlot,
) -> BridgeCommunitySqlCompletionActiveSnippetSlot {
    BridgeCommunitySqlCompletionActiveSnippetSlot {
        slot_type: slot.r#type,
        replace_start_utf16: slot.replace_start_utf16,
        replace_end_utf16: slot.replace_end_utf16,
    }
}

fn community_sql_completion(completion: BridgeCommunitySqlCompletion) -> CommunitySqlCompletion {
    CommunitySqlCompletion {
        status: completion.status.to_ascii_lowercase(),
        replace_start_utf16: completion.replace_start_utf16,
        replace_end_utf16: completion.replace_end_utf16,
        candidates: completion
            .candidates
            .into_iter()
            .map(community_sql_completion_candidate)
            .collect(),
        editor_hints: completion
            .editor_hints
            .into_iter()
            .map(community_sql_completion_editor_hint)
            .collect(),
        reason_code: completion.reason_code,
    }
}

fn community_sql_completion_candidate(
    candidate: BridgeCommunitySqlCompletionCandidate,
) -> CommunitySqlCompletionCandidate {
    CommunitySqlCompletionCandidate {
        id: candidate.id,
        label: candidate.label,
        r#type: candidate.candidate_type,
        insert_text: candidate.insert_text,
        insert_type: candidate.insert_type,
        replace_start_utf16: candidate.replace_start_utf16,
        replace_end_utf16: candidate.replace_end_utf16,
        detail: candidate.detail,
        description: candidate.description,
        data_type: candidate.data_type,
        object_type: candidate.object_type,
        comment: candidate.comment,
        datasource_name: candidate.datasource_name,
        database_name: candidate.database_name,
        schema_name: candidate.schema_name,
        table_name: candidate.table_name,
        table_alias: candidate.table_alias,
        column_name: candidate.column_name,
        object_name: candidate.object_name,
        parameter_mode: candidate.parameter_mode,
        sort_rank: candidate.sort_rank,
        sort_text: candidate.sort_text,
        snippet_slots: candidate.snippet_slots,
    }
}

fn community_sql_completion_editor_hint(
    hint: BridgeCommunitySqlCompletionEditorHint,
) -> CommunitySqlCompletionEditorHint {
    CommunitySqlCompletionEditorHint {
        r#type: hint.hint_type,
        statement_range: hint
            .statement_range
            .as_ref()
            .map(community_sql_completion_range),
        row_range: hint.row_range.as_ref().map(community_sql_completion_range),
        value_range: hint
            .value_range
            .as_ref()
            .map(community_sql_completion_range),
        items: hint
            .items
            .into_iter()
            .map(community_sql_completion_editor_hint_item)
            .collect(),
    }
}

fn community_sql_completion_editor_hint_item(
    item: BridgeCommunitySqlCompletionEditorHintItem,
) -> CommunitySqlCompletionEditorHintItem {
    CommunitySqlCompletionEditorHintItem {
        row_index: item.row_index,
        column_index: item.column_index,
        field_name: item.field_name,
        field_type: item.field_type,
        label: item.label,
        range: item.range.as_ref().map(community_sql_completion_range),
        active: item.active,
    }
}

fn community_sql_completion_range(
    range: &BridgeCommunitySqlCompletionRange,
) -> CommunitySqlCompletionRange {
    CommunitySqlCompletionRange {
        start_line_number: range.start_line_number,
        start_column: range.start_column,
        end_line_number: range.end_line_number,
        end_column: range.end_column,
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
        CommunityDatabase, CommunityDriverConfig, CommunityForeignKey, CommunityFormattedSql,
        CommunityFunction, CommunityFunctionParameter, CommunityParsedStatement, CommunityPlugin,
        CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices,
        CommunityPrimaryKey, CommunityProcedure, CommunityProcedureParameter, CommunitySchema,
        CommunitySqlAnalysis, CommunitySqlDiagnostic, CommunitySqlValidation, CommunityTable,
        CommunityTableColumn, CommunityTableIndex, CommunityTableIndexColumn, CommunityTrigger,
        ListCommunityColumnsRequest, ListCommunityDatabasesRequest, ListCommunityIndexesRequest,
        ListCommunitySchemasRequest, ListCommunityTableKeysRequest, ListCommunityTablesRequest,
        ListCommunityViewsRequest,
    };
    use chat2db_java_bridge::{
        CommunityDatabase as BridgeCommunityDatabase,
        CommunityDriverConfig as BridgeCommunityDriverConfig,
        CommunityForeignKey as BridgeCommunityForeignKey,
        CommunityFormattedSql as BridgeCommunityFormattedSql,
        CommunityFunction as BridgeCommunityFunction,
        CommunityFunctionParameter as BridgeCommunityFunctionParameter,
        CommunityParsedStatement as BridgeCommunityParsedStatement,
        CommunityPlugin as BridgeCommunityPlugin,
        CommunityPluginBehavior as BridgeCommunityPluginBehavior,
        CommunityPluginCatalog as BridgeCommunityPluginCatalog,
        CommunityPluginServices as BridgeCommunityPluginServices,
        CommunityPrimaryKey as BridgeCommunityPrimaryKey,
        CommunityProcedure as BridgeCommunityProcedure,
        CommunityProcedureParameter as BridgeCommunityProcedureParameter,
        CommunitySchema as BridgeCommunitySchema,
        CommunitySqlAnalysis as BridgeCommunitySqlAnalysis,
        CommunitySqlDiagnostic as BridgeCommunitySqlDiagnostic,
        CommunitySqlValidation as BridgeCommunitySqlValidation,
        CommunityTable as BridgeCommunityTable, CommunityTableColumn as BridgeCommunityTableColumn,
        CommunityTableIndex as BridgeCommunityTableIndex,
        CommunityTableIndexColumn as BridgeCommunityTableIndexColumn,
        CommunityTrigger as BridgeCommunityTrigger,
    };
    use chat2db_storage::{EncryptedFileVault, Storage};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::{sync::oneshot, time};

    use super::{
        Application, bridge_schema, community_database, community_foreign_key,
        community_formatted_sql, community_function, community_function_parameter,
        community_plugin_catalog, community_primary_key, community_procedure,
        community_procedure_parameter, community_schema, community_sql_analysis,
        community_sql_validation, community_table, community_table_column, community_table_index,
        community_trigger, preserve_primary_result, run_cancellation_safe,
        run_cancellation_safe_with_cleanup,
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
    #[allow(clippy::too_many_lines)]
    fn programmability_metadata_mapping_preserves_every_field() {
        assert_eq!(
            community_function(BridgeCommunityFunction {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "double_value".to_owned(),
                remarks: "Doubles a value".to_owned(),
                function_type: Some(1),
                specific_name: "double_value_1".to_owned(),
                body: "return value * 2".to_owned(),
                template: "double_value(?)".to_owned(),
            }),
            CommunityFunction {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "double_value".to_owned(),
                remarks: "Doubles a value".to_owned(),
                function_type: Some(1),
                specific_name: "double_value_1".to_owned(),
                body: "return value * 2".to_owned(),
                template: "double_value(?)".to_owned(),
            }
        );
        assert_eq!(
            community_function_parameter(BridgeCommunityFunctionParameter {
                function_database: "inventory".to_owned(),
                function_schema: "APP".to_owned(),
                function_name: "double_value".to_owned(),
                column_name: "value".to_owned(),
                column_type: Some(1),
                data_type: Some(4),
                type_name: "INTEGER".to_owned(),
                precision: Some(32),
                length: Some(4),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "Input value".to_owned(),
                char_octet_length: Some(4),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "double_value_1".to_owned(),
            }),
            CommunityFunctionParameter {
                function_database: "inventory".to_owned(),
                function_schema: "APP".to_owned(),
                function_name: "double_value".to_owned(),
                column_name: "value".to_owned(),
                column_type: Some(1),
                data_type: Some(4),
                type_name: "INTEGER".to_owned(),
                precision: Some(32),
                length: Some(4),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "Input value".to_owned(),
                char_octet_length: Some(4),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "double_value_1".to_owned(),
            }
        );
        assert_eq!(
            community_procedure(BridgeCommunityProcedure {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "refresh_items".to_owned(),
                remarks: "Refreshes inventory".to_owned(),
                procedure_type: Some(2),
                specific_name: "refresh_items_1".to_owned(),
                body: "call refresh_items()".to_owned(),
            }),
            CommunityProcedure {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "refresh_items".to_owned(),
                remarks: "Refreshes inventory".to_owned(),
                procedure_type: Some(2),
                specific_name: "refresh_items_1".to_owned(),
                body: "call refresh_items()".to_owned(),
            }
        );
        assert_eq!(
            community_procedure_parameter(BridgeCommunityProcedureParameter {
                procedure_database: "inventory".to_owned(),
                procedure_schema: "APP".to_owned(),
                procedure_name: "refresh_items".to_owned(),
                column_name: "limit_value".to_owned(),
                column_type: Some(1),
                data_type: Some(4),
                type_name: "INTEGER".to_owned(),
                precision: Some(32),
                length: Some(4),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "Row limit".to_owned(),
                column_default: "100".to_owned(),
                sql_data_type: Some(4),
                sql_datetime_sub: Some(0),
                char_octet_length: Some(4),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "refresh_items_1".to_owned(),
            }),
            CommunityProcedureParameter {
                procedure_database: "inventory".to_owned(),
                procedure_schema: "APP".to_owned(),
                procedure_name: "refresh_items".to_owned(),
                column_name: "limit_value".to_owned(),
                column_type: Some(1),
                data_type: Some(4),
                type_name: "INTEGER".to_owned(),
                precision: Some(32),
                length: Some(4),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "Row limit".to_owned(),
                column_default: "100".to_owned(),
                sql_data_type: Some(4),
                sql_datetime_sub: Some(0),
                char_octet_length: Some(4),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "refresh_items_1".to_owned(),
            }
        );
        assert_eq!(
            community_trigger(BridgeCommunityTrigger {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "items_audit".to_owned(),
                event_manipulation: "INSERT".to_owned(),
                body: "audit.ItemsTrigger".to_owned(),
            }),
            CommunityTrigger {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "items_audit".to_owned(),
                event_manipulation: "INSERT".to_owned(),
                body: "audit.ItemsTrigger".to_owned(),
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
    fn sql_validation_mapping_preserves_every_field() {
        let validation = BridgeCommunitySqlValidation {
            valid: false,
            statements: vec![BridgeCommunityParsedStatement {
                sql: "select from".to_owned(),
                statement_type: "UNKNOWN".to_owned(),
                kind: "Unknown".to_owned(),
            }],
            diagnostics: vec![BridgeCommunitySqlDiagnostic {
                start_line: 1,
                start_column: 8,
                end_line: 1,
                end_column: 12,
                token_text: "from".to_owned(),
                message: "unexpected FROM".to_owned(),
            }],
        };

        assert_eq!(
            community_sql_validation(validation),
            CommunitySqlValidation {
                valid: false,
                statements: vec![CommunityParsedStatement {
                    sql: "select from".to_owned(),
                    statement_type: "UNKNOWN".to_owned(),
                    kind: "Unknown".to_owned(),
                }],
                diagnostics: vec![CommunitySqlDiagnostic {
                    start_line: 1,
                    start_column: 8,
                    end_line: 1,
                    end_column: 12,
                    token_text: "from".to_owned(),
                    message: "unexpected FROM".to_owned(),
                }],
            }
        );
    }

    #[test]
    fn formatted_sql_mapping_preserves_sql() {
        assert_eq!(
            community_formatted_sql(BridgeCommunityFormattedSql {
                sql: "SELECT\n  1;".to_owned(),
            }),
            CommunityFormattedSql {
                sql: "SELECT\n  1;".to_owned(),
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
