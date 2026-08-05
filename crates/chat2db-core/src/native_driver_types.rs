use chat2db_contract::{
    CommunityDatabaseList, CommunityErTable, CommunityForeignKeyList, CommunityFunction,
    CommunityFunctionList, CommunityFunctionParameterList, CommunityPrimaryKeyList,
    CommunityProcedure, CommunityProcedureList, CommunityProcedureParameterList,
    CommunityRoutineInvocationPreview, CommunityRoutineMigrationExecution,
    CommunityRoutineMigrationRequest, CommunitySchemaList, CommunityTable,
    CommunityTableColumnList, CommunityTableIndexList, CommunityTableList,
    CommunityTablePreviewAccepted, CommunityTrigger, CommunityTriggerList, CommunityViewList,
    DmlExportRequest, GetCommunityFunctionRequest, GetCommunityProcedureRequest,
    GetCommunityTriggerRequest, ImportFileRequest, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityFunctionsRequest, ListCommunityIndexesRequest,
    ListCommunityProceduresRequest, ListCommunitySchemasRequest, ListCommunityTableKeysRequest,
    ListCommunityTablesRequest, ListCommunityTriggersRequest, ListCommunityViewsRequest,
    OtherFileExportRequest, PreviewCommunityRoutineInvocationRequest, SqlFileExportRequest,
    StartCommunityTablePreviewRequest, TransferArtifact,
};

pub(crate) type DatabaseList = CommunityDatabaseList;
pub(crate) type SchemaList = CommunitySchemaList;
pub(crate) type TableMetadata = CommunityTable;
pub(crate) type TableList = CommunityTableList;
pub(crate) type ColumnList = CommunityTableColumnList;
pub(crate) type IndexList = CommunityTableIndexList;
pub(crate) type ViewList = CommunityViewList;
pub(crate) type ForeignKeyList = CommunityForeignKeyList;
pub(crate) type PrimaryKeyList = CommunityPrimaryKeyList;
pub(crate) type FunctionMetadata = CommunityFunction;
pub(crate) type FunctionList = CommunityFunctionList;
pub(crate) type FunctionParameterList = CommunityFunctionParameterList;
pub(crate) type ProcedureMetadata = CommunityProcedure;
pub(crate) type ProcedureList = CommunityProcedureList;
pub(crate) type ProcedureParameterList = CommunityProcedureParameterList;
pub(crate) type TriggerMetadata = CommunityTrigger;
pub(crate) type TriggerList = CommunityTriggerList;
pub(crate) type EntityRelationTable = CommunityErTable;
pub(crate) type TablePreviewAccepted = CommunityTablePreviewAccepted;
pub(crate) type RoutineInvocationPreview = CommunityRoutineInvocationPreview;
pub(crate) type RoutineMigrationExecution = CommunityRoutineMigrationExecution;
pub(crate) type ImportTransferRequest = ImportFileRequest;
pub(crate) type SqlExportTransferRequest = SqlFileExportRequest;
pub(crate) type OtherExportTransferRequest = OtherFileExportRequest;
pub(crate) type DmlExportTransferRequest = DmlExportRequest;
pub(crate) type ExportArtifact = TransferArtifact;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuiltSql {
    pub(crate) sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatabaseDefinition {
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) charset: String,
    pub(crate) collation: String,
    pub(crate) owner: String,
    pub(crate) system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaDefinition {
    pub(crate) database_name: String,
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) owner: String,
    pub(crate) system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateSchemaSqlRequest {
    pub(crate) schema: SchemaDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamespaceSqlRequest {
    pub(crate) operation: NamespaceSqlOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NamespaceSqlOperation {
    CreateDatabase {
        database: DatabaseDefinition,
    },
    AlterDatabase {
        old_database: DatabaseDefinition,
        new_database: DatabaseDefinition,
    },
    DropDatabase {
        database_name: String,
    },
    UseDatabase {
        database_name: String,
    },
    CreateSchema {
        schema: SchemaDefinition,
    },
    AlterSchema {
        old_schema_name: String,
        new_schema_name: String,
    },
    DropSchema {
        schema_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DmlSqlRequest {
    pub(crate) target: DmlTarget,
    pub(crate) statement: DmlStatement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_field_names,
    reason = "qualified table segments are clearer with their database, schema, and table suffixes"
)]
pub(crate) struct DmlTarget {
    pub(crate) database_name: Option<String>,
    pub(crate) schema_name: Option<String>,
    pub(crate) table_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DmlColumn {
    pub(crate) name: String,
    pub(crate) data_type_name: String,
    pub(crate) precision: Option<u32>,
    pub(crate) scale: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DmlTemporalKind {
    Date,
    Time,
    LocalDatetime,
    OffsetDatetime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DmlValue {
    Null,
    String(String),
    Decimal(String),
    Boolean(bool),
    Temporal {
        kind: DmlTemporalKind,
        iso8601: String,
    },
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DmlRow {
    pub(crate) values: Vec<DmlValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DmlAssignment {
    pub(crate) column: DmlColumn,
    pub(crate) value: DmlValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DmlStatement {
    SingleInsert {
        columns: Vec<DmlColumn>,
        row: DmlRow,
    },
    MultiInsert {
        columns: Vec<DmlColumn>,
        rows: Vec<DmlRow>,
    },
    Update {
        assignments: Vec<DmlAssignment>,
        predicates: Vec<DmlAssignment>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetadataScope {
    pub(crate) datasource_id: String,
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableRef {
    pub(crate) scope: MetadataScope,
    pub(crate) table_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectRef {
    pub(crate) scope: MetadataScope,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListDatabasesRequest {
    pub(crate) datasource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListSchemasRequest {
    pub(crate) datasource_id: String,
    pub(crate) database_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListTablesRequest {
    pub(crate) scope: MetadataScope,
    pub(crate) name_pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListColumnsRequest {
    pub(crate) table: TableRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListIndexesRequest {
    pub(crate) table: TableRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListViewsRequest {
    pub(crate) scope: MetadataScope,
    pub(crate) name_pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListTableKeysRequest {
    pub(crate) table: TableRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRoutinesRequest {
    pub(crate) scope: MetadataScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListTriggersRequest {
    pub(crate) scope: MetadataScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TablePreviewRequest {
    pub(crate) table: TableRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutineInvocationRequest {
    pub(crate) scope: MetadataScope,
    pub(crate) routine_type: String,
    pub(crate) routine_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutineMigrationRequest {
    pub(crate) scope: MetadataScope,
    pub(crate) database_type: String,
    pub(crate) routine_type: String,
    pub(crate) routine_name: String,
    pub(crate) ddl: String,
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

impl From<ListCommunityViewsRequest> for ObjectRef {
    fn from(request: ListCommunityViewsRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            name: request.view_name_pattern,
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

impl From<GetCommunityFunctionRequest> for ObjectRef {
    fn from(request: GetCommunityFunctionRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            name: request.function_name,
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

impl From<GetCommunityProcedureRequest> for ObjectRef {
    fn from(request: GetCommunityProcedureRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            name: request.procedure_name,
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

impl From<GetCommunityTriggerRequest> for ObjectRef {
    fn from(request: GetCommunityTriggerRequest) -> Self {
        Self {
            scope: MetadataScope {
                datasource_id: request.datasource_id,
                database_name: request.database_name,
                schema_name: request.schema_name,
            },
            name: request.trigger_name,
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
