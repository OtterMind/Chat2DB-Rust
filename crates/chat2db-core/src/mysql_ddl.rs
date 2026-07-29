//! MySQL-specific structured SQL builders used by the retained Community API.

use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::AppError;

pub const MYSQL_RESULT_DEFAULT_PLACEHOLDER: &str = "CHAT2DB_UPDATE_TABLE_DATA_USER_FILLED_DEFAULT";
pub const MYSQL_RESULT_GENERATED_PLACEHOLDER: &str =
    "CHAT2DB_UPDATE_TABLE_DATA_USER_FILLED_GENERATED";
pub const MYSQL_PARTIAL_LARGE_VALUE_PREFIX: &str = "CHAT2DB_LARGE_VALUE_PREVIEW:";

const MAX_IDENTIFIER_CHARS: usize = 64;
const MAX_VIEW_BODY_BYTES: usize = 1024 * 1024;
const MAX_COMMENT_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlQualifiedName {
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub schema_name: Option<String>,
    pub name: String,
}

impl MysqlQualifiedName {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            database_name: None,
            schema_name: None,
            name: name.into(),
        }
    }

    #[must_use]
    pub fn in_database(database_name: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            database_name: Some(database_name.into()),
            schema_name: None,
            name: name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlResultGridHeader {
    pub name: String,
    #[serde(default)]
    pub column_type: String,
    #[serde(default)]
    pub data_type: String,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub auto_increment: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlResultGridOperationType {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlResultGridOperation {
    #[serde(rename = "type")]
    pub operation_type: MysqlResultGridOperationType,
    #[serde(default)]
    pub data_list: Vec<Option<String>>,
    #[serde(default)]
    pub old_data_list: Vec<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlResultGridCopyOperationType {
    Create,
    UpdateCopy,
    Where,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlResultGridCopyOperation {
    #[serde(rename = "type")]
    pub operation_type: MysqlResultGridCopyOperationType,
    #[serde(default)]
    pub data_list: Vec<Option<String>>,
    #[serde(default)]
    pub select_cols: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlDatabaseDefinition {
    pub name: String,
    #[serde(default)]
    pub if_not_exists: bool,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct MysqlColumnDefinition {
    pub name: String,
    pub type_name: String,
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub scale: Option<u32>,
    #[serde(default)]
    pub unsigned: bool,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub enum_values: Vec<String>,
    #[serde(default)]
    pub on_update_current_timestamp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlSortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlIndexMethod {
    Btree,
    Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlIndexKind {
    Primary,
    Normal,
    Unique,
    Fulltext,
    Spatial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlIndexColumn {
    pub name: String,
    #[serde(default)]
    pub prefix_length: Option<u32>,
    #[serde(default)]
    pub order: Option<MysqlSortOrder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlIndexDefinition {
    pub kind: MysqlIndexKind,
    #[serde(default)]
    pub name: Option<String>,
    pub columns: Vec<MysqlIndexColumn>,
    #[serde(default)]
    pub method: Option<MysqlIndexMethod>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlTableDefinition {
    pub name: MysqlQualifiedName,
    #[serde(default)]
    pub if_not_exists: bool,
    pub columns: Vec<MysqlColumnDefinition>,
    #[serde(default)]
    pub indexes: Vec<MysqlIndexDefinition>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub auto_increment: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlColumnPosition {
    First,
    After(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum MysqlColumnAlter {
    Add {
        column: MysqlColumnDefinition,
        #[serde(default)]
        position: Option<MysqlColumnPosition>,
    },
    Modify {
        old_name: String,
        column: MysqlColumnDefinition,
        #[serde(default)]
        position: Option<MysqlColumnPosition>,
    },
    Delete {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum MysqlIndexAlter {
    Add {
        index: MysqlIndexDefinition,
    },
    Modify {
        old_kind: MysqlIndexKind,
        #[serde(default)]
        old_name: Option<String>,
        index: MysqlIndexDefinition,
    },
    Delete {
        kind: MysqlIndexKind,
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlTableAlter {
    pub table: MysqlQualifiedName,
    #[serde(default)]
    pub rename_to: Option<MysqlQualifiedName>,
    #[serde(default)]
    pub columns: Vec<MysqlColumnAlter>,
    #[serde(default)]
    pub indexes: Vec<MysqlIndexAlter>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub auto_increment: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlTableCopy {
    pub source: MysqlQualifiedName,
    pub target: MysqlQualifiedName,
    #[serde(default)]
    pub if_not_exists: bool,
    #[serde(default)]
    pub copy_data: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlViewDefinition {
    pub name: MysqlQualifiedName,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub use_or_replace: bool,
    #[serde(default)]
    pub algorithm: Option<MysqlViewAlgorithm>,
    #[serde(default)]
    pub definer: Option<MysqlViewDefiner>,
    #[serde(default)]
    pub sql_security: Option<MysqlViewSecurity>,
    #[serde(default)]
    pub check_option: Option<MysqlViewCheckOption>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlViewAlgorithm {
    Undefined,
    Merge,
    Temptable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlViewSecurity {
    Definer,
    Invoker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlViewDefiner {
    pub user: String,
    pub host: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MysqlViewCheckOption {
    Cascaded,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct MysqlEditorColumnType {
    pub type_name: String,
    pub support_length: bool,
    pub support_scale: bool,
    pub support_nullable: bool,
    pub support_auto_increment: bool,
    pub support_charset: bool,
    pub support_collation: bool,
    pub support_comments: bool,
    pub support_default_value: bool,
    pub support_extent: bool,
    pub support_value: bool,
    pub support_unit: bool,
    pub support_on_update_current_timestamp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlEditorCharset {
    pub charset_name: String,
    pub default_collation_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlEditorCollation {
    pub collation_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlEditorIndexType {
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlEditorDefaultValue {
    pub default_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct MysqlEditorEngineType {
    pub name: String,
    #[serde(rename = "supportTTL")]
    pub support_ttl: bool,
    pub support_sort_order: bool,
    pub support_skipping_indices: bool,
    pub support_deduplication: bool,
    pub support_settings: bool,
    pub support_parallel_insert: bool,
    pub support_projections: bool,
    pub support_replication: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlTableEditorMeta {
    pub column_types: Vec<MysqlEditorColumnType>,
    pub charsets: Vec<MysqlEditorCharset>,
    pub collations: Vec<MysqlEditorCollation>,
    pub index_types: Vec<MysqlEditorIndexType>,
    pub default_values: Vec<MysqlEditorDefaultValue>,
    pub engine_types: Vec<MysqlEditorEngineType>,
}

/// Builds one native `MySQL` result-grid mutation statement.
///
/// # Errors
///
/// Returns [`AppError`] when identifiers, row shapes, placeholders, or typed values are invalid.
pub fn build_mysql_result_grid_sql(
    table: &MysqlQualifiedName,
    headers: &[MysqlResultGridHeader],
    operation: &MysqlResultGridOperation,
) -> Result<String, AppError> {
    validate_result_grid(table, headers, operation)?;
    let table = quote_qualified_name(table)?;
    match operation.operation_type {
        MysqlResultGridOperationType::Create => {
            build_result_grid_insert(&table, headers, &operation.data_list)
        }
        MysqlResultGridOperationType::Update => build_result_grid_update(
            &table,
            headers,
            &operation.data_list,
            &operation.old_data_list,
        ),
        MysqlResultGridOperationType::Delete => {
            build_result_grid_delete(&table, headers, &operation.old_data_list)
        }
    }
}

/// Builds a semicolon-delimited native `MySQL` result-grid mutation script.
///
/// # Errors
///
/// Returns [`AppError`] when no operations are supplied or any operation is invalid.
pub fn build_mysql_result_grid_script(
    table: &MysqlQualifiedName,
    headers: &[MysqlResultGridHeader],
    operations: &[MysqlResultGridOperation],
) -> Result<String, AppError> {
    if operations.is_empty() {
        return Err(invalid_grid(
            "At least one result-grid operation is required",
        ));
    }
    let statements = operations
        .iter()
        .map(|operation| build_mysql_result_grid_sql(table, headers, operation))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("{};", statements.join(";\n")))
}

/// Builds SQL copied from selected result-grid rows without executing it.
///
/// # Errors
///
/// Returns [`AppError`] when the selection or any typed value is invalid.
pub fn build_mysql_result_grid_copy_sql(
    table: &MysqlQualifiedName,
    headers: &[MysqlResultGridHeader],
    operations: &[MysqlResultGridCopyOperation],
) -> Result<String, AppError> {
    validate_copy_operations(table, headers, operations)?;
    if operations.first().is_some_and(|operation| {
        operation.operation_type == MysqlResultGridCopyOperationType::Where
    }) {
        if operations
            .iter()
            .any(|operation| operation.operation_type != MysqlResultGridCopyOperationType::Where)
        {
            return Err(invalid_grid(
                "WHERE copy operations cannot be mixed with INSERT or UPDATE operations",
            ));
        }
        return build_copy_where(headers, operations).map(|predicate| format!("WHERE {predicate}"));
    }

    let table = quote_qualified_name(table)?;
    let statements = operations
        .iter()
        .map(|operation| match operation.operation_type {
            MysqlResultGridCopyOperationType::Create => {
                build_copy_insert(&table, headers, operation)
            }
            MysqlResultGridCopyOperationType::UpdateCopy => {
                build_copy_update(&table, headers, operation)
            }
            MysqlResultGridCopyOperationType::Where => Err(invalid_grid(
                "WHERE copy operations cannot be mixed with INSERT or UPDATE operations",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("{};", statements.join(";\n")))
}

/// Builds a typed SQL `IN` value list from one selected result-grid column.
///
/// # Errors
///
/// Returns [`AppError`] for an empty, mixed-column, binary, partial, or malformed selection.
pub fn build_mysql_result_grid_in_values(
    headers: &[MysqlResultGridHeader],
    operations: &[MysqlResultGridCopyOperation],
) -> Result<String, AppError> {
    validate_copy_headers(headers)?;
    if operations.is_empty() {
        return Err(invalid_grid("At least one selected value is required"));
    }
    let mut selected_index = None;
    let mut values = Vec::new();
    for operation in operations {
        require_row_length("dataList", &operation.data_list, headers.len())?;
        reject_partial_large_values(&operation.data_list)?;
        let [index] = operation.select_cols.as_slice() else {
            return Err(invalid_grid(
                "SQL IN values require exactly one selected column per row",
            ));
        };
        validate_selected_index(*index, headers.len())?;
        if selected_index
            .replace(*index)
            .is_some_and(|current| current != *index)
        {
            return Err(invalid_grid(
                "SQL IN values must come from the same result column",
            ));
        }
        let header = &headers[*index];
        if is_binary_type(base_type_name(&result_grid_type_name(header))) {
            return Err(invalid_grid(
                "Binary and large-value columns cannot be copied as SQL IN values",
            ));
        }
        let value = serialize_mysql_value(header, operation.data_list[*index].as_deref())?;
        if !values.contains(&value) {
            values.push(value);
        }
    }
    Ok(format!("({})", values.join(", ")))
}

/// Builds a quoted SQL `IN` value list from external clipboard text.
///
/// # Errors
///
/// Returns [`AppError`] when no non-blank values are supplied.
pub fn build_mysql_external_in_values(values: &[String]) -> Result<String, AppError> {
    let mut quoted = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let value = quote_mysql_string(value);
        if !quoted.contains(&value) {
            quoted.push(value);
        }
    }
    if quoted.is_empty() {
        return Err(invalid_grid(
            "At least one non-blank clipboard value is required",
        ));
    }
    Ok(format!("({})", quoted.join(", ")))
}

/// Wraps exactly one `MySQL` read statement in a bounded count query.
///
/// # Errors
///
/// Returns [`AppError`] when the source is empty, contains multiple statements,
/// or is not a `SELECT`/CTE read candidate.
pub fn build_mysql_count_query(sql: &str) -> Result<String, AppError> {
    let statements = crate::native_mysql::split_mysql_script(sql)?;
    let [source] = statements.as_slice() else {
        return Err(invalid_grid(
            "The row-count source must contain exactly one SELECT statement",
        ));
    };
    if !crate::native_mysql::is_native_read_candidate(source)? {
        return Err(invalid_grid(
            "The row-count source must be a SELECT or CTE query",
        ));
    }
    Ok(format!(
        "SELECT COUNT(*) AS `CHAT2DB_COUNT` FROM ({source}) AS `CHAT2DB_COUNT_SOURCE`"
    ))
}

/// Builds a `MySQL` `CREATE DATABASE` statement.
///
/// # Errors
///
/// Returns [`AppError`] when the name or namespace options are invalid.
pub fn build_mysql_create_database(database: &MysqlDatabaseDefinition) -> Result<String, AppError> {
    build_create_namespace(database)
}

/// Builds a `MySQL` `CREATE DATABASE` statement for the schema alias.
///
/// # Errors
///
/// Returns [`AppError`] when the name or namespace options are invalid.
pub fn build_mysql_create_schema(schema: &MysqlDatabaseDefinition) -> Result<String, AppError> {
    build_create_namespace(schema)
}

/// Builds a `MySQL` `DROP DATABASE` statement.
///
/// # Errors
///
/// Returns [`AppError`] when the database name is invalid.
pub fn build_mysql_drop_database(name: &str, if_exists: bool) -> Result<String, AppError> {
    build_drop_namespace(name, if_exists)
}

/// Builds a `MySQL` `DROP DATABASE` statement for the schema alias.
///
/// # Errors
///
/// Returns [`AppError`] when the schema name is invalid.
pub fn build_mysql_drop_schema(name: &str, if_exists: bool) -> Result<String, AppError> {
    build_drop_namespace(name, if_exists)
}

/// Builds a structured `MySQL` `CREATE TABLE` statement.
///
/// # Errors
///
/// Returns [`AppError`] when the table, columns, indexes, or options are invalid.
pub fn build_mysql_create_table(table: &MysqlTableDefinition) -> Result<String, AppError> {
    if table.columns.is_empty() {
        return Err(invalid_ddl("A table must contain at least one column"));
    }
    let name = quote_qualified_name(&table.name)?;
    let mut definitions = table
        .columns
        .iter()
        .map(build_column_definition)
        .collect::<Result<Vec<_>, _>>()?;
    definitions.extend(
        table
            .indexes
            .iter()
            .map(build_index_definition)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut sql = format!(
        "CREATE TABLE {}{} (\n  {}\n)",
        if table.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        },
        name,
        definitions.join(",\n  ")
    );
    append_table_options(
        &mut sql,
        table.engine.as_deref(),
        table.charset.as_deref(),
        table.collation.as_deref(),
        table.comment.as_deref(),
        table.auto_increment,
    )?;
    Ok(sql)
}

/// Builds one structured `MySQL` `ALTER TABLE` statement.
///
/// # Errors
///
/// Returns [`AppError`] when a change is missing or any structured field is invalid.
pub fn build_mysql_alter_table(alter: &MysqlTableAlter) -> Result<String, AppError> {
    let table = quote_qualified_name(&alter.table)?;
    let mut clauses = Vec::new();
    for change in &alter.columns {
        clauses.push(build_column_alter(change)?);
    }
    for change in &alter.indexes {
        clauses.extend(build_index_alter(change)?);
    }
    if let Some(engine) = alter.engine.as_deref() {
        clauses.push(format!(
            "ENGINE = {}",
            validate_option_token("engine", engine)?
        ));
    }
    if let Some(charset) = alter.charset.as_deref() {
        clauses.push(format!(
            "DEFAULT CHARACTER SET = {}",
            validate_option_token("charset", charset)?
        ));
    }
    if let Some(collation) = alter.collation.as_deref() {
        clauses.push(format!(
            "COLLATE = {}",
            validate_option_token("collation", collation)?
        ));
    }
    if let Some(comment) = alter.comment.as_deref() {
        validate_comment(comment)?;
        clauses.push(format!("COMMENT = {}", quote_mysql_string(comment)));
    }
    if let Some(auto_increment) = alter.auto_increment {
        if auto_increment == 0 {
            return Err(invalid_ddl("AUTO_INCREMENT must be greater than zero"));
        }
        clauses.push(format!("AUTO_INCREMENT = {auto_increment}"));
    }
    if let Some(rename_to) = &alter.rename_to {
        clauses.push(format!("RENAME TO {}", quote_qualified_name(rename_to)?));
    }
    if clauses.is_empty() {
        return Err(invalid_ddl("ALTER TABLE requires at least one change"));
    }
    Ok(format!("ALTER TABLE {table}\n  {}", clauses.join(",\n  ")))
}

/// Builds a `MySQL` `DROP TABLE` statement.
///
/// # Errors
///
/// Returns [`AppError`] when the qualified table name is invalid.
pub fn build_mysql_drop_table(
    table: &MysqlQualifiedName,
    if_exists: bool,
) -> Result<String, AppError> {
    Ok(format!(
        "DROP TABLE {}{}",
        if if_exists { "IF EXISTS " } else { "" },
        quote_qualified_name(table)?
    ))
}

/// Builds a `MySQL` `TRUNCATE TABLE` statement.
///
/// # Errors
///
/// Returns [`AppError`] when the qualified table name is invalid.
pub fn build_mysql_truncate_table(table: &MysqlQualifiedName) -> Result<String, AppError> {
    Ok(format!("TRUNCATE TABLE {}", quote_qualified_name(table)?))
}

/// Builds ordered statements for a `MySQL` table copy.
///
/// # Errors
///
/// Returns [`AppError`] when either table name is invalid or source and target are equal.
pub fn build_mysql_copy_table(copy: &MysqlTableCopy) -> Result<Vec<String>, AppError> {
    let source = quote_qualified_name(&copy.source)?;
    let target = quote_qualified_name(&copy.target)?;
    if source == target {
        return Err(invalid_ddl("Source and target tables must be different"));
    }
    let mut statements = vec![format!(
        "CREATE TABLE {}{target} LIKE {source}",
        if copy.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        }
    )];
    if copy.copy_data {
        statements.push(format!("INSERT INTO {target} SELECT * FROM {source}"));
    }
    Ok(statements)
}

/// Builds a structured `MySQL` `CREATE OR REPLACE VIEW` statement.
///
/// # Errors
///
/// Returns [`AppError`] when identifiers or the bounded view body are invalid.
pub fn build_mysql_create_or_replace_view(view: &MysqlViewDefinition) -> Result<String, AppError> {
    build_mysql_view(view, true)
}

/// Builds a structured `MySQL` `CREATE VIEW` statement and honors `useOrReplace`.
///
/// # Errors
///
/// Returns [`AppError`] when identifiers or the bounded view body are invalid.
pub fn build_mysql_create_view(view: &MysqlViewDefinition) -> Result<String, AppError> {
    build_mysql_view(view, view.use_or_replace)
}

fn build_mysql_view(view: &MysqlViewDefinition, use_or_replace: bool) -> Result<String, AppError> {
    let body = validate_view_body(&view.body)?;
    let name = quote_qualified_name(&view.name)?;
    let columns = if view.columns.is_empty() {
        String::new()
    } else {
        let columns = view
            .columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Result<Vec<_>, _>>()?;
        format!(" ({})", columns.join(", "))
    };
    let algorithm = view.algorithm.map_or(String::new(), |algorithm| {
        format!(
            " ALGORITHM = {}",
            match algorithm {
                MysqlViewAlgorithm::Undefined => "UNDEFINED",
                MysqlViewAlgorithm::Merge => "MERGE",
                MysqlViewAlgorithm::Temptable => "TEMPTABLE",
            }
        )
    });
    let definer = view.definer.as_ref().map_or(Ok(String::new()), |definer| {
        validate_view_definer(definer).map(|()| {
            format!(
                " DEFINER = {}@{}",
                quote_mysql_string(&definer.user),
                quote_mysql_string(&definer.host)
            )
        })
    })?;
    let security = view.sql_security.map_or(String::new(), |security| {
        format!(
            " SQL SECURITY {}",
            match security {
                MysqlViewSecurity::Definer => "DEFINER",
                MysqlViewSecurity::Invoker => "INVOKER",
            }
        )
    });
    let check_option = view.check_option.map_or(String::new(), |check_option| {
        format!(
            " WITH {} CHECK OPTION",
            match check_option {
                MysqlViewCheckOption::Cascaded => "CASCADED",
                MysqlViewCheckOption::Local => "LOCAL",
            }
        )
    });
    let create = if use_or_replace {
        "CREATE OR REPLACE"
    } else {
        "CREATE"
    };
    Ok(format!(
        "{create}{algorithm}{definer}{security} VIEW {name}{columns} AS {body}{check_option}"
    ))
}

/// Builds a `MySQL` `DROP VIEW` statement.
///
/// # Errors
///
/// Returns [`AppError`] when the qualified view name is invalid.
pub fn build_mysql_drop_view(
    view: &MysqlQualifiedName,
    if_exists: bool,
) -> Result<String, AppError> {
    Ok(format!(
        "DROP VIEW {}{}",
        if if_exists { "IF EXISTS " } else { "" },
        quote_qualified_name(view)?
    ))
}

#[must_use]
pub fn mysql_table_editor_meta() -> MysqlTableEditorMeta {
    MysqlTableEditorMeta {
        column_types: MYSQL_COLUMN_TYPES
            .iter()
            .map(|type_name| editor_column_type(type_name))
            .collect(),
        charsets: MYSQL_CHARSETS
            .iter()
            .map(
                |(charset_name, default_collation_name)| MysqlEditorCharset {
                    charset_name: (*charset_name).to_owned(),
                    default_collation_name: (*default_collation_name).to_owned(),
                },
            )
            .collect(),
        collations: MYSQL_COLLATIONS
            .iter()
            .map(|collation_name| MysqlEditorCollation {
                collation_name: (*collation_name).to_owned(),
            })
            .collect(),
        index_types: ["Primary", "Normal", "Unique", "Fulltext", "Spatial"]
            .into_iter()
            .map(|type_name| MysqlEditorIndexType {
                type_name: type_name.to_owned(),
            })
            .collect(),
        default_values: ["EMPTY_STRING", "NULL", "CURRENT_TIMESTAMP"]
            .into_iter()
            .map(|default_value| MysqlEditorDefaultValue {
                default_value: default_value.to_owned(),
            })
            .collect(),
        engine_types: [
            "InnoDB",
            "MyISAM",
            "MEMORY",
            "CSV",
            "ARCHIVE",
            "BLACKHOLE",
            "FEDERATED",
            "MRG_MYISAM",
            "NDB",
        ]
        .into_iter()
        .map(|name| MysqlEditorEngineType {
            name: name.to_owned(),
            support_ttl: false,
            support_sort_order: false,
            support_skipping_indices: false,
            support_deduplication: false,
            support_settings: false,
            support_parallel_insert: false,
            support_projections: false,
            support_replication: false,
        })
        .collect(),
    }
}

fn invalid_grid(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_mysql_result_grid", message)
}

fn invalid_ddl(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_mysql_ddl", message)
}

fn validate_result_grid(
    table: &MysqlQualifiedName,
    headers: &[MysqlResultGridHeader],
    operation: &MysqlResultGridOperation,
) -> Result<(), AppError> {
    quote_qualified_name(table)?;
    if headers.len() < 2 {
        return Err(invalid_grid(
            "Result-grid headers must include the row-number column and at least one data column",
        ));
    }
    for header in &headers[1..] {
        quote_identifier(&header.name)
            .map_err(|error| invalid_grid(format!("Invalid result-grid column: {error}")))?;
    }
    reject_partial_large_values(&operation.data_list)?;
    reject_partial_large_values(&operation.old_data_list)?;
    match operation.operation_type {
        MysqlResultGridOperationType::Create => {
            require_row_length("dataList", &operation.data_list, headers.len())?;
            if !operation.old_data_list.is_empty() {
                require_row_length("oldDataList", &operation.old_data_list, headers.len())?;
            }
        }
        MysqlResultGridOperationType::Update => {
            require_row_length("dataList", &operation.data_list, headers.len())?;
            require_row_length("oldDataList", &operation.old_data_list, headers.len())?;
        }
        MysqlResultGridOperationType::Delete => {
            require_row_length("oldDataList", &operation.old_data_list, headers.len())?;
            if !operation.data_list.is_empty() {
                require_row_length("dataList", &operation.data_list, headers.len())?;
            }
        }
    }
    Ok(())
}

fn require_row_length(
    label: &str,
    row: &[Option<String>],
    expected: usize,
) -> Result<(), AppError> {
    if row.len() != expected {
        return Err(invalid_grid(format!(
            "{label} contains {} values but {expected} headers were supplied",
            row.len()
        )));
    }
    Ok(())
}

fn reject_partial_large_values(row: &[Option<String>]) -> Result<(), AppError> {
    if row.iter().flatten().any(|value| {
        value
            .to_ascii_uppercase()
            .starts_with(MYSQL_PARTIAL_LARGE_VALUE_PREFIX)
    }) {
        return Err(AppError::invalid(
            "mysql_partial_large_value_rejected",
            "Partial large-value previews cannot be written back",
        ));
    }
    Ok(())
}

fn build_result_grid_insert(
    table: &str,
    headers: &[MysqlResultGridHeader],
    row: &[Option<String>],
) -> Result<String, AppError> {
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for (header, value) in headers[1..].iter().zip(&row[1..]) {
        if is_generated_placeholder(value.as_deref()) {
            if header.auto_increment {
                continue;
            }
            return Err(invalid_grid(
                "The generated-value placeholder is only valid for auto-increment columns",
            ));
        }
        columns.push(quote_identifier(&header.name)?);
        values.push(serialize_assignment_value(header, value.as_deref())?);
    }
    if columns.is_empty() {
        return Ok(format!("INSERT INTO {table} () VALUES ()"));
    }
    Ok(format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        columns.join(", "),
        values.join(", ")
    ))
}

fn build_result_grid_update(
    table: &str,
    headers: &[MysqlResultGridHeader],
    row: &[Option<String>],
    old_row: &[Option<String>],
) -> Result<String, AppError> {
    let mut assignments = Vec::new();
    for index in 1..headers.len() {
        let header = &headers[index];
        let value = &row[index];
        if value == &old_row[index] {
            continue;
        }
        if is_generated_placeholder(value.as_deref()) {
            if header.auto_increment {
                continue;
            }
            return Err(invalid_grid(
                "The generated-value placeholder is only valid for auto-increment columns",
            ));
        }
        assignments.push(format!(
            "{} = {}",
            quote_identifier(&header.name)?,
            serialize_assignment_value(header, value.as_deref())?
        ));
    }
    if assignments.is_empty() {
        return Err(invalid_grid("UPDATE does not contain any changed values"));
    }
    let (where_clause, uses_primary_key) = build_result_grid_where(headers, old_row)?;
    Ok(format!(
        "UPDATE {table} SET {} WHERE {where_clause}{}",
        assignments.join(", "),
        if uses_primary_key { "" } else { " LIMIT 1" }
    ))
}

fn build_result_grid_delete(
    table: &str,
    headers: &[MysqlResultGridHeader],
    old_row: &[Option<String>],
) -> Result<String, AppError> {
    let (where_clause, uses_primary_key) = build_result_grid_where(headers, old_row)?;
    Ok(format!(
        "DELETE FROM {table} WHERE {where_clause}{}",
        if uses_primary_key { "" } else { " LIMIT 1" }
    ))
}

fn build_result_grid_where(
    headers: &[MysqlResultGridHeader],
    old_row: &[Option<String>],
) -> Result<(String, bool), AppError> {
    let primary_indexes = (1..headers.len())
        .filter(|index| headers[*index].primary_key)
        .collect::<Vec<_>>();
    let uses_primary_key = !primary_indexes.is_empty();
    let indexes = if uses_primary_key {
        primary_indexes
    } else {
        (1..headers.len()).collect()
    };
    let predicates = indexes
        .into_iter()
        .map(|index| {
            let header = &headers[index];
            let value = &old_row[index];
            reject_where_placeholder(value.as_deref())?;
            let column = quote_identifier(&header.name)?;
            if value.is_none() {
                Ok(format!("{column} IS NULL"))
            } else {
                Ok(format!(
                    "{column} = {}",
                    serialize_mysql_value(header, value.as_deref())?
                ))
            }
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok((predicates.join(" AND "), uses_primary_key))
}

fn validate_copy_headers(headers: &[MysqlResultGridHeader]) -> Result<(), AppError> {
    if headers.len() < 2 {
        return Err(invalid_grid(
            "Result-grid headers must include the row-number column and at least one data column",
        ));
    }
    for header in &headers[1..] {
        quote_identifier(&header.name)
            .map_err(|error| invalid_grid(format!("Invalid result-grid column: {error}")))?;
    }
    Ok(())
}

fn validate_copy_operations(
    table: &MysqlQualifiedName,
    headers: &[MysqlResultGridHeader],
    operations: &[MysqlResultGridCopyOperation],
) -> Result<(), AppError> {
    quote_qualified_name(table)?;
    validate_copy_headers(headers)?;
    if operations.is_empty() {
        return Err(invalid_grid("At least one copy operation is required"));
    }
    for operation in operations {
        require_row_length("dataList", &operation.data_list, headers.len())?;
        reject_partial_large_values(&operation.data_list)?;
        if operation.select_cols.is_empty() {
            return Err(invalid_grid("At least one result column must be selected"));
        }
        let mut selected = operation.select_cols.clone();
        selected.sort_unstable();
        if selected.windows(2).any(|indexes| indexes[0] == indexes[1]) {
            return Err(invalid_grid("Selected result columns must be unique"));
        }
        for index in selected {
            validate_selected_index(index, headers.len())?;
        }
    }
    Ok(())
}

fn validate_selected_index(index: usize, header_count: usize) -> Result<(), AppError> {
    if index == 0 || index >= header_count {
        return Err(invalid_grid(
            "Selected result columns cannot include the row number or exceed the header list",
        ));
    }
    Ok(())
}

fn build_copy_insert(
    table: &str,
    headers: &[MysqlResultGridHeader],
    operation: &MysqlResultGridCopyOperation,
) -> Result<String, AppError> {
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for index in &operation.select_cols {
        let header = &headers[*index];
        let value = operation.data_list[*index].as_deref();
        if is_generated_placeholder(value) {
            if header.auto_increment {
                continue;
            }
            return Err(invalid_grid(
                "The generated-value placeholder is only valid for auto-increment columns",
            ));
        }
        columns.push(quote_identifier(&header.name)?);
        values.push(serialize_assignment_value(header, value)?);
    }
    if columns.is_empty() {
        return Ok(format!("INSERT INTO {table} () VALUES ()"));
    }
    Ok(format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        columns.join(", "),
        values.join(", ")
    ))
}

fn build_copy_update(
    table: &str,
    headers: &[MysqlResultGridHeader],
    operation: &MysqlResultGridCopyOperation,
) -> Result<String, AppError> {
    let assignments = operation
        .select_cols
        .iter()
        .map(|index| {
            let header = &headers[*index];
            let value = operation.data_list[*index].as_deref();
            if is_generated_placeholder(value) {
                return Err(invalid_grid(
                    "The generated-value placeholder cannot be copied into an UPDATE",
                ));
            }
            Ok(format!(
                "{} = {}",
                quote_identifier(&header.name)?,
                serialize_assignment_value(header, value)?
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let (where_clause, uses_primary_key) = build_result_grid_where(headers, &operation.data_list)?;
    Ok(format!(
        "UPDATE {table} SET {} WHERE {where_clause}{}",
        assignments.join(", "),
        if uses_primary_key { "" } else { " LIMIT 1" }
    ))
}

fn build_copy_where(
    headers: &[MysqlResultGridHeader],
    operations: &[MysqlResultGridCopyOperation],
) -> Result<String, AppError> {
    let same_single_column = operations
        .first()
        .and_then(|operation| operation.select_cols.first().copied())
        .filter(|_| {
            operations.iter().all(|operation| {
                operation.select_cols.len() == 1
                    && operation.select_cols.first() == operations[0].select_cols.first()
            })
        });
    if let Some(index) = same_single_column {
        let header = &headers[index];
        let column = quote_identifier(&header.name)?;
        let mut values = Vec::<Option<String>>::new();
        for operation in operations {
            let value = operation.data_list[index]
                .as_deref()
                .map(|value| serialize_mysql_value(header, Some(value)))
                .transpose()?;
            if !values.contains(&value) {
                values.push(value);
            }
        }
        if values.len() == 1 {
            return values[0].as_ref().map_or_else(
                || Ok(format!("{column} IS NULL")),
                |value| {
                    if is_result_grid_string(header) {
                        Ok(format!("{column} LIKE {value}"))
                    } else {
                        Ok(format!("{column} = {value}"))
                    }
                },
            );
        }
        let mut predicates = Vec::new();
        if values.iter().any(Option::is_none) {
            predicates.push(format!("{column} IS NULL"));
        }
        let non_null = values.into_iter().flatten().collect::<Vec<_>>();
        if !non_null.is_empty() {
            predicates.push(format!("{column} IN ({})", non_null.join(", ")));
        }
        return Ok(predicates.join(" OR "));
    }

    operations
        .iter()
        .map(|operation| {
            let predicates = operation
                .select_cols
                .iter()
                .map(|index| {
                    let header = &headers[*index];
                    let column = quote_identifier(&header.name)?;
                    operation.data_list[*index].as_deref().map_or_else(
                        || Ok(format!("{column} IS NULL")),
                        |value| {
                            let value = serialize_mysql_value(header, Some(value))?;
                            Ok(if is_result_grid_string(header) {
                                format!("{column} LIKE {value}")
                            } else {
                                format!("{column} = {value}")
                            })
                        },
                    )
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            Ok(format!("({})", predicates.join(" AND ")))
        })
        .collect::<Result<Vec<_>, AppError>>()
        .map(|predicates| predicates.join(" OR "))
}

fn is_result_grid_string(header: &MysqlResultGridHeader) -> bool {
    supports_charset(base_type_name(&result_grid_type_name(header)))
}

fn reject_where_placeholder(value: Option<&str>) -> Result<(), AppError> {
    if value.is_some_and(|value| {
        value == MYSQL_RESULT_DEFAULT_PLACEHOLDER || value == MYSQL_RESULT_GENERATED_PLACEHOLDER
    }) {
        return Err(invalid_grid(
            "Write placeholders cannot be used to identify an existing row",
        ));
    }
    Ok(())
}

fn is_generated_placeholder(value: Option<&str>) -> bool {
    value == Some(MYSQL_RESULT_GENERATED_PLACEHOLDER)
}

fn serialize_assignment_value(
    header: &MysqlResultGridHeader,
    value: Option<&str>,
) -> Result<String, AppError> {
    if value == Some(MYSQL_RESULT_DEFAULT_PLACEHOLDER) {
        return Ok("DEFAULT".to_owned());
    }
    serialize_mysql_value(header, value)
}

fn serialize_mysql_value(
    header: &MysqlResultGridHeader,
    value: Option<&str>,
) -> Result<String, AppError> {
    let Some(value) = value else {
        return Ok("NULL".to_owned());
    };
    if value == MYSQL_RESULT_GENERATED_PLACEHOLDER {
        return Err(invalid_grid(
            "The generated-value placeholder cannot be serialized as data",
        ));
    }
    let type_name = result_grid_type_name(header);
    serialize_typed_literal(&type_name, value).map_err(|error| {
        invalid_grid(format!(
            "Column {} has an invalid value: {error}",
            header.name
        ))
    })
}

fn result_grid_type_name(header: &MysqlResultGridHeader) -> String {
    let source = if header.column_type.trim().is_empty() {
        &header.data_type
    } else {
        &header.column_type
    };
    normalize_type_name(source)
}

fn serialize_typed_literal(type_name: &str, value: &str) -> Result<String, AppError> {
    let base_type = base_type_name(type_name);
    let trimmed = value.trim();
    if is_integer_type(base_type) || base_type == "BIT" {
        if !is_integer_literal(trimmed) {
            return Err(invalid_ddl("Expected an integer literal"));
        }
        if (type_name.contains("UNSIGNED") || base_type == "BIT") && trimmed.starts_with('-') {
            return Err(invalid_ddl(
                "Unsigned columns cannot contain negative values",
            ));
        }
        return Ok(trimmed.to_owned());
    }
    if matches!(base_type, "DECIMAL" | "NUMERIC") {
        if !is_decimal_literal(trimmed, false) {
            return Err(invalid_ddl("Expected a decimal literal"));
        }
        if type_name.contains("UNSIGNED") && trimmed.starts_with('-') {
            return Err(invalid_ddl(
                "Unsigned columns cannot contain negative values",
            ));
        }
        return Ok(trimmed.to_owned());
    }
    if matches!(base_type, "FLOAT" | "DOUBLE" | "REAL") {
        if !is_decimal_literal(trimmed, true) {
            return Err(invalid_ddl("Expected a finite numeric literal"));
        }
        if type_name.contains("UNSIGNED") && trimmed.starts_with('-') {
            return Err(invalid_ddl(
                "Unsigned columns cannot contain negative values",
            ));
        }
        return Ok(trimmed.to_owned());
    }
    if matches!(base_type, "BOOL" | "BOOLEAN") {
        return match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok("TRUE".to_owned()),
            "false" | "0" => Ok("FALSE".to_owned()),
            _ => Err(invalid_ddl("Expected a boolean literal")),
        };
    }
    if is_binary_type(base_type) {
        let bytes = STANDARD
            .decode(value)
            .map_err(|_| invalid_ddl("Expected a Base64-encoded binary value"))?;
        let mut literal = String::with_capacity(bytes.len() * 2 + 3);
        literal.push_str("X'");
        for byte in bytes {
            write!(&mut literal, "{byte:02X}").expect("writing to a String cannot fail");
        }
        literal.push('\'');
        return Ok(literal);
    }
    Ok(quote_mysql_string(value))
}

fn is_integer_literal(value: &str) -> bool {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_decimal_literal(value: &str, allow_exponent: bool) -> bool {
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    if value.is_empty() {
        return false;
    }
    let (mantissa, exponent) = if allow_exponent {
        value.find(['e', 'E']).map_or((value, None), |index| {
            (&value[..index], Some(&value[index + 1..]))
        })
    } else {
        (value, None)
    };
    if exponent.is_some_and(|exponent| !is_integer_literal(exponent)) {
        return false;
    }
    let mut parts = mantissa.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let whole_valid = whole.bytes().all(|byte| byte.is_ascii_digit());
    let fraction_valid =
        fraction.is_none_or(|fraction| fraction.bytes().all(|byte| byte.is_ascii_digit()));
    whole_valid
        && fraction_valid
        && (!whole.is_empty() || fraction.is_some_and(|fraction| !fraction.is_empty()))
}

fn build_create_namespace(database: &MysqlDatabaseDefinition) -> Result<String, AppError> {
    let mut sql = format!(
        "CREATE DATABASE {}{}",
        if database.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        },
        quote_identifier(&database.name)?
    );
    if let Some(charset) = database.charset.as_deref() {
        sql.push_str(" DEFAULT CHARACTER SET = ");
        sql.push_str(&validate_option_token("charset", charset)?);
    }
    if let Some(collation) = database.collation.as_deref() {
        sql.push_str(" COLLATE = ");
        sql.push_str(&validate_option_token("collation", collation)?);
    }
    Ok(sql)
}

fn build_drop_namespace(name: &str, if_exists: bool) -> Result<String, AppError> {
    Ok(format!(
        "DROP DATABASE {}{}",
        if if_exists { "IF EXISTS " } else { "" },
        quote_identifier(name)?
    ))
}

fn build_column_definition(column: &MysqlColumnDefinition) -> Result<String, AppError> {
    let name = quote_identifier(&column.name)?;
    let normalized_type = normalize_column_type(&column.type_name, column.unsigned)?;
    let base_type = base_type_name(&normalized_type);
    validate_column_shape(column, base_type)?;
    let mut sql = format!(
        "{name} {}",
        render_column_type(column, &normalized_type, base_type)?
    );
    if let Some(charset) = column.charset.as_deref() {
        sql.push_str(" CHARACTER SET ");
        sql.push_str(&validate_option_token("charset", charset)?);
    }
    if let Some(collation) = column.collation.as_deref() {
        sql.push_str(" COLLATE ");
        sql.push_str(&validate_option_token("collation", collation)?);
    }
    sql.push_str(if column.nullable {
        " NULL"
    } else {
        " NOT NULL"
    });
    if let Some(default_value) = column.default_value.as_deref() {
        sql.push_str(" DEFAULT ");
        sql.push_str(&build_column_default(&normalized_type, default_value)?);
    }
    if column.on_update_current_timestamp {
        sql.push_str(" ON UPDATE CURRENT_TIMESTAMP");
    }
    if column.auto_increment {
        sql.push_str(" AUTO_INCREMENT");
    }
    if let Some(comment) = column.comment.as_deref() {
        validate_comment(comment)?;
        sql.push_str(" COMMENT ");
        sql.push_str(&quote_mysql_string(comment));
    }
    Ok(sql)
}

fn normalize_column_type(type_name: &str, unsigned: bool) -> Result<String, AppError> {
    let normalized = normalize_type_name(type_name);
    if normalized.is_empty() || normalized.contains(['(', ')', ',', '\'', '"', '`', ';']) {
        return Err(invalid_ddl(
            "Column type must be a structured MySQL type name",
        ));
    }
    let base = base_type_name(&normalized);
    if !MYSQL_BASE_COLUMN_TYPES.contains(&base) {
        return Err(invalid_ddl(format!(
            "Unsupported MySQL column type: {type_name}"
        )));
    }
    let already_unsigned = normalized.ends_with(" UNSIGNED");
    if normalized.split_whitespace().count() > usize::from(already_unsigned) + 1 {
        return Err(invalid_ddl("Column type contains unsupported modifiers"));
    }
    if (unsigned || already_unsigned) && !supports_unsigned(base) {
        return Err(invalid_ddl(format!("{base} does not support UNSIGNED")));
    }
    Ok(if unsigned && !already_unsigned {
        format!("{base} UNSIGNED")
    } else {
        normalized
    })
}

fn validate_column_shape(column: &MysqlColumnDefinition, base_type: &str) -> Result<(), AppError> {
    if matches!(base_type, "CHAR" | "VARCHAR" | "BINARY" | "VARBINARY")
        && column.length.is_none_or(|length| length == 0)
    {
        return Err(invalid_ddl(format!(
            "{} requires a positive length",
            column.type_name
        )));
    }
    if base_type == "BIT"
        && column
            .length
            .is_some_and(|length| !(1..=64).contains(&length))
    {
        return Err(invalid_ddl("BIT length must be between 1 and 64"));
    }
    if matches!(base_type, "DECIMAL" | "NUMERIC") {
        if column
            .length
            .is_some_and(|precision| !(1..=65).contains(&precision))
        {
            return Err(invalid_ddl("DECIMAL precision must be between 1 and 65"));
        }
        if column.scale.is_some_and(|scale| scale > 30) {
            return Err(invalid_ddl("DECIMAL scale must be at most 30"));
        }
        if column
            .length
            .zip(column.scale)
            .is_some_and(|(precision, scale)| scale > precision)
        {
            return Err(invalid_ddl("DECIMAL scale cannot exceed its precision"));
        }
    } else if column.scale.is_some() {
        return Err(invalid_ddl(
            "Scale is only supported for DECIMAL and NUMERIC",
        ));
    }
    if is_temporal_fraction_type(base_type) && column.length.is_some_and(|precision| precision > 6)
    {
        return Err(invalid_ddl(
            "Temporal fractional precision must be at most 6",
        ));
    }
    if !matches!(
        base_type,
        "BIT" | "CHAR" | "VARCHAR" | "BINARY" | "VARBINARY" | "DECIMAL" | "NUMERIC"
    ) && !is_temporal_fraction_type(base_type)
        && column.length.is_some()
    {
        return Err(invalid_ddl(format!(
            "{base_type} does not support a length"
        )));
    }
    if matches!(base_type, "ENUM" | "SET") && column.enum_values.is_empty() {
        return Err(invalid_ddl(format!(
            "{base_type} requires at least one value"
        )));
    }
    if !matches!(base_type, "ENUM" | "SET") && !column.enum_values.is_empty() {
        return Err(invalid_ddl("enumValues are only valid for ENUM and SET"));
    }
    if column.auto_increment && !is_integer_type(base_type) {
        return Err(invalid_ddl("AUTO_INCREMENT requires an integer column"));
    }
    if column.on_update_current_timestamp && !matches!(base_type, "DATETIME" | "TIMESTAMP") {
        return Err(invalid_ddl(
            "ON UPDATE CURRENT_TIMESTAMP requires DATETIME or TIMESTAMP",
        ));
    }
    if column.charset.is_some() && !supports_charset(base_type) {
        return Err(invalid_ddl(format!(
            "{base_type} does not support a charset"
        )));
    }
    if column.collation.is_some() && !supports_charset(base_type) {
        return Err(invalid_ddl(format!(
            "{base_type} does not support a collation"
        )));
    }
    Ok(())
}

fn render_column_type(
    column: &MysqlColumnDefinition,
    normalized_type: &str,
    base_type: &str,
) -> Result<String, AppError> {
    let suffix = if normalized_type.ends_with(" UNSIGNED") {
        " UNSIGNED"
    } else {
        ""
    };
    if matches!(base_type, "ENUM" | "SET") {
        let values = column
            .enum_values
            .iter()
            .map(|value| quote_mysql_string(value))
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(format!("{base_type}({values})"));
    }
    let dimensions = match (column.length, column.scale) {
        (Some(length), Some(scale)) => format!("({length},{scale})"),
        (Some(length), None) => format!("({length})"),
        (None, Some(scale)) if matches!(base_type, "DECIMAL" | "NUMERIC") => {
            format!("(10,{scale})")
        }
        (None, Some(_)) => return Err(invalid_ddl("Scale requires a precision")),
        (None, None) => String::new(),
    };
    Ok(format!("{base_type}{dimensions}{suffix}"))
}

fn build_column_default(type_name: &str, default_value: &str) -> Result<String, AppError> {
    let trimmed = default_value.trim();
    if trimmed.eq_ignore_ascii_case("EMPTY_STRING") {
        return Ok("''".to_owned());
    }
    if trimmed.eq_ignore_ascii_case("NULL") {
        return Ok("NULL".to_owned());
    }
    if trimmed.eq_ignore_ascii_case("CURRENT_TIMESTAMP") || trimmed.eq_ignore_ascii_case("NOW()") {
        if matches!(base_type_name(type_name), "DATETIME" | "TIMESTAMP") {
            return Ok("CURRENT_TIMESTAMP".to_owned());
        }
        return Err(invalid_ddl(
            "CURRENT_TIMESTAMP is only valid for DATETIME and TIMESTAMP defaults",
        ));
    }
    serialize_typed_literal(type_name, default_value)
}

fn build_index_definition(index: &MysqlIndexDefinition) -> Result<String, AppError> {
    if index.columns.is_empty() {
        return Err(invalid_ddl("An index must contain at least one column"));
    }
    let keyword = match index.kind {
        MysqlIndexKind::Primary => "PRIMARY KEY",
        MysqlIndexKind::Normal => "INDEX",
        MysqlIndexKind::Unique => "UNIQUE INDEX",
        MysqlIndexKind::Fulltext => "FULLTEXT INDEX",
        MysqlIndexKind::Spatial => "SPATIAL INDEX",
    };
    let name = if index.kind == MysqlIndexKind::Primary {
        String::new()
    } else {
        let name = index
            .name
            .as_deref()
            .ok_or_else(|| invalid_ddl("Non-primary indexes require a name"))?;
        format!(" {}", quote_identifier(name)?)
    };
    let columns = index
        .columns
        .iter()
        .map(|column| {
            let mut sql = quote_identifier(&column.name)?;
            if let Some(prefix_length) = column.prefix_length {
                if prefix_length == 0 {
                    return Err(invalid_ddl("Index prefix length must be greater than zero"));
                }
                write!(&mut sql, "({prefix_length})").expect("writing to a String cannot fail");
            }
            if let Some(order) = &column.order {
                sql.push(' ');
                sql.push_str(match order {
                    MysqlSortOrder::Asc => "ASC",
                    MysqlSortOrder::Desc => "DESC",
                });
            }
            Ok(sql)
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let mut sql = format!("{keyword}{name} ({})", columns.join(", "));
    if let Some(method) = &index.method {
        if matches!(
            index.kind,
            MysqlIndexKind::Fulltext | MysqlIndexKind::Spatial
        ) {
            return Err(invalid_ddl(
                "FULLTEXT and SPATIAL indexes do not accept an index method",
            ));
        }
        sql.push_str(match method {
            MysqlIndexMethod::Btree => " USING BTREE",
            MysqlIndexMethod::Hash => " USING HASH",
        });
    }
    if let Some(comment) = index.comment.as_deref() {
        validate_comment(comment)?;
        sql.push_str(" COMMENT ");
        sql.push_str(&quote_mysql_string(comment));
    }
    Ok(sql)
}

fn build_column_alter(change: &MysqlColumnAlter) -> Result<String, AppError> {
    match change {
        MysqlColumnAlter::Add { column, position } => Ok(format!(
            "ADD COLUMN {}{}",
            build_column_definition(column)?,
            build_column_position(position.as_ref())?
        )),
        MysqlColumnAlter::Modify {
            old_name,
            column,
            position,
        } => {
            let definition = build_column_definition(column)?;
            let operation = if old_name == &column.name {
                format!("MODIFY COLUMN {definition}")
            } else {
                format!("CHANGE COLUMN {} {definition}", quote_identifier(old_name)?)
            };
            Ok(format!(
                "{operation}{}",
                build_column_position(position.as_ref())?
            ))
        }
        MysqlColumnAlter::Delete { name } => Ok(format!("DROP COLUMN {}", quote_identifier(name)?)),
    }
}

fn build_column_position(position: Option<&MysqlColumnPosition>) -> Result<String, AppError> {
    match position {
        None => Ok(String::new()),
        Some(MysqlColumnPosition::First) => Ok(" FIRST".to_owned()),
        Some(MysqlColumnPosition::After(column)) => {
            Ok(format!(" AFTER {}", quote_identifier(column)?))
        }
    }
}

fn build_index_alter(change: &MysqlIndexAlter) -> Result<Vec<String>, AppError> {
    match change {
        MysqlIndexAlter::Add { index } => {
            Ok(vec![format!("ADD {}", build_index_definition(index)?)])
        }
        MysqlIndexAlter::Modify {
            old_kind,
            old_name,
            index,
        } => Ok(vec![
            build_drop_index_clause(*old_kind, old_name.as_deref())?,
            format!("ADD {}", build_index_definition(index)?),
        ]),
        MysqlIndexAlter::Delete { kind, name } => {
            Ok(vec![build_drop_index_clause(*kind, name.as_deref())?])
        }
    }
}

fn build_drop_index_clause(kind: MysqlIndexKind, name: Option<&str>) -> Result<String, AppError> {
    if kind == MysqlIndexKind::Primary {
        Ok("DROP PRIMARY KEY".to_owned())
    } else {
        let name = name.ok_or_else(|| invalid_ddl("Dropping an index requires its name"))?;
        Ok(format!("DROP INDEX {}", quote_identifier(name)?))
    }
}

fn append_table_options(
    sql: &mut String,
    engine: Option<&str>,
    charset: Option<&str>,
    collation: Option<&str>,
    comment: Option<&str>,
    auto_increment: Option<u64>,
) -> Result<(), AppError> {
    if let Some(engine) = engine {
        sql.push_str(" ENGINE = ");
        sql.push_str(&validate_option_token("engine", engine)?);
    }
    if let Some(charset) = charset {
        sql.push_str(" DEFAULT CHARACTER SET = ");
        sql.push_str(&validate_option_token("charset", charset)?);
    }
    if let Some(collation) = collation {
        sql.push_str(" COLLATE = ");
        sql.push_str(&validate_option_token("collation", collation)?);
    }
    if let Some(auto_increment) = auto_increment {
        if auto_increment == 0 {
            return Err(invalid_ddl("AUTO_INCREMENT must be greater than zero"));
        }
        write!(sql, " AUTO_INCREMENT = {auto_increment}").expect("writing to a String cannot fail");
    }
    if let Some(comment) = comment {
        validate_comment(comment)?;
        sql.push_str(" COMMENT = ");
        sql.push_str(&quote_mysql_string(comment));
    }
    Ok(())
}

fn validate_view_body(body: &str) -> Result<String, AppError> {
    if body.trim().is_empty() {
        return Err(invalid_ddl("View body cannot be empty"));
    }
    if body.len() > MAX_VIEW_BODY_BYTES {
        return Err(invalid_ddl(format!(
            "View body exceeds the {MAX_VIEW_BODY_BYTES}-byte limit"
        )));
    }
    if body.contains('\0') {
        return Err(invalid_ddl("View body cannot contain NUL bytes"));
    }
    let statements = crate::native_mysql::split_mysql_script(body)?;
    let [statement] = statements.as_slice() else {
        return Err(invalid_ddl(
            "View body must contain exactly one SELECT or CTE statement",
        ));
    };
    if !crate::native_mysql::is_native_read_candidate(statement)? {
        return Err(invalid_ddl("View body must be a SELECT or CTE statement"));
    }
    Ok(statement.clone())
}

fn validate_view_definer(definer: &MysqlViewDefiner) -> Result<(), AppError> {
    for (label, value, max_chars) in [
        ("user", definer.user.as_str(), 32),
        ("host", definer.host.as_str(), 255),
    ] {
        if value.is_empty()
            || value.chars().count() > max_chars
            || value.chars().any(char::is_control)
        {
            return Err(invalid_ddl(format!("Invalid MySQL view definer {label}")));
        }
    }
    Ok(())
}

fn validate_comment(comment: &str) -> Result<(), AppError> {
    if comment.len() > MAX_COMMENT_BYTES {
        return Err(invalid_ddl(format!(
            "Comment exceeds the {MAX_COMMENT_BYTES}-byte limit"
        )));
    }
    if comment.contains('\0') {
        return Err(invalid_ddl("Comments cannot contain NUL bytes"));
    }
    Ok(())
}

fn quote_qualified_name(name: &MysqlQualifiedName) -> Result<String, AppError> {
    let database = non_empty_option(name.database_name.as_deref());
    let schema = non_empty_option(name.schema_name.as_deref());
    if database
        .zip(schema)
        .is_some_and(|(database, schema)| database != schema)
    {
        return Err(invalid_ddl(
            "MySQL databaseName and schemaName must identify the same namespace",
        ));
    }
    let namespace = database.or(schema);
    let object = quote_identifier(&name.name)?;
    namespace.map_or(Ok(object.clone()), |namespace| {
        Ok(format!("{}.{object}", quote_identifier(namespace)?))
    })
}

fn non_empty_option(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

fn quote_identifier(identifier: &str) -> Result<String, AppError> {
    let identifier = identifier.trim();
    if identifier.is_empty() {
        return Err(invalid_ddl("MySQL identifiers cannot be empty"));
    }
    if identifier.chars().count() > MAX_IDENTIFIER_CHARS {
        return Err(invalid_ddl(format!(
            "MySQL identifiers cannot exceed {MAX_IDENTIFIER_CHARS} characters"
        )));
    }
    if identifier.chars().any(char::is_control) {
        return Err(invalid_ddl(
            "MySQL identifiers cannot contain control characters",
        ));
    }
    Ok(format!("`{}`", identifier.replace('`', "``")))
}

fn quote_mysql_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for character in value.chars() {
        match character {
            '\0' => escaped.push_str("\\0"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{001a}' => escaped.push_str("\\Z"),
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("''"),
            _ => escaped.push(character),
        }
    }
    escaped.push('\'');
    escaped
}

fn validate_option_token(label: &str, value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(invalid_ddl(format!("Invalid MySQL {label}")));
    }
    Ok(value.to_owned())
}

fn normalize_type_name(type_name: &str) -> String {
    type_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

fn base_type_name(type_name: &str) -> &str {
    type_name.split(['(', ' ']).next().unwrap_or_default()
}

fn is_integer_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT"
    )
}

fn supports_unsigned(type_name: &str) -> bool {
    is_integer_type(type_name)
        || matches!(
            type_name,
            "DECIMAL" | "NUMERIC" | "FLOAT" | "DOUBLE" | "REAL"
        )
}

fn supports_charset(type_name: &str) -> bool {
    matches!(
        type_name,
        "CHAR" | "VARCHAR" | "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM" | "SET"
    )
}

fn is_binary_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB"
    )
}

fn is_temporal_fraction_type(type_name: &str) -> bool {
    matches!(type_name, "TIME" | "DATETIME" | "TIMESTAMP")
}

fn editor_column_type(type_name: &str) -> MysqlEditorColumnType {
    let base_type = base_type_name(type_name);
    MysqlEditorColumnType {
        type_name: type_name.to_owned(),
        support_length: matches!(
            base_type,
            "BIT"
                | "DECIMAL"
                | "NUMERIC"
                | "FLOAT"
                | "DOUBLE"
                | "REAL"
                | "TIME"
                | "DATETIME"
                | "TIMESTAMP"
                | "CHAR"
                | "VARCHAR"
                | "BINARY"
                | "VARBINARY"
        ),
        support_scale: matches!(
            base_type,
            "DECIMAL" | "NUMERIC" | "FLOAT" | "DOUBLE" | "REAL"
        ),
        support_nullable: true,
        support_auto_increment: is_integer_type(base_type),
        support_charset: supports_charset(base_type),
        support_collation: supports_charset(base_type),
        support_comments: true,
        support_default_value: !matches!(
            base_type,
            "TINYBLOB"
                | "BLOB"
                | "MEDIUMBLOB"
                | "LONGBLOB"
                | "TINYTEXT"
                | "TEXT"
                | "MEDIUMTEXT"
                | "LONGTEXT"
                | "GEOMETRY"
                | "POINT"
                | "LINESTRING"
                | "POLYGON"
                | "MULTIPOINT"
                | "MULTILINESTRING"
                | "MULTIPOLYGON"
                | "GEOMETRYCOLLECTION"
                | "JSON"
        ),
        support_extent: matches!(base_type, "ENUM" | "SET"),
        support_value: matches!(base_type, "ENUM" | "SET"),
        support_unit: false,
        support_on_update_current_timestamp: matches!(base_type, "DATETIME" | "TIMESTAMP"),
    }
}

const MYSQL_BASE_COLUMN_TYPES: &[&str] = &[
    "BIT",
    "TINYINT",
    "SMALLINT",
    "MEDIUMINT",
    "INT",
    "INTEGER",
    "BIGINT",
    "DECIMAL",
    "NUMERIC",
    "FLOAT",
    "DOUBLE",
    "REAL",
    "BOOL",
    "BOOLEAN",
    "DATE",
    "DATETIME",
    "TIMESTAMP",
    "TIME",
    "YEAR",
    "CHAR",
    "VARCHAR",
    "BINARY",
    "VARBINARY",
    "TINYBLOB",
    "BLOB",
    "MEDIUMBLOB",
    "LONGBLOB",
    "TINYTEXT",
    "TEXT",
    "MEDIUMTEXT",
    "LONGTEXT",
    "ENUM",
    "SET",
    "JSON",
    "GEOMETRY",
    "POINT",
    "LINESTRING",
    "POLYGON",
    "MULTIPOINT",
    "MULTILINESTRING",
    "MULTIPOLYGON",
    "GEOMETRYCOLLECTION",
];

const MYSQL_COLUMN_TYPES: &[&str] = &[
    "BIT",
    "TINYINT",
    "TINYINT UNSIGNED",
    "SMALLINT",
    "SMALLINT UNSIGNED",
    "MEDIUMINT",
    "MEDIUMINT UNSIGNED",
    "INT",
    "INT UNSIGNED",
    "BIGINT",
    "BIGINT UNSIGNED",
    "DECIMAL",
    "DECIMAL UNSIGNED",
    "NUMERIC",
    "NUMERIC UNSIGNED",
    "FLOAT",
    "FLOAT UNSIGNED",
    "DOUBLE",
    "DOUBLE UNSIGNED",
    "REAL",
    "REAL UNSIGNED",
    "BOOL",
    "BOOLEAN",
    "DATE",
    "DATETIME",
    "TIMESTAMP",
    "TIME",
    "YEAR",
    "CHAR",
    "VARCHAR",
    "BINARY",
    "VARBINARY",
    "TINYBLOB",
    "BLOB",
    "MEDIUMBLOB",
    "LONGBLOB",
    "TINYTEXT",
    "TEXT",
    "MEDIUMTEXT",
    "LONGTEXT",
    "ENUM",
    "SET",
    "JSON",
    "GEOMETRY",
    "POINT",
    "LINESTRING",
    "POLYGON",
    "MULTIPOINT",
    "MULTILINESTRING",
    "MULTIPOLYGON",
    "GEOMETRYCOLLECTION",
];

const MYSQL_CHARSETS: &[(&str, &str)] = &[
    ("utf8mb4", "utf8mb4_0900_ai_ci"),
    ("utf8mb3", "utf8mb3_general_ci"),
    ("latin1", "latin1_swedish_ci"),
    ("ascii", "ascii_general_ci"),
    ("binary", "binary"),
    ("gbk", "gbk_chinese_ci"),
    ("gb18030", "gb18030_chinese_ci"),
];

const MYSQL_COLLATIONS: &[&str] = &[
    "utf8mb4_0900_ai_ci",
    "utf8mb4_0900_as_ci",
    "utf8mb4_0900_bin",
    "utf8mb4_general_ci",
    "utf8mb4_unicode_ci",
    "utf8mb4_bin",
    "utf8mb3_general_ci",
    "latin1_swedish_ci",
    "latin1_general_ci",
    "ascii_general_ci",
    "binary",
    "gbk_chinese_ci",
    "gb18030_chinese_ci",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str) -> MysqlQualifiedName {
        MysqlQualifiedName::in_database("app`db", name)
    }

    fn row_header(
        name: &str,
        column_type: &str,
        primary_key: bool,
        auto_increment: bool,
    ) -> MysqlResultGridHeader {
        MysqlResultGridHeader {
            name: name.to_owned(),
            column_type: column_type.to_owned(),
            data_type: column_type.to_owned(),
            primary_key,
            auto_increment,
        }
    }

    fn headers(with_primary_key: bool) -> Vec<MysqlResultGridHeader> {
        vec![
            row_header("#", "INTEGER", false, false),
            row_header("id", "BIGINT UNSIGNED", with_primary_key, true),
            row_header("display`name", "VARCHAR", false, false),
            row_header("score", "DECIMAL", false, false),
            row_header("note", "TEXT", false, false),
        ]
    }

    fn column(name: &str, type_name: &str) -> MysqlColumnDefinition {
        MysqlColumnDefinition {
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            length: None,
            scale: None,
            unsigned: false,
            nullable: false,
            default_value: None,
            auto_increment: false,
            charset: None,
            collation: None,
            comment: None,
            enum_values: Vec::new(),
            on_update_current_timestamp: false,
        }
    }

    fn index(kind: MysqlIndexKind, name: Option<&str>, columns: &[&str]) -> MysqlIndexDefinition {
        MysqlIndexDefinition {
            kind,
            name: name.map(str::to_owned),
            columns: columns
                .iter()
                .map(|name| MysqlIndexColumn {
                    name: (*name).to_owned(),
                    prefix_length: None,
                    order: None,
                })
                .collect(),
            method: None,
            comment: None,
        }
    }

    #[test]
    fn qualified_names_escape_every_identifier_segment() {
        let name = MysqlQualifiedName {
            database_name: Some("sales`prod".to_owned()),
            schema_name: Some("sales`prod".to_owned()),
            name: "order`line".to_owned(),
        };
        assert_eq!(
            quote_qualified_name(&name).expect("qualified name"),
            "`sales``prod`.`order``line`"
        );

        let mismatched = MysqlQualifiedName {
            database_name: Some("one".to_owned()),
            schema_name: Some("two".to_owned()),
            name: "items".to_owned(),
        };
        assert!(quote_qualified_name(&mismatched).is_err());
    }

    #[test]
    fn grid_insert_ignores_row_number_and_generated_value() {
        let operation = MysqlResultGridOperation {
            operation_type: MysqlResultGridOperationType::Create,
            data_list: vec![
                Some("99".to_owned()),
                Some(MYSQL_RESULT_GENERATED_PLACEHOLDER.to_owned()),
                Some("O'Reilly\\notes\nnext".to_owned()),
                Some(MYSQL_RESULT_DEFAULT_PLACEHOLDER.to_owned()),
                None,
            ],
            old_data_list: Vec::new(),
        };
        let sql = build_mysql_result_grid_sql(&table("people"), &headers(true), &operation)
            .expect("insert SQL");
        assert_eq!(
            sql,
            "INSERT INTO `app``db`.`people` (`display``name`, `score`, `note`) VALUES ('O''Reilly\\\\notes\\nnext', DEFAULT, NULL)"
        );
    }

    #[test]
    fn grid_insert_with_only_generated_column_uses_empty_row_syntax() {
        let headers = vec![
            row_header("#", "INTEGER", false, false),
            row_header("id", "BIGINT", true, true),
        ];
        let operation = MysqlResultGridOperation {
            operation_type: MysqlResultGridOperationType::Create,
            data_list: vec![
                Some("1".to_owned()),
                Some(MYSQL_RESULT_GENERATED_PLACEHOLDER.to_owned()),
            ],
            old_data_list: Vec::new(),
        };
        assert_eq!(
            build_mysql_result_grid_sql(&table("people"), &headers, &operation)
                .expect("insert SQL"),
            "INSERT INTO `app``db`.`people` () VALUES ()"
        );
    }

    #[test]
    fn grid_update_prefers_primary_key_and_omits_limit() {
        let old = vec![
            Some("1".to_owned()),
            Some("42".to_owned()),
            Some("old".to_owned()),
            Some("1.5".to_owned()),
            None,
        ];
        let mut new = old.clone();
        new[2] = Some("new".to_owned());
        new[4] = Some("memo".to_owned());
        let sql = build_mysql_result_grid_sql(
            &table("people"),
            &headers(true),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Update,
                data_list: new,
                old_data_list: old,
            },
        )
        .expect("update SQL");
        assert_eq!(
            sql,
            "UPDATE `app``db`.`people` SET `display``name` = 'new', `note` = 'memo' WHERE `id` = 42"
        );
    }

    #[test]
    fn grid_update_without_primary_key_uses_all_old_values_and_limit() {
        let old = vec![
            Some("1".to_owned()),
            Some("42".to_owned()),
            Some("old".to_owned()),
            Some("1.5".to_owned()),
            None,
        ];
        let mut new = old.clone();
        new[2] = Some("new".to_owned());
        let sql = build_mysql_result_grid_sql(
            &table("people"),
            &headers(false),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Update,
                data_list: new,
                old_data_list: old,
            },
        )
        .expect("update SQL");
        assert_eq!(
            sql,
            "UPDATE `app``db`.`people` SET `display``name` = 'new' WHERE `id` = 42 AND `display``name` = 'old' AND `score` = 1.5 AND `note` IS NULL LIMIT 1"
        );
    }

    #[test]
    fn grid_delete_without_primary_key_is_single_row_safe() {
        let old = vec![
            Some("1".to_owned()),
            Some("42".to_owned()),
            Some("old".to_owned()),
            Some("1.5".to_owned()),
            None,
        ];
        let sql = build_mysql_result_grid_sql(
            &table("people"),
            &headers(false),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Delete,
                data_list: Vec::new(),
                old_data_list: old,
            },
        )
        .expect("delete SQL");
        assert!(sql.ends_with("`note` IS NULL LIMIT 1"));
    }

    #[test]
    fn grid_rejects_mismatched_rows_partial_values_and_numeric_injection() {
        let mismatched = build_mysql_result_grid_sql(
            &table("people"),
            &headers(true),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Create,
                data_list: vec![None],
                old_data_list: Vec::new(),
            },
        )
        .expect_err("mismatched row");
        assert_eq!(mismatched.api_error().code, "invalid_mysql_result_grid");

        let partial = build_mysql_result_grid_sql(
            &table("people"),
            &headers(true),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Create,
                data_list: vec![
                    None,
                    Some(MYSQL_RESULT_GENERATED_PLACEHOLDER.to_owned()),
                    Some("CHAT2DB_LARGE_VALUE_PREVIEW:PARTIAL".to_owned()),
                    Some("1".to_owned()),
                    None,
                ],
                old_data_list: Vec::new(),
            },
        )
        .expect_err("partial value");
        assert_eq!(
            partial.api_error().code,
            "mysql_partial_large_value_rejected"
        );

        let injection = build_mysql_result_grid_sql(
            &table("people"),
            &headers(true),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Create,
                data_list: vec![
                    None,
                    Some("1 OR 1=1".to_owned()),
                    Some("safe".to_owned()),
                    Some("1.0".to_owned()),
                    None,
                ],
                old_data_list: Vec::new(),
            },
        )
        .expect_err("numeric injection");
        assert_eq!(injection.api_error().code, "invalid_mysql_result_grid");
    }

    #[test]
    fn grid_binary_values_are_base64_decoded_to_hex_literals() {
        let headers = vec![
            row_header("#", "INTEGER", false, false),
            row_header("payload", "BLOB", false, false),
        ];
        let operation = MysqlResultGridOperation {
            operation_type: MysqlResultGridOperationType::Create,
            data_list: vec![None, Some("AP8Q".to_owned())],
            old_data_list: Vec::new(),
        };
        assert_eq!(
            build_mysql_result_grid_sql(&table("files"), &headers, &operation)
                .expect("binary insert"),
            "INSERT INTO `app``db`.`files` (`payload`) VALUES (X'00FF10')"
        );

        let invalid = MysqlResultGridOperation {
            data_list: vec![None, Some("not base64!".to_owned())],
            ..operation
        };
        assert!(build_mysql_result_grid_sql(&table("files"), &headers, &invalid).is_err());
    }

    #[test]
    fn empty_grid_update_is_rejected_without_panicking() {
        let row = vec![
            None,
            Some("42".to_owned()),
            Some("same".to_owned()),
            Some("1".to_owned()),
            None,
        ];
        let error = build_mysql_result_grid_sql(
            &table("people"),
            &headers(true),
            &MysqlResultGridOperation {
                operation_type: MysqlResultGridOperationType::Update,
                data_list: row.clone(),
                old_data_list: row,
            },
        )
        .expect_err("no changes");
        assert_eq!(error.api_error().code, "invalid_mysql_result_grid");
    }

    #[test]
    fn grid_copy_builds_selected_insert_update_and_where_sql() {
        let row = vec![
            Some("1".to_owned()),
            Some("42".to_owned()),
            Some("O'Reilly".to_owned()),
            Some("1.5".to_owned()),
            None,
        ];
        let insert = MysqlResultGridCopyOperation {
            operation_type: MysqlResultGridCopyOperationType::Create,
            data_list: row.clone(),
            select_cols: vec![2, 3],
        };
        assert_eq!(
            build_mysql_result_grid_copy_sql(&table("people"), &headers(true), &[insert])
                .expect("copy insert"),
            "INSERT INTO `app``db`.`people` (`display``name`, `score`) VALUES ('O''Reilly', 1.5);"
        );

        let update = MysqlResultGridCopyOperation {
            operation_type: MysqlResultGridCopyOperationType::UpdateCopy,
            data_list: row.clone(),
            select_cols: vec![2],
        };
        assert_eq!(
            build_mysql_result_grid_copy_sql(&table("people"), &headers(true), &[update])
                .expect("copy update"),
            "UPDATE `app``db`.`people` SET `display``name` = 'O''Reilly' WHERE `id` = 42;"
        );

        let where_operation = MysqlResultGridCopyOperation {
            operation_type: MysqlResultGridCopyOperationType::Where,
            data_list: row,
            select_cols: vec![2],
        };
        assert_eq!(
            build_mysql_result_grid_copy_sql(&table("people"), &headers(true), &[where_operation])
                .expect("copy where"),
            "WHERE `display``name` LIKE 'O''Reilly'"
        );
    }

    #[test]
    fn grid_copy_in_values_is_typed_distinct_and_single_column() {
        let first = MysqlResultGridCopyOperation {
            operation_type: MysqlResultGridCopyOperationType::Where,
            data_list: vec![None, Some("42".to_owned()), None, None, None],
            select_cols: vec![1],
        };
        let duplicate = first.clone();
        let null = MysqlResultGridCopyOperation {
            data_list: vec![None, None, None, None, None],
            ..first.clone()
        };
        assert_eq!(
            build_mysql_result_grid_in_values(
                &headers(true),
                &[first.clone(), duplicate, null.clone()]
            )
            .expect("typed IN values"),
            "(42, NULL)"
        );
        assert_eq!(
            build_mysql_external_in_values(&[
                " alpha ".to_owned(),
                "alpha".to_owned(),
                "O'Reilly".to_owned(),
            ])
            .expect("external IN values"),
            "('alpha', 'O''Reilly')"
        );

        let mixed = MysqlResultGridCopyOperation {
            select_cols: vec![2],
            ..first
        };
        assert!(build_mysql_result_grid_in_values(&headers(true), &[mixed, null]).is_err());
    }

    #[test]
    fn count_query_accepts_one_read_and_rejects_statement_injection() {
        assert_eq!(
            build_mysql_count_query("SELECT ';' AS marker FROM items;")
                .expect("single count source"),
            "SELECT COUNT(*) AS `CHAT2DB_COUNT` FROM (SELECT ';' AS marker FROM items) AS `CHAT2DB_COUNT_SOURCE`"
        );
        assert!(build_mysql_count_query("SELECT 1; DROP TABLE items").is_err());
        assert!(build_mysql_count_query("DELETE FROM items").is_err());
    }

    #[test]
    fn database_and_schema_are_mysql_namespace_aliases() {
        let definition = MysqlDatabaseDefinition {
            name: "tenant`one".to_owned(),
            if_not_exists: true,
            charset: Some("utf8mb4".to_owned()),
            collation: Some("utf8mb4_0900_ai_ci".to_owned()),
        };
        let expected = "CREATE DATABASE IF NOT EXISTS `tenant``one` DEFAULT CHARACTER SET = utf8mb4 COLLATE = utf8mb4_0900_ai_ci";
        assert_eq!(
            build_mysql_create_database(&definition).expect("database SQL"),
            expected
        );
        assert_eq!(
            build_mysql_create_schema(&definition).expect("schema SQL"),
            expected
        );
        assert_eq!(
            build_mysql_drop_schema("tenant`one", true).expect("drop schema SQL"),
            "DROP DATABASE IF EXISTS `tenant``one`"
        );

        let mut invalid = definition;
        invalid.charset = Some("utf8mb4; DROP DATABASE prod".to_owned());
        assert!(build_mysql_create_database(&invalid).is_err());
    }

    #[test]
    fn create_table_builds_structured_columns_indexes_and_options() {
        let mut id = column("id", "BIGINT");
        id.unsigned = true;
        id.auto_increment = true;

        let mut label = column("label", "VARCHAR");
        label.length = Some(255);
        label.nullable = true;
        label.default_value = Some("O'Reilly".to_owned());
        label.charset = Some("utf8mb4".to_owned());
        label.collation = Some("utf8mb4_0900_ai_ci".to_owned());
        label.comment = Some("display\\name".to_owned());

        let mut state = column("state", "ENUM");
        state.enum_values = vec!["new".to_owned(), "in'flight".to_owned()];
        state.default_value = Some("new".to_owned());

        let definition = MysqlTableDefinition {
            name: table("order`items"),
            if_not_exists: true,
            columns: vec![id, label, state],
            indexes: vec![
                index(MysqlIndexKind::Primary, None, &["id"]),
                index(MysqlIndexKind::Unique, Some("uq`label"), &["label"]),
            ],
            engine: Some("InnoDB".to_owned()),
            charset: Some("utf8mb4".to_owned()),
            collation: Some("utf8mb4_0900_ai_ci".to_owned()),
            comment: Some("orders' current".to_owned()),
            auto_increment: Some(100),
        };
        let sql = build_mysql_create_table(&definition).expect("create table SQL");
        assert!(sql.starts_with(
            "CREATE TABLE IF NOT EXISTS `app``db`.`order``items` (\n  `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT"
        ));
        assert!(sql.contains(
            "`label` VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci NULL DEFAULT 'O''Reilly' COMMENT 'display\\\\name'"
        ));
        assert!(sql.contains("`state` ENUM('new', 'in''flight') NOT NULL DEFAULT 'new'"));
        assert!(sql.contains("PRIMARY KEY (`id`)"));
        assert!(sql.contains("UNIQUE INDEX `uq``label` (`label`)"));
        assert!(sql.ends_with(
            "ENGINE = InnoDB DEFAULT CHARACTER SET = utf8mb4 COLLATE = utf8mb4_0900_ai_ci AUTO_INCREMENT = 100 COMMENT = 'orders'' current'"
        ));
    }

    #[test]
    fn create_table_rejects_raw_types_and_invalid_properties() {
        let mut raw = column("id", "INT); DROP TABLE users; --");
        let mut definition = MysqlTableDefinition {
            name: table("items"),
            if_not_exists: false,
            columns: vec![raw.clone()],
            indexes: Vec::new(),
            engine: None,
            charset: None,
            collation: None,
            comment: None,
            auto_increment: None,
        };
        assert!(build_mysql_create_table(&definition).is_err());

        raw.type_name = "VARCHAR".to_owned();
        definition.columns = vec![raw];
        assert!(build_mysql_create_table(&definition).is_err());

        let mut decimal = column("amount", "DECIMAL");
        decimal.length = Some(4);
        decimal.scale = Some(5);
        definition.columns = vec![decimal];
        assert!(build_mysql_create_table(&definition).is_err());
    }

    #[test]
    fn alter_table_supports_column_index_options_and_rename() {
        let mut added = column("created_at", "TIMESTAMP");
        added.nullable = false;
        added.default_value = Some("now()".to_owned());
        added.on_update_current_timestamp = true;

        let mut modified = column("title", "VARCHAR");
        modified.length = Some(300);
        modified.nullable = true;

        let alter = MysqlTableAlter {
            table: table("items"),
            rename_to: Some(table("renamed`items")),
            columns: vec![
                MysqlColumnAlter::Add {
                    column: added,
                    position: Some(MysqlColumnPosition::First),
                },
                MysqlColumnAlter::Modify {
                    old_name: "old_title".to_owned(),
                    column: modified,
                    position: Some(MysqlColumnPosition::After("created_at".to_owned())),
                },
                MysqlColumnAlter::Delete {
                    name: "obsolete".to_owned(),
                },
            ],
            indexes: vec![
                MysqlIndexAlter::Modify {
                    old_kind: MysqlIndexKind::Normal,
                    old_name: Some("idx_old".to_owned()),
                    index: index(MysqlIndexKind::Unique, Some("idx_new"), &["title"]),
                },
                MysqlIndexAlter::Delete {
                    kind: MysqlIndexKind::Primary,
                    name: None,
                },
            ],
            engine: Some("InnoDB".to_owned()),
            charset: Some("utf8mb4".to_owned()),
            collation: Some("utf8mb4_0900_ai_ci".to_owned()),
            comment: Some(String::new()),
            auto_increment: Some(10),
        };
        let sql = build_mysql_alter_table(&alter).expect("alter table SQL");
        assert!(sql.contains(
            "ADD COLUMN `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP FIRST"
        ));
        assert!(
            sql.contains("CHANGE COLUMN `old_title` `title` VARCHAR(300) NULL AFTER `created_at`")
        );
        assert!(sql.contains("DROP COLUMN `obsolete`"));
        assert!(sql.contains("DROP INDEX `idx_old`"));
        assert!(sql.contains("ADD UNIQUE INDEX `idx_new` (`title`)"));
        assert!(sql.contains("DROP PRIMARY KEY"));
        assert!(sql.contains("COMMENT = ''"));
        assert!(sql.ends_with("RENAME TO `app``db`.`renamed``items`"));
    }

    #[test]
    fn table_drop_truncate_and_copy_quote_names() {
        assert_eq!(
            build_mysql_drop_table(&table("items"), true).expect("drop table SQL"),
            "DROP TABLE IF EXISTS `app``db`.`items`"
        );
        assert_eq!(
            build_mysql_truncate_table(&table("items")).expect("truncate table SQL"),
            "TRUNCATE TABLE `app``db`.`items`"
        );
        assert_eq!(
            build_mysql_copy_table(&MysqlTableCopy {
                source: table("items"),
                target: table("items_copy"),
                if_not_exists: true,
                copy_data: true,
            })
            .expect("copy table SQL"),
            vec![
                "CREATE TABLE IF NOT EXISTS `app``db`.`items_copy` LIKE `app``db`.`items`",
                "INSERT INTO `app``db`.`items_copy` SELECT * FROM `app``db`.`items`",
            ]
        );
    }

    #[test]
    fn view_builder_preserves_body_and_uses_closed_options() {
        let view = MysqlViewDefinition {
            name: table("active`users"),
            columns: vec!["user`id".to_owned(), "name".to_owned()],
            use_or_replace: false,
            algorithm: Some(MysqlViewAlgorithm::Merge),
            definer: Some(MysqlViewDefiner {
                user: "app'user".to_owned(),
                host: "localhost".to_owned(),
            }),
            sql_security: Some(MysqlViewSecurity::Invoker),
            check_option: Some(MysqlViewCheckOption::Cascaded),
            body: "SELECT id, name\nFROM users WHERE active = 1".to_owned(),
        };
        assert_eq!(
            build_mysql_create_or_replace_view(&view).expect("view SQL"),
            "CREATE OR REPLACE ALGORITHM = MERGE DEFINER = 'app''user'@'localhost' SQL SECURITY INVOKER VIEW `app``db`.`active``users` (`user``id`, `name`) AS SELECT id, name\nFROM users WHERE active = 1 WITH CASCADED CHECK OPTION"
        );
        assert_eq!(
            build_mysql_create_view(&view).expect("plain view SQL"),
            "CREATE ALGORITHM = MERGE DEFINER = 'app''user'@'localhost' SQL SECURITY INVOKER VIEW `app``db`.`active``users` (`user``id`, `name`) AS SELECT id, name\nFROM users WHERE active = 1 WITH CASCADED CHECK OPTION"
        );
        assert_eq!(
            build_mysql_drop_view(&view.name, true).expect("drop view SQL"),
            "DROP VIEW IF EXISTS `app``db`.`active``users`"
        );

        let empty = MysqlViewDefinition {
            body: "   ".to_owned(),
            ..view.clone()
        };
        assert!(build_mysql_create_or_replace_view(&empty).is_err());

        let oversized = MysqlViewDefinition {
            body: "x".repeat(MAX_VIEW_BODY_BYTES + 1),
            ..view
        };
        assert!(build_mysql_create_or_replace_view(&oversized).is_err());

        let injected = MysqlViewDefinition {
            body: "SELECT 1; DROP TABLE users".to_owned(),
            ..oversized
        };
        assert!(build_mysql_create_or_replace_view(&injected).is_err());
    }

    #[test]
    fn table_editor_meta_covers_common_mysql_84_options() {
        let meta = mysql_table_editor_meta();
        for type_name in [
            "BIGINT UNSIGNED",
            "DECIMAL",
            "DATETIME",
            "VARCHAR",
            "JSON",
            "GEOMETRY",
        ] {
            assert!(
                meta.column_types
                    .iter()
                    .any(|column_type| column_type.type_name == type_name)
            );
        }
        assert!(
            meta.charsets
                .iter()
                .any(|charset| charset.charset_name == "utf8mb4")
        );
        assert!(
            meta.collations
                .iter()
                .any(|collation| collation.collation_name == "utf8mb4_0900_ai_ci")
        );
        assert_eq!(
            meta.index_types
                .iter()
                .map(|index_type| index_type.type_name.as_str())
                .collect::<Vec<_>>(),
            ["Primary", "Normal", "Unique", "Fulltext", "Spatial"]
        );
        assert!(
            meta.engine_types
                .iter()
                .any(|engine| engine.name == "InnoDB")
        );
        let json = serde_json::to_value(meta).expect("serialize table editor meta");
        assert!(json["columnTypes"].is_array());
        assert!(json["defaultValues"].is_array());
        assert!(json["engineTypes"].is_array());
    }
}
