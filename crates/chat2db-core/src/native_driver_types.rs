/// Stable identity and runtime-selection metadata for one native Rust driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeDriverDescriptor {
    /// Stable identifier owned by the native driver implementation.
    pub(crate) id: &'static str,
    /// Rust crate or library that implements the database protocol.
    pub(crate) implementation: &'static str,
    /// Product database types routed to this driver.
    pub(crate) database_types: &'static [&'static str],
    /// Historical driver names, package identifiers, or classes accepted at the compatibility boundary.
    pub(crate) compatibility_aliases: &'static [&'static str],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaMetadata {
    pub database_name: String,
    pub name: String,
    pub comment: String,
    pub owner: String,
    pub system: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaList {
    pub items: Vec<SchemaMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseMetadata {
    pub name: String,
    pub comment: String,
    pub charset: String,
    pub collation: String,
    pub owner: String,
    pub system: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseList {
    pub items: Vec<DatabaseMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableMetadata {
    pub database_name: String,
    pub schema_name: String,
    pub name: String,
    pub table_type: String,
    pub comment: String,
    pub database_type: String,
    pub pinned: bool,
    pub ddl: String,
    pub engine: String,
    pub charset: String,
    pub collation: String,
    pub increment_value: Option<String>,
    pub partition: String,
    pub tablespace: String,
    pub rows: Option<String>,
    pub data_length: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableList {
    pub items: Vec<TableMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ViewList {
    pub(crate) items: Vec<TableMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnMetadata {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub name: String,
    pub column_type: String,
    pub data_type: Option<i32>,
    pub default_value: Option<String>,
    pub auto_increment: Option<bool>,
    pub comment: String,
    pub primary_key: Option<bool>,
    pub primary_key_name: String,
    pub primary_key_order: i32,
    pub column_size: Option<i32>,
    pub buffer_length: Option<i32>,
    pub decimal_digits: Option<i32>,
    pub num_prec_radix: Option<i32>,
    pub sql_data_type: Option<i32>,
    pub sql_datetime_sub: Option<i32>,
    pub char_octet_length: Option<i32>,
    pub ordinal_position: Option<i32>,
    pub nullable: Option<i32>,
    pub generated_column: Option<bool>,
    pub extent: String,
    pub charset: String,
    pub collation: String,
    pub unit: String,
    pub sparse: Option<bool>,
    pub default_constraint_name: String,
    pub seed: Option<i32>,
    pub increment: Option<i32>,
    pub on_update_current_timestamp: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnList {
    pub items: Vec<ColumnMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IndexColumnMetadata {
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) table_name: String,
    pub(crate) index_name: String,
    pub(crate) column_name: String,
    pub(crate) column_type: String,
    pub(crate) comment: String,
    pub(crate) ordinal_position: Option<i32>,
    pub(crate) collation: String,
    pub(crate) non_unique: Option<bool>,
    pub(crate) index_qualifier: String,
    pub(crate) sort_order: String,
    pub(crate) cardinality: Option<String>,
    pub(crate) pages: Option<String>,
    pub(crate) filter_condition: String,
    pub(crate) sub_part: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IndexMetadata {
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) table_name: String,
    pub(crate) name: String,
    pub(crate) index_type: String,
    pub(crate) unique: Option<bool>,
    pub(crate) comment: String,
    pub(crate) columns: Vec<IndexColumnMetadata>,
    pub(crate) concurrently: Option<bool>,
    pub(crate) method: String,
    pub(crate) foreign_schema_name: String,
    pub(crate) foreign_table_name: String,
    pub(crate) foreign_column_names: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IndexList {
    pub(crate) items: Vec<IndexMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ForeignKeyMetadata {
    pub(crate) primary_table_database: String,
    pub(crate) primary_table_schema: String,
    pub(crate) primary_table_name: String,
    pub(crate) primary_column_name: String,
    pub(crate) foreign_table_database: String,
    pub(crate) foreign_table_schema: String,
    pub(crate) foreign_table_name: String,
    pub(crate) foreign_column_name: String,
    pub(crate) key_sequence: i32,
    pub(crate) update_rule: i32,
    pub(crate) delete_rule: i32,
    pub(crate) foreign_key_name: String,
    pub(crate) primary_key_name: String,
    pub(crate) deferrability: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ForeignKeyList {
    pub(crate) items: Vec<ForeignKeyMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PrimaryKeyMetadata {
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) table_name: String,
    pub(crate) column_name: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PrimaryKeyList {
    pub(crate) items: Vec<PrimaryKeyMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionMetadata {
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) name: String,
    pub(crate) remarks: String,
    pub(crate) function_type: Option<i32>,
    pub(crate) specific_name: String,
    pub(crate) body: String,
    pub(crate) template: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionList {
    pub(crate) items: Vec<FunctionMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionParameterMetadata {
    pub(crate) function_database: String,
    pub(crate) function_schema: String,
    pub(crate) function_name: String,
    pub(crate) column_name: String,
    pub(crate) column_type: Option<i32>,
    pub(crate) data_type: Option<i32>,
    pub(crate) type_name: String,
    pub(crate) precision: Option<i32>,
    pub(crate) length: Option<i32>,
    pub(crate) scale: Option<i32>,
    pub(crate) radix: Option<i32>,
    pub(crate) nullable: Option<i32>,
    pub(crate) remarks: String,
    pub(crate) char_octet_length: Option<i32>,
    pub(crate) ordinal_position: Option<i32>,
    pub(crate) is_nullable: String,
    pub(crate) specific_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionParameterList {
    pub(crate) items: Vec<FunctionParameterMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcedureMetadata {
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) name: String,
    pub(crate) remarks: String,
    pub(crate) procedure_type: Option<i32>,
    pub(crate) specific_name: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcedureList {
    pub(crate) items: Vec<ProcedureMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcedureParameterMetadata {
    pub(crate) procedure_database: String,
    pub(crate) procedure_schema: String,
    pub(crate) procedure_name: String,
    pub(crate) column_name: String,
    pub(crate) column_type: Option<i32>,
    pub(crate) data_type: Option<i32>,
    pub(crate) type_name: String,
    pub(crate) precision: Option<i32>,
    pub(crate) length: Option<i32>,
    pub(crate) scale: Option<i32>,
    pub(crate) radix: Option<i32>,
    pub(crate) nullable: Option<i32>,
    pub(crate) remarks: String,
    pub(crate) column_default: String,
    pub(crate) sql_data_type: Option<i32>,
    pub(crate) sql_datetime_sub: Option<i32>,
    pub(crate) char_octet_length: Option<i32>,
    pub(crate) ordinal_position: Option<i32>,
    pub(crate) is_nullable: String,
    pub(crate) specific_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcedureParameterList {
    pub(crate) items: Vec<ProcedureParameterMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TriggerMetadata {
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) name: String,
    pub(crate) event_manipulation: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TriggerList {
    pub(crate) items: Vec<TriggerMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EntityRelationColumn {
    pub(crate) name: String,
    pub(crate) column_type: String,
    pub(crate) primary_key: bool,
    pub(crate) comment: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EntityRelationForeignKey {
    pub(crate) primary_table: String,
    pub(crate) primary_column: String,
    pub(crate) foreign_table: String,
    pub(crate) foreign_column: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EntityRelationTable {
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) columns: Vec<EntityRelationColumn>,
    pub(crate) foreign_keys: Vec<EntityRelationForeignKey>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TablePreviewAccepted {
    pub operation_id: String,
    pub sql: String,
    pub row_limit: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RoutineInvocationPreview {
    pub(crate) sql: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RoutineMigrationExecution {
    pub(crate) success: bool,
    pub(crate) message: String,
    pub(crate) sql: String,
    pub(crate) failure_stage: Option<String>,
    pub(crate) restore_attempted: bool,
    pub(crate) restore_succeeded: bool,
}

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
pub struct MetadataScope {
    pub datasource_id: String,
    pub database_name: String,
    pub schema_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub scope: MetadataScope,
    pub table_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetadataObjectRef {
    pub(crate) scope: MetadataScope,
    pub(crate) object_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDatabasesRequest {
    pub datasource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSchemasRequest {
    pub datasource_id: String,
    pub database_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListTablesRequest {
    pub scope: MetadataScope,
    pub name_pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListColumnsRequest {
    pub table: TableRef,
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
pub struct TablePreviewRequest {
    pub table: TableRef,
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
