//! Adapts retained public API contracts to the database-neutral native driver SPI.

use chat2db_contract::{
    CommunityAccount, CommunityAccountAction, CommunityAccountCapability,
    CommunityAccountCommandRequest, CommunityAccountExecution, CommunityAccountGrantList,
    CommunityAccountGrantsRequest, CommunityAccountList, CommunityAccountPreview,
    CommunityAccountPrivilegeScope, CommunityDatabase, CommunityDatabaseList, CommunityErColumn,
    CommunityErForeignKey, CommunityErTable, CommunityForeignKey, CommunityForeignKeyList,
    CommunityFunction, CommunityFunctionList, CommunityFunctionParameter,
    CommunityFunctionParameterList, CommunityPrimaryKey, CommunityPrimaryKeyList,
    CommunityProcedure, CommunityProcedureList, CommunityProcedureParameter,
    CommunityProcedureParameterList, CommunityRoutineInvocationPreview,
    CommunityRoutineMigrationExecution, CommunityRoutineMigrationRequest, CommunitySchema,
    CommunitySchemaDiffEndpoint, CommunitySchemaDiffRequest, CommunitySchemaDiffSql,
    CommunitySchemaList, CommunityTable, CommunityTableColumn, CommunityTableColumnList,
    CommunityTableIndex, CommunityTableIndexColumn, CommunityTableIndexList, CommunityTableList,
    CommunityTablePreviewAccepted, CommunityTrigger, CommunityTriggerList, CommunityViewList,
    GetCommunityFunctionRequest, GetCommunityProcedureRequest, GetCommunityTriggerRequest,
    ListCommunityColumnsRequest, ListCommunityDatabasesRequest, ListCommunityFunctionsRequest,
    ListCommunityIndexesRequest, ListCommunityProceduresRequest, ListCommunitySchemasRequest,
    ListCommunityTableKeysRequest, ListCommunityTablesRequest, ListCommunityTriggersRequest,
    ListCommunityViewsRequest, PreviewCommunityRoutineInvocationRequest,
    StartCommunityTablePreviewRequest,
};

use crate::{
    AppError, Application,
    native_administration_types::{
        AdministrationAction, AdministrationCapability, AdministrationCommand,
        AdministrationExecution, AdministrationPreview, Principal, PrincipalGrantList,
        PrincipalGrantsRequest, PrincipalList, PrincipalRef, PrivilegeScope, PrivilegeTarget,
    },
    native_driver_types::{
        ColumnList, ColumnMetadata, DatabaseList, DatabaseMetadata, EntityRelationColumn,
        EntityRelationForeignKey, EntityRelationTable, ForeignKeyList, ForeignKeyMetadata,
        FunctionList, FunctionMetadata, FunctionParameterList, FunctionParameterMetadata,
        IndexColumnMetadata, IndexList, IndexMetadata, ListColumnsRequest, ListDatabasesRequest,
        ListIndexesRequest, ListRoutinesRequest, ListSchemasRequest, ListTableKeysRequest,
        ListTablesRequest, ListTriggersRequest, ListViewsRequest, MetadataObjectRef, MetadataScope,
        PrimaryKeyList, PrimaryKeyMetadata, ProcedureList, ProcedureMetadata,
        ProcedureParameterList, ProcedureParameterMetadata, RoutineInvocationPreview,
        RoutineInvocationRequest, RoutineMigrationExecution, RoutineMigrationRequest, SchemaList,
        SchemaMetadata, TableList, TableMetadata, TablePreviewAccepted, TablePreviewRequest,
        TableRef, TriggerList, TriggerMetadata, ViewList,
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
        if !source_driver
            .descriptor()
            .id
            .eq_ignore_ascii_case(target_driver.descriptor().id)
        {
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

impl From<ListCommunityDatabasesRequest> for ListDatabasesRequest {
    fn from(request: ListCommunityDatabasesRequest) -> Self {
        Self {
            datasource_id: request.datasource_id,
        }
    }
}

impl From<ListCommunitySchemasRequest> for ListSchemasRequest {
    fn from(request: ListCommunitySchemasRequest) -> Self {
        Self {
            datasource_id: request.datasource_id,
            database_name: request.database_name,
        }
    }
}

impl From<ListCommunityTablesRequest> for ListTablesRequest {
    fn from(request: ListCommunityTablesRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            name_pattern: request.table_name_pattern,
        }
    }
}

impl From<ListCommunityColumnsRequest> for ListColumnsRequest {
    fn from(request: ListCommunityColumnsRequest) -> Self {
        Self {
            table: TableRef {
                scope: MetadataScope {
                    datasource_id: request.datasource_id,
                    database_name: request.database_name,
                    schema_name: request.schema_name,
                },
                table_name: request.table_name,
            },
        }
    }
}

impl From<ListCommunityIndexesRequest> for ListIndexesRequest {
    fn from(request: ListCommunityIndexesRequest) -> Self {
        Self {
            table: TableRef {
                scope: MetadataScope {
                    datasource_id: request.datasource_id,
                    database_name: request.database_name,
                    schema_name: request.schema_name,
                },
                table_name: request.table_name,
            },
        }
    }
}

impl From<ListCommunityViewsRequest> for ListViewsRequest {
    fn from(request: ListCommunityViewsRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            name_pattern: request.view_name_pattern,
        }
    }
}

impl From<ListCommunityViewsRequest> for MetadataObjectRef {
    fn from(request: ListCommunityViewsRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            object_name: request.view_name_pattern,
        }
    }
}

impl From<ListCommunityTableKeysRequest> for ListTableKeysRequest {
    fn from(request: ListCommunityTableKeysRequest) -> Self {
        Self {
            table: TableRef {
                scope: MetadataScope {
                    datasource_id: request.datasource_id,
                    database_name: request.database_name,
                    schema_name: request.schema_name,
                },
                table_name: request.table_name,
            },
        }
    }
}

impl From<ListCommunityFunctionsRequest> for ListRoutinesRequest {
    fn from(request: ListCommunityFunctionsRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
        }
    }
}

impl From<GetCommunityFunctionRequest> for MetadataObjectRef {
    fn from(request: GetCommunityFunctionRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            object_name: request.function_name,
        }
    }
}

impl From<ListCommunityProceduresRequest> for ListRoutinesRequest {
    fn from(request: ListCommunityProceduresRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
        }
    }
}

impl From<GetCommunityProcedureRequest> for MetadataObjectRef {
    fn from(request: GetCommunityProcedureRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            object_name: request.procedure_name,
        }
    }
}

impl From<ListCommunityTriggersRequest> for ListTriggersRequest {
    fn from(request: ListCommunityTriggersRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
        }
    }
}

impl From<GetCommunityTriggerRequest> for MetadataObjectRef {
    fn from(request: GetCommunityTriggerRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            object_name: request.trigger_name,
        }
    }
}

impl From<StartCommunityTablePreviewRequest> for TablePreviewRequest {
    fn from(request: StartCommunityTablePreviewRequest) -> Self {
        Self {
            table: TableRef {
                scope: MetadataScope {
                    datasource_id: request.datasource_id,
                    database_name: request.database_name,
                    schema_name: request.schema_name,
                },
                table_name: request.table_name,
            },
        }
    }
}

impl From<PreviewCommunityRoutineInvocationRequest> for RoutineInvocationRequest {
    fn from(request: PreviewCommunityRoutineInvocationRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            routine_type: request.routine_type,
            routine_name: request.routine_name,
        }
    }
}

impl From<CommunityRoutineMigrationRequest> for RoutineMigrationRequest {
    fn from(request: CommunityRoutineMigrationRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            database_type: request.database_type,
            routine_type: request.routine_type,
            routine_name: request.routine_name,
            ddl: request.ddl,
        }
    }
}

impl From<RoutineMigrationRequest> for CommunityRoutineMigrationRequest {
    fn from(request: RoutineMigrationRequest) -> Self {
        Self {
            datasource_id: request.scope.datasource_id,
            database_type: request.database_type,
            database_name: request.scope.database_name,
            schema_name: request.scope.schema_name,
            routine_type: request.routine_type,
            routine_name: request.routine_name,
            ddl: request.ddl,
        }
    }
}

pub(crate) fn schema_list_response(response: SchemaList) -> CommunitySchemaList {
    CommunitySchemaList {
        items: response.items.into_iter().map(schema_response).collect(),
    }
}

pub(crate) fn database_list_response(response: DatabaseList) -> CommunityDatabaseList {
    CommunityDatabaseList {
        items: response.items.into_iter().map(database_response).collect(),
    }
}

pub(crate) fn table_list_response(response: TableList) -> CommunityTableList {
    CommunityTableList {
        items: response.items.into_iter().map(table_response).collect(),
    }
}

pub(crate) fn column_list_response(response: ColumnList) -> CommunityTableColumnList {
    CommunityTableColumnList {
        items: response.items.into_iter().map(column_response).collect(),
    }
}

pub(crate) fn index_list_response(response: IndexList) -> CommunityTableIndexList {
    CommunityTableIndexList {
        items: response.items.into_iter().map(index_response).collect(),
    }
}

pub(crate) fn view_list_response(response: ViewList) -> CommunityViewList {
    CommunityViewList {
        items: response.items.into_iter().map(table_response).collect(),
    }
}

pub(crate) fn table_response(response: TableMetadata) -> CommunityTable {
    CommunityTable {
        database_name: response.database_name,
        schema_name: response.schema_name,
        name: response.name,
        table_type: response.table_type,
        comment: response.comment,
        database_type: response.database_type,
        pinned: response.pinned,
        ddl: response.ddl,
        engine: response.engine,
        charset: response.charset,
        collation: response.collation,
        increment_value: response.increment_value,
        partition: response.partition,
        tablespace: response.tablespace,
        rows: response.rows,
        data_length: response.data_length,
        create_time: response.create_time,
        update_time: response.update_time,
    }
}

pub(crate) fn foreign_key_list_response(response: ForeignKeyList) -> CommunityForeignKeyList {
    CommunityForeignKeyList {
        items: response
            .items
            .into_iter()
            .map(foreign_key_response)
            .collect(),
    }
}

pub(crate) fn primary_key_list_response(response: PrimaryKeyList) -> CommunityPrimaryKeyList {
    CommunityPrimaryKeyList {
        items: response
            .items
            .into_iter()
            .map(primary_key_response)
            .collect(),
    }
}

pub(crate) fn function_list_response(response: FunctionList) -> CommunityFunctionList {
    CommunityFunctionList {
        items: response.items.into_iter().map(function_response).collect(),
    }
}

pub(crate) fn function_response(response: FunctionMetadata) -> CommunityFunction {
    CommunityFunction {
        database_name: response.database_name,
        schema_name: response.schema_name,
        name: response.name,
        remarks: response.remarks,
        function_type: response.function_type,
        specific_name: response.specific_name,
        body: response.body,
        template: response.template,
    }
}

pub(crate) fn function_parameter_list_response(
    response: FunctionParameterList,
) -> CommunityFunctionParameterList {
    CommunityFunctionParameterList {
        items: response
            .items
            .into_iter()
            .map(function_parameter_response)
            .collect(),
    }
}

pub(crate) fn procedure_list_response(response: ProcedureList) -> CommunityProcedureList {
    CommunityProcedureList {
        items: response.items.into_iter().map(procedure_response).collect(),
    }
}

pub(crate) fn procedure_response(response: ProcedureMetadata) -> CommunityProcedure {
    CommunityProcedure {
        database_name: response.database_name,
        schema_name: response.schema_name,
        name: response.name,
        remarks: response.remarks,
        procedure_type: response.procedure_type,
        specific_name: response.specific_name,
        body: response.body,
    }
}

pub(crate) fn procedure_parameter_list_response(
    response: ProcedureParameterList,
) -> CommunityProcedureParameterList {
    CommunityProcedureParameterList {
        items: response
            .items
            .into_iter()
            .map(procedure_parameter_response)
            .collect(),
    }
}

pub(crate) fn trigger_list_response(response: TriggerList) -> CommunityTriggerList {
    CommunityTriggerList {
        items: response.items.into_iter().map(trigger_response).collect(),
    }
}

pub(crate) fn trigger_response(response: TriggerMetadata) -> CommunityTrigger {
    CommunityTrigger {
        database_name: response.database_name,
        schema_name: response.schema_name,
        name: response.name,
        event_manipulation: response.event_manipulation,
        body: response.body,
    }
}

pub(crate) fn er_tables_response(response: Vec<EntityRelationTable>) -> Vec<CommunityErTable> {
    response.into_iter().map(er_table_response).collect()
}

pub(crate) fn table_preview_response(
    response: TablePreviewAccepted,
) -> CommunityTablePreviewAccepted {
    CommunityTablePreviewAccepted {
        operation_id: response.operation_id,
        sql: response.sql,
        row_limit: response.row_limit,
    }
}

pub(crate) fn routine_invocation_response(
    response: RoutineInvocationPreview,
) -> CommunityRoutineInvocationPreview {
    CommunityRoutineInvocationPreview { sql: response.sql }
}

pub(crate) fn routine_migration_execution_response(
    response: RoutineMigrationExecution,
) -> CommunityRoutineMigrationExecution {
    CommunityRoutineMigrationExecution {
        success: response.success,
        message: response.message,
        sql: response.sql,
        failure_stage: response.failure_stage,
        restore_attempted: response.restore_attempted,
        restore_succeeded: response.restore_succeeded,
    }
}

pub(crate) fn compatibility_api_error(error: AppError) -> AppError {
    let api = error.api_error();
    let compatibility_code = match api.code.as_str() {
        "invalid_routine_invocation_request" => "invalid_community_routine_invocation_request",
        "invalid_routine_migration_request" => "invalid_community_routine_migration_request",
        "invalid_table_preview_request" => "invalid_community_table_preview_request",
        _ => return error,
    };
    AppError::invalid(compatibility_code, api.message)
}

fn schema_response(response: SchemaMetadata) -> CommunitySchema {
    CommunitySchema {
        database_name: response.database_name,
        name: response.name,
        comment: response.comment,
        owner: response.owner,
        system: response.system,
    }
}

fn database_response(response: DatabaseMetadata) -> CommunityDatabase {
    CommunityDatabase {
        name: response.name,
        comment: response.comment,
        charset: response.charset,
        collation: response.collation,
        owner: response.owner,
        system: response.system,
    }
}

fn column_response(response: ColumnMetadata) -> CommunityTableColumn {
    CommunityTableColumn {
        database_name: response.database_name,
        schema_name: response.schema_name,
        table_name: response.table_name,
        name: response.name,
        column_type: response.column_type,
        data_type: response.data_type,
        default_value: response.default_value,
        auto_increment: response.auto_increment,
        comment: response.comment,
        primary_key: response.primary_key,
        primary_key_name: response.primary_key_name,
        primary_key_order: response.primary_key_order,
        column_size: response.column_size,
        buffer_length: response.buffer_length,
        decimal_digits: response.decimal_digits,
        num_prec_radix: response.num_prec_radix,
        sql_data_type: response.sql_data_type,
        sql_datetime_sub: response.sql_datetime_sub,
        char_octet_length: response.char_octet_length,
        ordinal_position: response.ordinal_position,
        nullable: response.nullable,
        generated_column: response.generated_column,
        extent: response.extent,
        charset: response.charset,
        collation: response.collation,
        unit: response.unit,
        sparse: response.sparse,
        default_constraint_name: response.default_constraint_name,
        seed: response.seed,
        increment: response.increment,
        on_update_current_timestamp: response.on_update_current_timestamp,
    }
}

fn index_column_response(response: IndexColumnMetadata) -> CommunityTableIndexColumn {
    CommunityTableIndexColumn {
        database_name: response.database_name,
        schema_name: response.schema_name,
        table_name: response.table_name,
        index_name: response.index_name,
        column_name: response.column_name,
        column_type: response.column_type,
        comment: response.comment,
        ordinal_position: response.ordinal_position,
        collation: response.collation,
        non_unique: response.non_unique,
        index_qualifier: response.index_qualifier,
        sort_order: response.sort_order,
        cardinality: response.cardinality,
        pages: response.pages,
        filter_condition: response.filter_condition,
        sub_part: response.sub_part,
    }
}

fn index_response(response: IndexMetadata) -> CommunityTableIndex {
    CommunityTableIndex {
        database_name: response.database_name,
        schema_name: response.schema_name,
        table_name: response.table_name,
        name: response.name,
        index_type: response.index_type,
        unique: response.unique,
        comment: response.comment,
        columns: response
            .columns
            .into_iter()
            .map(index_column_response)
            .collect(),
        concurrently: response.concurrently,
        method: response.method,
        foreign_schema_name: response.foreign_schema_name,
        foreign_table_name: response.foreign_table_name,
        foreign_column_names: response.foreign_column_names,
    }
}

fn foreign_key_response(response: ForeignKeyMetadata) -> CommunityForeignKey {
    CommunityForeignKey {
        primary_table_database: response.primary_table_database,
        primary_table_schema: response.primary_table_schema,
        primary_table_name: response.primary_table_name,
        primary_column_name: response.primary_column_name,
        foreign_table_database: response.foreign_table_database,
        foreign_table_schema: response.foreign_table_schema,
        foreign_table_name: response.foreign_table_name,
        foreign_column_name: response.foreign_column_name,
        key_sequence: response.key_sequence,
        update_rule: response.update_rule,
        delete_rule: response.delete_rule,
        foreign_key_name: response.foreign_key_name,
        primary_key_name: response.primary_key_name,
        deferrability: response.deferrability,
    }
}

fn primary_key_response(response: PrimaryKeyMetadata) -> CommunityPrimaryKey {
    CommunityPrimaryKey {
        database_name: response.database_name,
        schema_name: response.schema_name,
        table_name: response.table_name,
        column_name: response.column_name,
        name: response.name,
    }
}

fn function_parameter_response(response: FunctionParameterMetadata) -> CommunityFunctionParameter {
    CommunityFunctionParameter {
        function_database: response.function_database,
        function_schema: response.function_schema,
        function_name: response.function_name,
        column_name: response.column_name,
        column_type: response.column_type,
        data_type: response.data_type,
        type_name: response.type_name,
        precision: response.precision,
        length: response.length,
        scale: response.scale,
        radix: response.radix,
        nullable: response.nullable,
        remarks: response.remarks,
        char_octet_length: response.char_octet_length,
        ordinal_position: response.ordinal_position,
        is_nullable: response.is_nullable,
        specific_name: response.specific_name,
    }
}

fn procedure_parameter_response(
    response: ProcedureParameterMetadata,
) -> CommunityProcedureParameter {
    CommunityProcedureParameter {
        procedure_database: response.procedure_database,
        procedure_schema: response.procedure_schema,
        procedure_name: response.procedure_name,
        column_name: response.column_name,
        column_type: response.column_type,
        data_type: response.data_type,
        type_name: response.type_name,
        precision: response.precision,
        length: response.length,
        scale: response.scale,
        radix: response.radix,
        nullable: response.nullable,
        remarks: response.remarks,
        column_default: response.column_default,
        sql_data_type: response.sql_data_type,
        sql_datetime_sub: response.sql_datetime_sub,
        char_octet_length: response.char_octet_length,
        ordinal_position: response.ordinal_position,
        is_nullable: response.is_nullable,
        specific_name: response.specific_name,
    }
}

fn er_table_response(response: EntityRelationTable) -> CommunityErTable {
    CommunityErTable {
        name: response.name,
        comment: response.comment,
        column_list: response
            .columns
            .into_iter()
            .map(er_column_response)
            .collect(),
        foreign_key_list: response
            .foreign_keys
            .into_iter()
            .map(er_foreign_key_response)
            .collect(),
    }
}

fn er_column_response(response: EntityRelationColumn) -> CommunityErColumn {
    CommunityErColumn {
        name: response.name,
        column_type: response.column_type,
        primary_key: response.primary_key,
        comment: response.comment,
    }
}

fn er_foreign_key_response(response: EntityRelationForeignKey) -> CommunityErForeignKey {
    CommunityErForeignKey {
        pk_table_name: response.primary_table,
        pk_column_name: response.primary_column,
        fk_table_name: response.foreign_table,
        fk_column_name: response.foreign_column,
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
    use chat2db_contract::{
        CommunityAccountAction, CommunityAccountCommandRequest, CommunityErColumn,
        CommunityErForeignKey, CommunityErTable, CommunityTableColumn, CommunityTableColumnList,
        CommunityTableIndex, CommunityTableIndexColumn, CommunityTableIndexList,
    };

    use super::{
        administration_command, column_list_response, community_account_action,
        compatibility_api_error, er_tables_response, index_list_response,
    };
    use crate::{
        AppError,
        native_driver_types::{
            ColumnList, ColumnMetadata, EntityRelationColumn, EntityRelationForeignKey,
            EntityRelationTable, IndexColumnMetadata, IndexList, IndexMetadata,
        },
    };

    #[test]
    fn neutral_validation_errors_preserve_compatibility_wire_codes() {
        for (neutral, legacy) in [
            (
                "invalid_routine_invocation_request",
                "invalid_community_routine_invocation_request",
            ),
            (
                "invalid_routine_migration_request",
                "invalid_community_routine_migration_request",
            ),
            (
                "invalid_table_preview_request",
                "invalid_community_table_preview_request",
            ),
        ] {
            let error = compatibility_api_error(AppError::invalid(neutral, "invalid request"));
            let api = error.api_error();
            assert_eq!(api.code, legacy);
            assert_eq!(api.message, "invalid request");
        }
    }

    #[test]
    fn column_metadata_maps_every_field_to_the_legacy_response() {
        let response = column_list_response(ColumnList {
            items: vec![ColumnMetadata {
                database_name: "catalog".to_owned(),
                schema_name: "schema".to_owned(),
                table_name: "orders".to_owned(),
                name: "total".to_owned(),
                column_type: "DECIMAL(12,2)".to_owned(),
                data_type: Some(3),
                default_value: Some("0.00".to_owned()),
                auto_increment: Some(false),
                comment: "order total".to_owned(),
                primary_key: Some(false),
                primary_key_name: "pk_orders".to_owned(),
                primary_key_order: 2,
                column_size: Some(12),
                buffer_length: Some(16),
                decimal_digits: Some(2),
                num_prec_radix: Some(10),
                sql_data_type: Some(3),
                sql_datetime_sub: Some(4),
                char_octet_length: Some(48),
                ordinal_position: Some(5),
                nullable: Some(1),
                generated_column: Some(false),
                extent: "extent".to_owned(),
                charset: "utf8mb4".to_owned(),
                collation: "utf8mb4_bin".to_owned(),
                unit: "bytes".to_owned(),
                sparse: Some(true),
                default_constraint_name: "df_orders_total".to_owned(),
                seed: Some(7),
                increment: Some(8),
                on_update_current_timestamp: Some(true),
            }],
        });

        assert_eq!(
            response,
            CommunityTableColumnList {
                items: vec![CommunityTableColumn {
                    database_name: "catalog".to_owned(),
                    schema_name: "schema".to_owned(),
                    table_name: "orders".to_owned(),
                    name: "total".to_owned(),
                    column_type: "DECIMAL(12,2)".to_owned(),
                    data_type: Some(3),
                    default_value: Some("0.00".to_owned()),
                    auto_increment: Some(false),
                    comment: "order total".to_owned(),
                    primary_key: Some(false),
                    primary_key_name: "pk_orders".to_owned(),
                    primary_key_order: 2,
                    column_size: Some(12),
                    buffer_length: Some(16),
                    decimal_digits: Some(2),
                    num_prec_radix: Some(10),
                    sql_data_type: Some(3),
                    sql_datetime_sub: Some(4),
                    char_octet_length: Some(48),
                    ordinal_position: Some(5),
                    nullable: Some(1),
                    generated_column: Some(false),
                    extent: "extent".to_owned(),
                    charset: "utf8mb4".to_owned(),
                    collation: "utf8mb4_bin".to_owned(),
                    unit: "bytes".to_owned(),
                    sparse: Some(true),
                    default_constraint_name: "df_orders_total".to_owned(),
                    seed: Some(7),
                    increment: Some(8),
                    on_update_current_timestamp: Some(true),
                }],
            }
        );
    }

    #[test]
    fn index_metadata_preserves_nested_columns() {
        let indexes = index_list_response(IndexList {
            items: vec![IndexMetadata {
                database_name: "catalog".to_owned(),
                schema_name: "schema".to_owned(),
                table_name: "orders".to_owned(),
                name: "idx_orders_customer".to_owned(),
                index_type: "BTREE".to_owned(),
                unique: Some(true),
                comment: "customer lookup".to_owned(),
                columns: vec![IndexColumnMetadata {
                    database_name: "catalog".to_owned(),
                    schema_name: "schema".to_owned(),
                    table_name: "orders".to_owned(),
                    index_name: "idx_orders_customer".to_owned(),
                    column_name: "customer_id".to_owned(),
                    column_type: "BIGINT".to_owned(),
                    comment: "customer".to_owned(),
                    ordinal_position: Some(1),
                    collation: "A".to_owned(),
                    non_unique: Some(false),
                    index_qualifier: "catalog".to_owned(),
                    sort_order: "ASC".to_owned(),
                    cardinality: Some("99".to_owned()),
                    pages: Some("4".to_owned()),
                    filter_condition: "active = 1".to_owned(),
                    sub_part: Some("8".to_owned()),
                }],
                concurrently: Some(false),
                method: "btree".to_owned(),
                foreign_schema_name: "crm".to_owned(),
                foreign_table_name: "customers".to_owned(),
                foreign_column_names: vec!["id".to_owned()],
            }],
        });
        assert_eq!(
            indexes,
            CommunityTableIndexList {
                items: vec![CommunityTableIndex {
                    database_name: "catalog".to_owned(),
                    schema_name: "schema".to_owned(),
                    table_name: "orders".to_owned(),
                    name: "idx_orders_customer".to_owned(),
                    index_type: "BTREE".to_owned(),
                    unique: Some(true),
                    comment: "customer lookup".to_owned(),
                    columns: vec![CommunityTableIndexColumn {
                        database_name: "catalog".to_owned(),
                        schema_name: "schema".to_owned(),
                        table_name: "orders".to_owned(),
                        index_name: "idx_orders_customer".to_owned(),
                        column_name: "customer_id".to_owned(),
                        column_type: "BIGINT".to_owned(),
                        comment: "customer".to_owned(),
                        ordinal_position: Some(1),
                        collation: "A".to_owned(),
                        non_unique: Some(false),
                        index_qualifier: "catalog".to_owned(),
                        sort_order: "ASC".to_owned(),
                        cardinality: Some("99".to_owned()),
                        pages: Some("4".to_owned()),
                        filter_condition: "active = 1".to_owned(),
                        sub_part: Some("8".to_owned()),
                    }],
                    concurrently: Some(false),
                    method: "btree".to_owned(),
                    foreign_schema_name: "crm".to_owned(),
                    foreign_table_name: "customers".to_owned(),
                    foreign_column_names: vec!["id".to_owned()],
                }],
            }
        );
    }

    #[test]
    fn er_metadata_preserves_nested_relationships() {
        assert_eq!(
            er_tables_response(vec![EntityRelationTable {
                name: "orders".to_owned(),
                comment: "orders table".to_owned(),
                columns: vec![EntityRelationColumn {
                    name: "customer_id".to_owned(),
                    column_type: "BIGINT".to_owned(),
                    primary_key: false,
                    comment: "customer".to_owned(),
                }],
                foreign_keys: vec![EntityRelationForeignKey {
                    primary_table: "customers".to_owned(),
                    primary_column: "id".to_owned(),
                    foreign_table: "orders".to_owned(),
                    foreign_column: "customer_id".to_owned(),
                }],
            }]),
            vec![CommunityErTable {
                name: "orders".to_owned(),
                comment: "orders table".to_owned(),
                column_list: vec![CommunityErColumn {
                    name: "customer_id".to_owned(),
                    column_type: "BIGINT".to_owned(),
                    primary_key: false,
                    comment: "customer".to_owned(),
                }],
                foreign_key_list: vec![CommunityErForeignKey {
                    pk_table_name: "customers".to_owned(),
                    pk_column_name: "id".to_owned(),
                    fk_table_name: "orders".to_owned(),
                    fk_column_name: "customer_id".to_owned(),
                }],
            }]
        );
    }

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
