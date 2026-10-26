#![allow(
    unknown_lints,
    dead_code,
    clippy::unused_async,
    clippy::unused_async_trait_impl,
    clippy::missing_errors_doc,
    clippy::items_after_test_module
)]

use std::{future::Future, sync::Arc};

use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest,
    CommunityBuiltSql, CommunityDatabaseList, CommunityForeignKeyList, CommunityFormattedSql,
    CommunityFunction, CommunityFunctionList, CommunityFunctionParameterList,
    CommunityPluginCatalog, CommunityPrimaryKeyList, CommunityProcedure, CommunityProcedureList,
    CommunityProcedureParameterList, CommunityRoutineInvocationPreview,
    CommunityRoutineMigrationExecution, CommunityRoutineMigrationRequest, CommunitySchemaList,
    CommunitySqlAnalysis, CommunitySqlCompletion, CommunitySqlValidation, CommunityTable,
    CommunityTableColumnList, CommunityTableIndexList, CommunityTableList,
    CommunityTablePreviewAccepted, CommunityTrigger, CommunityTriggerList, CommunityViewList,
    CompleteCommunitySqlRequest, FormatCommunitySqlRequest, GetCommunityFunctionRequest,
    GetCommunityProcedureRequest, GetCommunityTriggerRequest, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityFunctionsRequest, ListCommunityIndexesRequest,
    ListCommunityProceduresRequest, ListCommunitySchemasRequest, ListCommunityTableKeysRequest,
    ListCommunityTablesRequest, ListCommunityTriggersRequest, ListCommunityViewsRequest,
    ParseCommunitySqlRequest, PreviewCommunityRoutineInvocationRequest,
    StartCommunityTablePreviewRequest, ValidateCommunitySqlRequest,
};

use crate::{
    AppError, Application,
    native_driver::{NativeDriver, native_capability_not_supported},
    storage_call,
};

impl Application {
    async fn native_driver_for_community_datasource(
        &self,
        datasource_id: &str,
        database_type: &str,
    ) -> Result<Option<Arc<dyn NativeDriver>>, AppError> {
        let storage = self.require_storage()?;
        let datasource_id = datasource_id.to_owned();
        let datasource = storage_call(move || storage.get_datasource(&datasource_id))
            .await?
            .ok_or_else(|| {
                AppError::not_found("datasource_not_found", "The datasource does not exist")
            })?;
        let Some(driver) = self.native_driver_for_datasource_driver_id(&datasource.driver_id)
        else {
            return Ok(None);
        };
        let database_type_matches = driver
            .descriptor()
            .database_types
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(database_type.trim()));
        if database_type_matches {
            return Ok(Some(driver));
        }
        if driver
            .descriptor()
            .id
            .eq_ignore_ascii_case(datasource.driver_id.trim())
        {
            return Err(AppError::invalid(
                "datasource_database_type_mismatch",
                "The datasource native Rust driver does not support the requested database type",
            ));
        }
        Ok(None)
    }

    /// Rejects native `MySQL` column reorders that cannot be rebuilt losslessly.
    ///
    /// # Errors
    ///
    /// Returns datasource, metadata, connection, or unsupported-column errors.
    pub async fn validate_native_mysql_column_reorder(
        &self,
        datasource_id: &str,
        database_name: &str,
        table_name: &str,
        column_names: &[String],
    ) -> Result<(), AppError> {
        let driver = self
            .require_native_driver_for_datasource(datasource_id)
            .await?;
        let tables = driver.tables().ok_or_else(|| {
            AppError::invalid(
                "native_table_capability_not_available",
                "The native Rust driver does not implement table operations",
            )
        })?;
        tables
            .validate_column_reorder(self, datasource_id, database_name, table_name, column_names)
            .await
    }

    /// Reads a `MySQL` table definition through the native driver without starting Java.
    ///
    /// # Errors
    ///
    /// Returns datasource, metadata, connection, or identifier validation errors.
    pub async fn table_ddl(
        &self,
        data_source_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        let driver = self
            .require_native_driver_for_datasource(data_source_id)
            .await?;
        let tables = driver.tables().ok_or_else(|| {
            AppError::invalid(
                "native_table_capability_not_available",
                "The native Rust driver does not implement table operations",
            )
        })?;
        tables
            .table_ddl(self, data_source_id, database_name, schema_name, table_name)
            .await
    }
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

impl Application {
    /// Lists schemas through the native driver SPI.
    ///
    /// # Errors
    ///
    /// Returns datasource, driver, or database failures.
    pub async fn list_community_schemas(
        &self,
        request: ListCommunitySchemasRequest,
    ) -> Result<CommunitySchemaList, AppError> {
        let driver = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
            .ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "schema metadata")
            })?;
        driver.metadata().ok_or_else(|| {
            native_capability_not_supported(&request.database_type, "schema metadata")
        })?;
        let database_type = request.database_type.clone();
        self.list_native_schemas(&database_type, request.into())
            .await
            .map(crate::native_api_adapter::schema_list_response)
    }

    /// Lists databases through the native driver SPI.
    ///
    /// # Errors
    ///
    /// Returns datasource, driver, or database failures.
    pub async fn list_community_databases(
        &self,
        request: ListCommunityDatabasesRequest,
    ) -> Result<CommunityDatabaseList, AppError> {
        let driver = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
            .ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "database metadata")
            })?;
        driver.metadata().ok_or_else(|| {
            native_capability_not_supported(&request.database_type, "database metadata")
        })?;
        let database_type = request.database_type.clone();
        self.list_native_databases(&database_type, request.into())
            .await
            .map(crate::native_api_adapter::database_list_response)
    }

    /// Lists tables through the native driver SPI.
    ///
    /// # Errors
    ///
    /// Returns datasource, driver, or database failures.
    pub async fn list_community_tables(
        &self,
        request: ListCommunityTablesRequest,
    ) -> Result<CommunityTableList, AppError> {
        let driver = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
            .ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "table metadata")
            })?;
        driver.metadata().ok_or_else(|| {
            native_capability_not_supported(&request.database_type, "table metadata")
        })?;
        let database_type = request.database_type.clone();
        self.list_native_tables(&database_type, request.into())
            .await
            .map(crate::native_api_adapter::table_list_response)
    }

    /// Lists columns through the native driver SPI.
    ///
    /// # Errors
    ///
    /// Returns datasource, driver, or database failures.
    pub async fn list_community_columns(
        &self,
        request: ListCommunityColumnsRequest,
    ) -> Result<CommunityTableColumnList, AppError> {
        let driver = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
            .ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "column metadata")
            })?;
        driver.metadata().ok_or_else(|| {
            native_capability_not_supported(&request.database_type, "column metadata")
        })?;
        let database_type = request.database_type.clone();
        self.list_native_columns(&database_type, request.into())
            .await
            .map(crate::native_api_adapter::column_list_response)
    }
}

impl Application {
    pub async fn build_community_create_schema(
        &self,
        _request: BuildCommunityCreateSchemaRequest,
    ) -> Result<CommunityBuiltSql, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn build_community_dml(
        &self,
        _request: BuildCommunityDmlRequest,
    ) -> Result<CommunityBuiltSql, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn build_community_namespace_sql(
        &self,
        _request: BuildCommunityNamespaceSqlRequest,
    ) -> Result<CommunityBuiltSql, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn complete_community_sql(
        &self,
        _request: CompleteCommunitySqlRequest,
    ) -> Result<CommunitySqlCompletion, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn execute_community_routine_migration(
        &self,
        _request: CommunityRoutineMigrationRequest,
    ) -> Result<CommunityRoutineMigrationExecution, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn format_community_sql(
        &self,
        _request: FormatCommunitySqlRequest,
    ) -> Result<CommunityFormattedSql, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn get_community_function(
        &self,
        _request: GetCommunityFunctionRequest,
    ) -> Result<CommunityFunction, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn get_community_procedure(
        &self,
        _request: GetCommunityProcedureRequest,
    ) -> Result<CommunityProcedure, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn get_community_trigger(
        &self,
        _request: GetCommunityTriggerRequest,
    ) -> Result<CommunityTrigger, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn get_community_view(
        &self,
        _request: ListCommunityViewsRequest,
    ) -> Result<CommunityTable, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_exported_keys(
        &self,
        request: ListCommunityTableKeysRequest,
    ) -> Result<CommunityForeignKeyList, AppError> {
        if let Some(driver) = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
        {
            let metadata = driver.metadata().ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "metadata")
            })?;
            return metadata
                .list_exported_keys(self, request.into())
                .await
                .map(crate::native_api_adapter::foreign_key_list_response);
        }
        Err(native_capability_not_supported(
            "this datasource",
            "the requested metadata",
        ))
    }

    pub async fn list_community_function_parameters(
        &self,
        _request: GetCommunityFunctionRequest,
    ) -> Result<CommunityFunctionParameterList, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_functions(
        &self,
        _request: ListCommunityFunctionsRequest,
    ) -> Result<CommunityFunctionList, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_imported_keys(
        &self,
        request: ListCommunityTableKeysRequest,
    ) -> Result<CommunityForeignKeyList, AppError> {
        if let Some(driver) = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
        {
            let metadata = driver.metadata().ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "metadata")
            })?;
            return metadata
                .list_imported_keys(self, request.into())
                .await
                .map(crate::native_api_adapter::foreign_key_list_response);
        }
        Err(native_capability_not_supported(
            "this datasource",
            "the requested metadata",
        ))
    }

    pub async fn list_community_indexes(
        &self,
        request: ListCommunityIndexesRequest,
    ) -> Result<CommunityTableIndexList, AppError> {
        if let Some(driver) = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
        {
            let metadata = driver.metadata().ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "metadata")
            })?;
            return metadata
                .list_indexes(self, request.into())
                .await
                .map(crate::native_api_adapter::index_list_response);
        }
        Err(native_capability_not_supported(
            "this datasource",
            "the requested metadata",
        ))
    }

    pub async fn list_community_plugins(&self) -> Result<CommunityPluginCatalog, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_primary_keys(
        &self,
        request: ListCommunityTableKeysRequest,
    ) -> Result<CommunityPrimaryKeyList, AppError> {
        if let Some(driver) = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
        {
            let metadata = driver.metadata().ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "metadata")
            })?;
            return metadata
                .list_primary_keys(self, request.into())
                .await
                .map(crate::native_api_adapter::primary_key_list_response);
        }
        Err(native_capability_not_supported(
            "this datasource",
            "the requested metadata",
        ))
    }

    pub async fn list_community_procedure_parameters(
        &self,
        _request: GetCommunityProcedureRequest,
    ) -> Result<CommunityProcedureParameterList, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_procedures(
        &self,
        _request: ListCommunityProceduresRequest,
    ) -> Result<CommunityProcedureList, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_triggers(
        &self,
        _request: ListCommunityTriggersRequest,
    ) -> Result<CommunityTriggerList, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn list_community_views(
        &self,
        request: ListCommunityViewsRequest,
    ) -> Result<CommunityViewList, AppError> {
        if let Some(driver) = self
            .native_driver_for_community_datasource(&request.datasource_id, &request.database_type)
            .await?
        {
            let metadata = driver.metadata().ok_or_else(|| {
                native_capability_not_supported(&request.database_type, "metadata")
            })?;
            return metadata
                .list_views(self, request.into())
                .await
                .map(crate::native_api_adapter::view_list_response);
        }
        Err(native_capability_not_supported(
            "this datasource",
            "the requested metadata",
        ))
    }

    pub async fn parse_community_sql(
        &self,
        _request: ParseCommunitySqlRequest,
    ) -> Result<CommunitySqlAnalysis, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn preview_community_routine_invocation(
        &self,
        _request: PreviewCommunityRoutineInvocationRequest,
    ) -> Result<CommunityRoutineInvocationPreview, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn start_community_table_preview(
        &self,
        _request: StartCommunityTablePreviewRequest,
    ) -> Result<CommunityTablePreviewAccepted, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }

    pub async fn validate_community_sql(
        &self,
        _request: ValidateCommunitySqlRequest,
    ) -> Result<CommunitySqlValidation, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }
}

impl Application {
    /// Rejects the removed Community routine migration preview.
    ///
    /// # Errors
    ///
    /// Always returns a removed-feature error.
    pub fn preview_community_routine_migration(
        &self,
        _request: &CommunityRoutineMigrationRequest,
    ) -> Result<CommunityRoutineInvocationPreview, AppError> {
        Err(AppError::invalid(
            "feature_removed",
            "this Community-backed feature was removed from the product",
        ))
    }
}
