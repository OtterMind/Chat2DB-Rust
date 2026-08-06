use chat2db_contract::{QueryLimits, StartQueryRequest};
use chat2db_java_bridge::{JdbcColumn, JdbcParameter, JdbcRow, JdbcValue};

use crate::{
    AppError, AppErrorKind, Application,
    datasource_session::{JdbcQueryLimits, JdbcQueryResult, jdbc_query},
    native_driver_types::{
        ColumnList, ColumnMetadata, DatabaseList, DatabaseMetadata, NativeDriverDescriptor,
        SchemaList, SchemaMetadata, TableList, TableMetadata, TablePreviewAccepted,
        TablePreviewRequest,
    },
};

const DM_DATABASE_TYPE: &str = "DM";
const MAX_DM_IDENTIFIER_BYTES: usize = 128;
const MAX_DM_TABLE_PREVIEW_ROWS: u32 = 1_000;
const DATABASE_QUERY_LIMITS: JdbcQueryLimits = JdbcQueryLimits {
    max_rows: 8,
    max_result_bytes: 64 * 1024,
};
const SCHEMA_QUERY_LIMITS: JdbcQueryLimits = JdbcQueryLimits {
    max_rows: 4_096,
    max_result_bytes: 4 * 1024 * 1024,
};
const TABLE_QUERY_LIMITS: JdbcQueryLimits = JdbcQueryLimits {
    max_rows: 20_000,
    max_result_bytes: 16 * 1024 * 1024,
};
const COLUMN_QUERY_LIMITS: JdbcQueryLimits = JdbcQueryLimits {
    max_rows: 8_192,
    max_result_bytes: 8 * 1024 * 1024,
};

const DM_SYSTEM_SCHEMAS: &[&str] = &["CTISYS", "SYS", "SYSDBA", "SYSSSO", "SYSAUDITOR"];

const LIST_DATABASES_SQL: &str = "SELECT NAME AS DATABASE_NAME FROM V$DATABASE";
const LIST_SCHEMAS_SQL: &str = "SELECT USERNAME AS SCHEMA_NAME FROM ALL_USERS ORDER BY USERNAME";

const LIST_TABLES_SQL: &str = "SELECT T.OWNER AS SCHEMA_NAME, \
    T.TABLE_NAME AS TABLE_NAME, 'TABLE' AS TABLE_TYPE, \
    C.COMMENTS AS TABLE_COMMENT, T.TABLESPACE_NAME AS TABLESPACE_NAME, \
    T.NUM_ROWS AS ROW_COUNT \
    FROM ALL_TABLES T \
    LEFT JOIN ALL_TAB_COMMENTS C \
      ON C.OWNER = T.OWNER AND C.TABLE_NAME = T.TABLE_NAME \
    WHERE T.OWNER = ? \
    ORDER BY T.TABLE_NAME";

const LIST_TABLES_WITH_PATTERN_SQL: &str = "SELECT T.OWNER AS SCHEMA_NAME, \
    T.TABLE_NAME AS TABLE_NAME, 'TABLE' AS TABLE_TYPE, \
    C.COMMENTS AS TABLE_COMMENT, T.TABLESPACE_NAME AS TABLESPACE_NAME, \
    T.NUM_ROWS AS ROW_COUNT \
    FROM ALL_TABLES T \
    LEFT JOIN ALL_TAB_COMMENTS C \
      ON C.OWNER = T.OWNER AND C.TABLE_NAME = T.TABLE_NAME \
    WHERE T.OWNER = ? AND T.TABLE_NAME LIKE ? \
    ORDER BY T.TABLE_NAME";

const LIST_COLUMNS_SQL: &str = "SELECT C.COLUMN_NAME AS COLUMN_NAME, \
    C.DATA_TYPE AS DATA_TYPE, C.DATA_DEFAULT AS DATA_DEFAULT, \
    CC.COMMENTS AS COLUMN_COMMENT, C.NULLABLE AS IS_NULLABLE, \
    C.COLUMN_ID AS ORDINAL_POSITION, C.DATA_LENGTH AS DATA_LENGTH, \
    C.DATA_PRECISION AS DATA_PRECISION, C.DATA_SCALE AS DATA_SCALE, \
    PK.CONSTRAINT_NAME AS PRIMARY_KEY_NAME, PK.POSITION AS PRIMARY_KEY_ORDER \
    FROM ALL_TAB_COLUMNS C \
    LEFT JOIN ALL_COL_COMMENTS CC \
      ON CC.OWNER = C.OWNER AND CC.TABLE_NAME = C.TABLE_NAME \
     AND CC.COLUMN_NAME = C.COLUMN_NAME \
    LEFT JOIN ( \
      SELECT AC.OWNER, ACC.TABLE_NAME, ACC.COLUMN_NAME, \
             AC.CONSTRAINT_NAME, ACC.POSITION \
      FROM ALL_CONSTRAINTS AC \
      JOIN ALL_CONS_COLUMNS ACC \
        ON ACC.OWNER = AC.OWNER AND ACC.CONSTRAINT_NAME = AC.CONSTRAINT_NAME \
       AND ACC.TABLE_NAME = AC.TABLE_NAME \
      WHERE AC.CONSTRAINT_TYPE = 'P' \
    ) PK \
      ON PK.OWNER = C.OWNER AND PK.TABLE_NAME = C.TABLE_NAME \
     AND PK.COLUMN_NAME = C.COLUMN_NAME \
    WHERE C.OWNER = ? AND C.TABLE_NAME = ? \
    ORDER BY C.COLUMN_ID";

pub(crate) const DM_DRIVER_DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
    id: "dm",
    implementation: "dm-jdbc",
    database_types: &[DM_DATABASE_TYPE],
    compatibility_aliases: &["dm", "dm-jdbc", "dm.jdbc.driver.DmDriver"],
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DmMetadataQuery {
    pub(crate) sql: String,
    pub(crate) parameters: Vec<JdbcParameter>,
}

pub(crate) fn list_schemas_query() -> DmMetadataQuery {
    DmMetadataQuery {
        sql: LIST_SCHEMAS_SQL.to_owned(),
        parameters: Vec::new(),
    }
}

pub(crate) fn list_databases_query() -> DmMetadataQuery {
    DmMetadataQuery {
        sql: LIST_DATABASES_SQL.to_owned(),
        parameters: Vec::new(),
    }
}

pub(crate) fn list_tables_query(
    schema_name: &str,
    table_name_pattern: &str,
) -> Result<DmMetadataQuery, AppError> {
    validate_metadata_name(schema_name, "schemaName")?;
    let mut parameters = vec![text_parameter(1, schema_name)];
    let sql = if table_name_pattern.is_empty() {
        LIST_TABLES_SQL
    } else {
        if table_name_pattern.len() > MAX_DM_IDENTIFIER_BYTES * 4
            || table_name_pattern.chars().any(char::is_control)
        {
            return Err(invalid_metadata_request("tableNamePattern"));
        }
        // Preserve JDBC/SQL LIKE semantics: `%`, `_`, and letter case are passed through.
        parameters.push(text_parameter(2, table_name_pattern));
        LIST_TABLES_WITH_PATTERN_SQL
    };
    Ok(DmMetadataQuery {
        sql: sql.to_owned(),
        parameters,
    })
}

pub(crate) fn list_columns_query(
    schema_name: &str,
    table_name: &str,
) -> Result<DmMetadataQuery, AppError> {
    validate_metadata_name(schema_name, "schemaName")?;
    validate_metadata_name(table_name, "tableName")?;
    Ok(DmMetadataQuery {
        sql: LIST_COLUMNS_SQL.to_owned(),
        parameters: vec![
            text_parameter(1, schema_name),
            text_parameter(2, table_name),
        ],
    })
}

pub(crate) async fn list_schemas(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) -> Result<SchemaList, AppError> {
    let query = list_schemas_query();
    let result = jdbc_query(
        application,
        &DM_DRIVER_DESCRIPTOR,
        datasource_id,
        query.sql,
        query.parameters,
        SCHEMA_QUERY_LIMITS,
    )
    .await?;
    ensure_complete_metadata(&result, "schemas")?;
    map_schemas(database_name, &result.columns, &result.rows)
}

pub(crate) async fn list_databases(
    application: &Application,
    datasource_id: &str,
) -> Result<DatabaseList, AppError> {
    let query = list_databases_query();
    let result = jdbc_query(
        application,
        &DM_DRIVER_DESCRIPTOR,
        datasource_id,
        query.sql,
        query.parameters,
        DATABASE_QUERY_LIMITS,
    )
    .await?;
    ensure_complete_metadata(&result, "databases")?;
    map_databases(&result.columns, &result.rows)
}

pub(crate) async fn list_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name_pattern: &str,
) -> Result<TableList, AppError> {
    let query = list_tables_query(schema_name, table_name_pattern)?;
    let result = jdbc_query(
        application,
        &DM_DRIVER_DESCRIPTOR,
        datasource_id,
        query.sql,
        query.parameters,
        TABLE_QUERY_LIMITS,
    )
    .await?;
    ensure_complete_metadata(&result, "tables")?;
    map_tables(database_name, schema_name, &result.columns, &result.rows)
}

pub(crate) async fn list_columns(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<ColumnList, AppError> {
    let query = list_columns_query(schema_name, table_name)?;
    let result = jdbc_query(
        application,
        &DM_DRIVER_DESCRIPTOR,
        datasource_id,
        query.sql,
        query.parameters,
        COLUMN_QUERY_LIMITS,
    )
    .await?;
    ensure_complete_metadata(&result, "columns")?;
    map_columns(
        database_name,
        schema_name,
        table_name,
        &result.columns,
        &result.rows,
    )
}

pub(crate) fn map_schemas(
    database_name: &str,
    columns: &[JdbcColumn],
    rows: &[JdbcRow],
) -> Result<SchemaList, AppError> {
    let schema_name = required_column(columns, "SCHEMA_NAME")?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let name = required_text(row, schema_name)?;
        items.push(SchemaMetadata {
            database_name: database_name.to_owned(),
            owner: name.clone(),
            system: DM_SYSTEM_SCHEMAS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&name)),
            name,
            ..SchemaMetadata::default()
        });
    }
    Ok(SchemaList { items })
}

pub(crate) fn map_databases(
    columns: &[JdbcColumn],
    rows: &[JdbcRow],
) -> Result<DatabaseList, AppError> {
    let database_name = required_column(columns, "DATABASE_NAME")?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(DatabaseMetadata {
            name: required_text(row, database_name)?,
            ..DatabaseMetadata::default()
        });
    }
    Ok(DatabaseList { items })
}

pub(crate) fn map_tables(
    database_name: &str,
    requested_schema_name: &str,
    columns: &[JdbcColumn],
    rows: &[JdbcRow],
) -> Result<TableList, AppError> {
    let schema_name = required_column(columns, "SCHEMA_NAME")?;
    let table_name = required_column(columns, "TABLE_NAME")?;
    let table_type = required_column(columns, "TABLE_TYPE")?;
    let comment = required_column(columns, "TABLE_COMMENT")?;
    let tablespace = required_column(columns, "TABLESPACE_NAME")?;
    let row_count = required_column(columns, "ROW_COUNT")?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let returned_schema_name = optional_text(row, schema_name)?.unwrap_or_default();
        items.push(TableMetadata {
            database_name: database_name.to_owned(),
            schema_name: if returned_schema_name.is_empty() {
                requested_schema_name.to_owned()
            } else {
                returned_schema_name
            },
            name: required_text(row, table_name)?,
            table_type: required_text(row, table_type)?,
            comment: optional_text(row, comment)?.unwrap_or_default(),
            database_type: DM_DATABASE_TYPE.to_owned(),
            tablespace: optional_text(row, tablespace)?.unwrap_or_default(),
            rows: optional_text(row, row_count)?,
            ..TableMetadata::default()
        });
    }
    Ok(TableList { items })
}

pub(crate) fn map_columns(
    database_name: &str,
    schema_name: &str,
    table_name: &str,
    columns: &[JdbcColumn],
    rows: &[JdbcRow],
) -> Result<ColumnList, AppError> {
    let column_name = required_column(columns, "COLUMN_NAME")?;
    let data_type = required_column(columns, "DATA_TYPE")?;
    let default_value = required_column(columns, "DATA_DEFAULT")?;
    let comment = required_column(columns, "COLUMN_COMMENT")?;
    let nullable = required_column(columns, "IS_NULLABLE")?;
    let ordinal_position = required_column(columns, "ORDINAL_POSITION")?;
    let data_length = required_column(columns, "DATA_LENGTH")?;
    let data_precision = required_column(columns, "DATA_PRECISION")?;
    let data_scale = required_column(columns, "DATA_SCALE")?;
    let primary_key_name = required_column(columns, "PRIMARY_KEY_NAME")?;
    let primary_key_order = required_column(columns, "PRIMARY_KEY_ORDER")?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let column_type = normalize_dm_column_type(&required_text(row, data_type)?);
        let precision = optional_i32(row, data_precision)?;
        let length = optional_i32(row, data_length)?;
        let decimal_digits = optional_i32(row, data_scale)?;
        let primary_key_name = optional_text(row, primary_key_name)?.unwrap_or_default();
        let column_size = if column_type.eq_ignore_ascii_case("TIMESTAMP") {
            decimal_digits
        } else {
            precision.or(length)
        };
        items.push(ColumnMetadata {
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: table_name.to_owned(),
            name: required_text(row, column_name)?,
            data_type: Some(dm_jdbc_type(&column_type)),
            column_type,
            default_value: optional_text(row, default_value)?,
            comment: optional_text(row, comment)?.unwrap_or_default(),
            primary_key: Some(!primary_key_name.is_empty()),
            primary_key_name,
            primary_key_order: optional_i32(row, primary_key_order)?.unwrap_or_default(),
            column_size,
            decimal_digits,
            ordinal_position: optional_i32(row, ordinal_position)?,
            nullable: optional_text(row, nullable)?.map_or(Some(2), |value| {
                match value.to_ascii_uppercase().as_str() {
                    "N" | "NO" => Some(0),
                    "Y" | "YES" => Some(1),
                    _ => Some(2),
                }
            }),
            ..ColumnMetadata::default()
        });
    }
    Ok(ColumnList { items })
}

pub(crate) async fn start_table_preview(
    application: &Application,
    request: TablePreviewRequest,
    row_limit: u32,
) -> Result<TablePreviewAccepted, AppError> {
    let sql = table_preview_sql(
        &request.table.scope.schema_name,
        &request.table.table_name,
        row_limit,
    )?;
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

pub(crate) fn table_preview_sql(
    schema_name: &str,
    table_name: &str,
    row_limit: u32,
) -> Result<String, AppError> {
    if row_limit == 0 || row_limit > MAX_DM_TABLE_PREVIEW_ROWS {
        return Err(AppError::invalid(
            "invalid_dm_table_preview_request",
            format!("rowLimit must be between 1 and {MAX_DM_TABLE_PREVIEW_ROWS}"),
        ));
    }
    Ok(format!(
        "SELECT * FROM {}.{} LIMIT {row_limit}",
        quote_identifier(schema_name, "schemaName")?,
        quote_identifier(table_name, "tableName")?
    ))
}

pub(crate) fn quote_identifier(value: &str, field: &str) -> Result<String, AppError> {
    validate_metadata_name(value, field)?;
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

fn text_parameter(position: u32, value: &str) -> JdbcParameter {
    JdbcParameter {
        position,
        value: JdbcValue::Text(value.to_owned()),
        jdbc_type: Some(12),
        jdbc_type_name: None,
    }
}

fn validate_metadata_name(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty()
        || value.len() > MAX_DM_IDENTIFIER_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(invalid_metadata_request(field));
    }
    Ok(())
}

fn invalid_metadata_request(field: &str) -> AppError {
    AppError::invalid("invalid_dm_metadata_request", format!("{field} is invalid"))
}

fn ensure_complete_metadata(result: &JdbcQueryResult, kind: &str) -> Result<(), AppError> {
    if result.completed.truncated_by_max_rows || result.completed.truncated_by_max_result_bytes {
        return Err(AppError::new(
            AppErrorKind::ResourceExhausted,
            chat2db_contract::ApiError::new(
                "dm_metadata_limit_exceeded",
                format!("DM {kind} metadata exceeded the bounded JDBC result limit"),
            ),
        ));
    }
    Ok(())
}

fn required_column(columns: &[JdbcColumn], label: &str) -> Result<usize, AppError> {
    columns
        .iter()
        .position(|column| {
            column.label.eq_ignore_ascii_case(label) || column.name.eq_ignore_ascii_case(label)
        })
        .ok_or_else(AppError::internal)
}

fn row_value(row: &JdbcRow, index: usize) -> Result<&JdbcValue, AppError> {
    row.values.get(index).ok_or_else(AppError::internal)
}

fn required_text(row: &JdbcRow, index: usize) -> Result<String, AppError> {
    optional_text(row, index)?.ok_or_else(AppError::internal)
}

fn optional_text(row: &JdbcRow, index: usize) -> Result<Option<String>, AppError> {
    let value = match row_value(row, index)? {
        JdbcValue::Null => return Ok(None),
        JdbcValue::Boolean(value) => value.to_string(),
        JdbcValue::SignedInteger(value) => value.to_string(),
        JdbcValue::UnsignedInteger(value) => value.to_string(),
        JdbcValue::Float32(value) => value.to_string(),
        JdbcValue::Float64(value) => value.to_string(),
        JdbcValue::Decimal(value)
        | JdbcValue::Text(value)
        | JdbcValue::Date(value)
        | JdbcValue::Time(value)
        | JdbcValue::Timestamp(value)
        | JdbcValue::TimestampWithTimeZone(value)
        | JdbcValue::Json(value)
        | JdbcValue::Uuid(value) => value.clone(),
        JdbcValue::Opaque { display_value, .. } => display_value.clone(),
        JdbcValue::Binary(_) => return Err(AppError::internal()),
    };
    Ok(Some(value))
}

fn optional_i32(row: &JdbcRow, index: usize) -> Result<Option<i32>, AppError> {
    let value = match row_value(row, index)? {
        JdbcValue::Null => return Ok(None),
        JdbcValue::SignedInteger(value) => i32::try_from(*value).ok(),
        JdbcValue::UnsignedInteger(value) => i32::try_from(*value).ok(),
        JdbcValue::Decimal(value) | JdbcValue::Text(value) => value.parse().ok(),
        _ => None,
    };
    value.map(Some).ok_or_else(AppError::internal)
}

fn normalize_dm_column_type(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut remaining = value.trim();
    while let Some(open) = remaining.find('(') {
        normalized.push_str(&remaining[..open]);
        let after_open = &remaining[open + 1..];
        let Some(close) = after_open.find(')') else {
            normalized.push('(');
            normalized.push_str(after_open);
            return normalized;
        };
        let contents = &after_open[..close];
        if contents.is_empty() || !contents.bytes().all(|byte| byte.is_ascii_digit()) {
            normalized.push('(');
            normalized.push_str(contents);
            normalized.push(')');
        }
        remaining = &after_open[close + 1..];
    }
    normalized.push_str(remaining);
    normalized
}

fn dm_jdbc_type(data_type: &str) -> i32 {
    match data_type.trim().to_ascii_uppercase().as_str() {
        "CHAR" => 1,
        "VARCHAR" | "VARCHAR2" => 12,
        "NCHAR" => -15,
        "NVARCHAR" | "NVARCHAR2" => -9,
        "BIT" => -7,
        "TINYINT" => -6,
        "SMALLINT" => 5,
        "INTEGER" | "INT" => 4,
        "BIGINT" => -5,
        "NUMERIC" => 2,
        "DECIMAL" | "NUMBER" => 3,
        "REAL" => 7,
        "FLOAT" => 6,
        "DOUBLE" | "DOUBLE PRECISION" => 8,
        "BINARY" => -2,
        "VARBINARY" => -3,
        "LONGVARBINARY" | "IMAGE" => -4,
        "DATE" => 91,
        "TIME" => 92,
        "TIMESTAMP" | "TIMESTAMP WITH TIME ZONE" | "TIMESTAMP WITH LOCAL TIME ZONE" => 93,
        "BOOLEAN" => 16,
        "BLOB" => 2004,
        "CLOB" | "TEXT" => 2005,
        "ARRAY" => 2003,
        "ROWID" => -8,
        "SQLXML" => 2009,
        _ => 1111,
    }
}

#[cfg(test)]
mod tests {
    use chat2db_java_bridge::{ColumnNullability, JdbcColumn, JdbcRow, JdbcValue, JdbcValueType};

    use super::{
        list_columns_query, list_databases_query, list_tables_query, map_columns, map_databases,
        map_schemas, map_tables, quote_identifier, table_preview_sql,
    };

    #[test]
    fn dm_identifiers_use_double_quotes_and_escape_embedded_quotes() {
        assert_eq!(
            quote_identifier("sales\"2026", "schemaName").expect("identifier must quote"),
            "\"sales\"\"2026\""
        );
        assert!(quote_identifier("", "schemaName").is_err());
        assert!(quote_identifier("bad\nname", "tableName").is_err());
        assert!(quote_identifier(&"x".repeat(129), "tableName").is_err());
    }

    #[test]
    fn dm_table_preview_is_schema_qualified_and_bounded() {
        assert_eq!(
            table_preview_sql("APP", "ORDER", 200).expect("preview SQL must build"),
            "SELECT * FROM \"APP\".\"ORDER\" LIMIT 200"
        );
        assert!(table_preview_sql("APP", "ORDER", 0).is_err());
        assert!(table_preview_sql("APP", "ORDER", 1_001).is_err());
    }

    #[test]
    fn dm_catalog_queries_bind_request_values() {
        let database_query = list_databases_query();
        assert_eq!(
            database_query.sql,
            "SELECT NAME AS DATABASE_NAME FROM V$DATABASE"
        );
        assert!(database_query.parameters.is_empty());

        let table_query = list_tables_query("APP' OR 1=1 --", "Order_%")
            .expect("quoted schema names are valid metadata values");
        assert!(!table_query.sql.contains("APP' OR 1=1 --"));
        assert_eq!(table_query.parameters.len(), 2);
        assert!(matches!(
            &table_query.parameters[0].value,
            JdbcValue::Text(value) if value == "APP' OR 1=1 --"
        ));
        assert!(matches!(
            &table_query.parameters[1].value,
            JdbcValue::Text(value) if value == "Order_%"
        ));

        let column_query = list_columns_query("APP", "ORDER").expect("column query must build");
        assert_eq!(column_query.parameters.len(), 2);
        assert!(column_query.sql.contains("ALL_TAB_COLUMNS"));
        assert!(column_query.sql.contains("ALL_CONSTRAINTS"));
    }

    #[test]
    fn dm_database_mapping_uses_the_live_catalog_name() {
        let columns = vec![column("DATABASE_NAME")];
        let rows = vec![row(vec![JdbcValue::Text("DAMENG".to_owned())])];
        let mapped = map_databases(&columns, &rows).expect("database must map");
        assert_eq!(mapped.items.len(), 1);
        assert_eq!(mapped.items[0].name, "DAMENG");
    }

    #[test]
    fn dm_schema_mapping_marks_known_system_schemas() {
        let columns = vec![column("SCHEMA_NAME")];
        let rows = vec![
            row(vec![JdbcValue::Text("SYS".to_owned())]),
            row(vec![JdbcValue::Text("APP".to_owned())]),
        ];
        let mapped = map_schemas("DMDB", &columns, &rows).expect("schemas must map");
        assert_eq!(mapped.items.len(), 2);
        assert!(mapped.items[0].system);
        assert!(!mapped.items[1].system);
        assert_eq!(mapped.items[1].database_name, "DMDB");
        assert_eq!(mapped.items[1].owner, "APP");
    }

    #[test]
    fn dm_table_mapping_preserves_comment_tablespace_and_estimated_rows() {
        let columns = columns(&[
            "SCHEMA_NAME",
            "TABLE_NAME",
            "TABLE_TYPE",
            "TABLE_COMMENT",
            "TABLESPACE_NAME",
            "ROW_COUNT",
        ]);
        let rows = vec![row(vec![
            JdbcValue::Text("APP".to_owned()),
            JdbcValue::Text("ORDERS".to_owned()),
            JdbcValue::Text("TABLE".to_owned()),
            JdbcValue::Text("sales orders".to_owned()),
            JdbcValue::Text("MAIN".to_owned()),
            JdbcValue::Decimal("42".to_owned()),
        ])];
        let mapped = map_tables("DMDB", "APP", &columns, &rows).expect("tables must map");
        let table = &mapped.items[0];
        assert_eq!(table.name, "ORDERS");
        assert_eq!(table.schema_name, "APP");
        assert_eq!(table.comment, "sales orders");
        assert_eq!(table.tablespace, "MAIN");
        assert_eq!(table.rows.as_deref(), Some("42"));
        assert_eq!(table.database_type, "DM");
    }

    #[test]
    fn dm_column_mapping_preserves_type_nullability_and_primary_key_order() {
        let columns = columns(&[
            "COLUMN_NAME",
            "DATA_TYPE",
            "DATA_DEFAULT",
            "COLUMN_COMMENT",
            "IS_NULLABLE",
            "ORDINAL_POSITION",
            "DATA_LENGTH",
            "DATA_PRECISION",
            "DATA_SCALE",
            "PRIMARY_KEY_NAME",
            "PRIMARY_KEY_ORDER",
        ]);
        let rows = vec![row(vec![
            JdbcValue::Text("ID".to_owned()),
            JdbcValue::Text("BIGINT".to_owned()),
            JdbcValue::Null,
            JdbcValue::Text("identifier".to_owned()),
            JdbcValue::Text("N".to_owned()),
            JdbcValue::SignedInteger(1),
            JdbcValue::SignedInteger(8),
            JdbcValue::SignedInteger(19),
            JdbcValue::SignedInteger(0),
            JdbcValue::Text("PK_ORDERS".to_owned()),
            JdbcValue::SignedInteger(1),
        ])];
        let mapped =
            map_columns("DMDB", "APP", "ORDERS", &columns, &rows).expect("columns must map");
        let column = &mapped.items[0];
        assert_eq!(column.name, "ID");
        assert_eq!(column.data_type, Some(-5));
        assert_eq!(column.column_size, Some(19));
        assert_eq!(column.nullable, Some(0));
        assert_eq!(column.primary_key, Some(true));
        assert_eq!(column.primary_key_name, "PK_ORDERS");
        assert_eq!(column.primary_key_order, 1);
    }

    fn columns(labels: &[&str]) -> Vec<JdbcColumn> {
        labels.iter().map(|label| column(label)).collect()
    }

    fn column(label: &str) -> JdbcColumn {
        JdbcColumn {
            ordinal: 1,
            label: label.to_owned(),
            name: label.to_owned(),
            jdbc_type: 12,
            jdbc_type_name: "VARCHAR".to_owned(),
            value_type: JdbcValueType::Text,
            nullability: ColumnNullability::Unknown,
            precision: None,
            scale: None,
            display_size: None,
            signed: None,
            catalog_name: None,
            schema_name: None,
            table_name: None,
        }
    }

    fn row(values: Vec<JdbcValue>) -> JdbcRow {
        JdbcRow { values }
    }
}
