// The mappers are consumed by the JDBC driver integration in the next change;
// keeping them warning-free makes that diff a pure wiring change.
#![allow(dead_code)]

//! Maps raw JDBC \`DatabaseMetaData\` rows into product metadata.
//!
//! The Java bridge only forwards the driver's own rows and labels. Every
//! dialect decision - catalog versus schema, system objects, case folding,
//! comment enrichment - belongs to this module, which is why the bridge never
//! needs to know about a specific database.

use chat2db_java_bridge::{JdbcColumn, JdbcRow, JdbcValue, MetadataResult};

use crate::native_driver_types::{
    ColumnList, ColumnMetadata, SchemaList, SchemaMetadata, TableList, TableMetadata,
};

/// Column labels defined by the JDBC specification for each metadata call.
mod labels {
    pub(super) const CATALOG: &str = "TABLE_CAT";
    pub(super) const SCHEMA: &str = "TABLE_SCHEM";
    pub(super) const TABLE: &str = "TABLE_NAME";
    pub(super) const TABLE_TYPE: &str = "TABLE_TYPE";
    pub(super) const REMARKS: &str = "REMARKS";
    pub(super) const COLUMN: &str = "COLUMN_NAME";
    pub(super) const DATA_TYPE: &str = "DATA_TYPE";
    pub(super) const TYPE_NAME: &str = "TYPE_NAME";
    pub(super) const COLUMN_SIZE: &str = "COLUMN_SIZE";
    pub(super) const DECIMAL_DIGITS: &str = "DECIMAL_DIGITS";
    pub(super) const ORDINAL: &str = "ORDINAL_POSITION";
    pub(super) const NULLABLE: &str = "NULLABLE";
    pub(super) const COLUMN_DEF: &str = "COLUMN_DEF";
    pub(super) const AUTO_INCREMENT: &str = "IS_AUTOINCREMENT";
}

/// One raw metadata result, addressing cells by their JDBC column label.
pub(crate) struct MetadataRows<'a> {
    result: &'a MetadataResult,
}

impl<'a> MetadataRows<'a> {
    pub(crate) const fn new(result: &'a MetadataResult) -> Self {
        Self { result }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.result.rows.is_empty()
    }

    fn index(&self, label: &str) -> Option<usize> {
        self.result
            .columns
            .iter()
            .position(|column| column.label.eq_ignore_ascii_case(label))
    }

    fn cell<'b>(&self, row: &'b JdbcRow, label: &str) -> Option<&'b JdbcValue> {
        row.values.get(self.index(label)?)
    }

    fn text(&self, row: &JdbcRow, label: &str) -> Option<String> {
        self.cell(row, label).and_then(text_value)
    }

    fn integer(&self, row: &JdbcRow, label: &str) -> Option<i64> {
        self.cell(row, label).and_then(integer_value)
    }

    fn yes(&self, row: &JdbcRow, label: &str) -> Option<bool> {
        self.text(row, label)
            .map(|value| value.eq_ignore_ascii_case("YES") || value == "1")
    }
}

fn text_value(value: &JdbcValue) -> Option<String> {
    match value {
        JdbcValue::Null | JdbcValue::Binary(_) => None,
        JdbcValue::Text(value)
        | JdbcValue::Decimal(value)
        | JdbcValue::Date(value)
        | JdbcValue::Time(value)
        | JdbcValue::Timestamp(value)
        | JdbcValue::TimestampWithTimeZone(value)
        | JdbcValue::Json(value)
        | JdbcValue::Uuid(value) => Some(value.clone()),
        JdbcValue::Boolean(value) => Some(value.to_string()),
        JdbcValue::SignedInteger(value) => Some(value.to_string()),
        JdbcValue::UnsignedInteger(value) => Some(value.to_string()),
        JdbcValue::Float32(value) => Some(value.to_string()),
        JdbcValue::Float64(value) => Some(value.to_string()),
        JdbcValue::Opaque { display_value, .. } => Some(display_value.clone()),
    }
}

fn integer_value(value: &JdbcValue) -> Option<i64> {
    match value {
        JdbcValue::SignedInteger(value) => Some(*value),
        JdbcValue::UnsignedInteger(value) => i64::try_from(*value).ok(),
        JdbcValue::Text(value) | JdbcValue::Decimal(value) => value.trim().parse().ok(),
        JdbcValue::Float64(value) => Some(*value as i64),
        JdbcValue::Boolean(value) => Some(i64::from(*value)),
        _ => None,
    }
}

/// Maps \`getSchemas\` rows into schema metadata.
pub(crate) fn schemas(result: &MetadataResult, database_name: &str) -> SchemaList {
    let rows = MetadataRows::new(result);
    let items = result
        .rows
        .iter()
        .filter_map(|row| {
            let name = rows.text(row, labels::SCHEMA)?;
            if name.trim().is_empty() {
                return None;
            }
            Some(SchemaMetadata {
                database_name: rows
                    .text(row, labels::CATALOG)
                    .filter(|catalog| !catalog.is_empty())
                    .unwrap_or_else(|| database_name.to_owned()),
                system: is_system_schema(&name),
                name,
                ..SchemaMetadata::default()
            })
        })
        .collect();
    SchemaList { items }
}

/// Maps \`getTables\` rows into table metadata.
pub(crate) fn tables(
    result: &MetadataResult,
    database_name: &str,
    schema_name: &str,
    database_type: &str,
) -> TableList {
    let rows = MetadataRows::new(result);
    let items = result
        .rows
        .iter()
        .filter_map(|row| {
            let name = rows.text(row, labels::TABLE)?;
            if name.trim().is_empty() {
                return None;
            }
            Some(TableMetadata {
                database_name: rows
                    .text(row, labels::CATALOG)
                    .filter(|catalog| !catalog.is_empty())
                    .unwrap_or_else(|| database_name.to_owned()),
                schema_name: rows
                    .text(row, labels::SCHEMA)
                    .filter(|schema| !schema.is_empty())
                    .unwrap_or_else(|| schema_name.to_owned()),
                name,
                table_type: rows.text(row, labels::TABLE_TYPE).unwrap_or_default(),
                comment: rows.text(row, labels::REMARKS).unwrap_or_default(),
                database_type: database_type.to_owned(),
                ..TableMetadata::default()
            })
        })
        .collect();
    TableList { items }
}

/// Maps \`getColumns\` rows into column metadata.
pub(crate) fn columns(
    result: &MetadataResult,
    database_name: &str,
    schema_name: &str,
) -> ColumnList {
    let rows = MetadataRows::new(result);
    let items = result
        .rows
        .iter()
        .filter_map(|row| {
            let name = rows.text(row, labels::COLUMN)?;
            if name.trim().is_empty() {
                return None;
            }
            let nullable = rows.integer(row, labels::NULLABLE);
            Some(ColumnMetadata {
                database_name: rows
                    .text(row, labels::CATALOG)
                    .filter(|catalog| !catalog.is_empty())
                    .unwrap_or_else(|| database_name.to_owned()),
                schema_name: rows
                    .text(row, labels::SCHEMA)
                    .filter(|schema| !schema.is_empty())
                    .unwrap_or_else(|| schema_name.to_owned()),
                table_name: rows.text(row, labels::TABLE).unwrap_or_default(),
                name,
                column_type: rows.text(row, labels::TYPE_NAME).unwrap_or_default(),
                data_type: rows
                    .integer(row, labels::DATA_TYPE)
                    .and_then(|value| i32::try_from(value).ok()),
                default_value: rows.text(row, labels::COLUMN_DEF),
                auto_increment: rows.yes(row, labels::AUTO_INCREMENT),
                comment: rows.text(row, labels::REMARKS).unwrap_or_default(),
                column_size: rows
                    .integer(row, labels::COLUMN_SIZE)
                    .and_then(|value| i32::try_from(value).ok()),
                decimal_digits: rows
                    .integer(row, labels::DECIMAL_DIGITS)
                    .and_then(|value| i32::try_from(value).ok()),
                ordinal_position: rows
                    .integer(row, labels::ORDINAL)
                    .and_then(|value| i32::try_from(value).ok()),
                // JDBC reports 1 for nullable, 0 for "unknown", 2 for not null.
                nullable: nullable.and_then(|value| i32::try_from(value).ok()),
                ..ColumnMetadata::default()
            })
        })
        .collect();
    ColumnList { items }
}

/// System schemas are hidden from the default tree in every dialect.
fn is_system_schema(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "information_schema" | "pg_catalog" | "pg_toast" | "sys" | "system" | "mysql"
            | "performance_schema" | "sysaux"
    )
}


#[cfg(test)]
mod tests {
    use chat2db_java_bridge::{ColumnNullability, JdbcValueType};

    use super::*;

    fn column(label: &str, ordinal: u32) -> JdbcColumn {
        JdbcColumn {
            ordinal,
            label: label.to_owned(),
            name: label.to_owned(),
            jdbc_type: 12,
            jdbc_type_name: "VARCHAR".to_owned(),
            value_type: JdbcValueType::Text,
            nullability: ColumnNullability::Nullable,
            precision: None,
            scale: None,
            display_size: None,
            signed: None,
            catalog_name: None,
            schema_name: None,
            table_name: None,
        }
    }

    fn text_row(values: &[&str]) -> JdbcRow {
        JdbcRow {
            values: values
                .iter()
                .map(|value| JdbcValue::Text((*value).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn tables_follow_the_jdbc_labels() {
        let result = MetadataResult {
            columns: vec![
                column("TABLE_CAT", 1),
                column("TABLE_SCHEM", 2),
                column("TABLE_NAME", 3),
                column("TABLE_TYPE", 4),
                column("REMARKS", 5),
            ],
            rows: vec![
                text_row(&["inventory", "APP", "items", "TABLE", "order lines"]),
                text_row(&["inventory", "APP", "", "TABLE", ""]),
            ],
            truncated: false,
        };

        let tables = tables(&result, "fallback_db", "fallback_schema", "dm");
        assert_eq!(tables.items.len(), 1, "empty table names are dropped");
        let table = &tables.items[0];
        assert_eq!(table.name, "items");
        assert_eq!(table.schema_name, "APP");
        assert_eq!(table.database_name, "inventory");
        assert_eq!(table.table_type, "TABLE");
        assert_eq!(table.comment, "order lines");
        assert_eq!(table.database_type, "dm");
    }

    #[test]
    fn tables_fall_back_to_the_request_scope() {
        let result = MetadataResult {
            columns: vec![column("TABLE_NAME", 1)],
            rows: vec![text_row(&["items"])],
            truncated: false,
        };
        let tables = tables(&result, "inventory", "PUBLIC", "h2");
        assert_eq!(tables.items[0].database_name, "inventory");
        assert_eq!(tables.items[0].schema_name, "PUBLIC");
    }

    #[test]
    fn columns_read_types_sizes_and_nullability() {
        let result = MetadataResult {
            columns: vec![
                column("TABLE_CAT", 1),
                column("TABLE_SCHEM", 2),
                column("TABLE_NAME", 3),
                column("COLUMN_NAME", 4),
                column("DATA_TYPE", 5),
                column("TYPE_NAME", 6),
                column("COLUMN_SIZE", 7),
                column("DECIMAL_DIGITS", 8),
                column("NULLABLE", 9),
                column("ORDINAL_POSITION", 10),
                column("COLUMN_DEF", 11),
                column("REMARKS", 12),
                column("IS_AUTOINCREMENT", 13),
            ],
            rows: vec![JdbcRow {
                values: vec![
                    JdbcValue::Text("inventory".to_owned()),
                    JdbcValue::Text("APP".to_owned()),
                    JdbcValue::Text("items".to_owned()),
                    JdbcValue::Text("id".to_owned()),
                    JdbcValue::SignedInteger(4),
                    JdbcValue::Text("INTEGER".to_owned()),
                    JdbcValue::SignedInteger(10),
                    JdbcValue::SignedInteger(0),
                    JdbcValue::SignedInteger(0),
                    JdbcValue::SignedInteger(1),
                    JdbcValue::Text("0".to_owned()),
                    JdbcValue::Text("primary key".to_owned()),
                    JdbcValue::Text("YES".to_owned()),
                ],
            }],
            truncated: false,
        };

        let columns = columns(&result, "fallback", "fallback");
        assert_eq!(columns.items.len(), 1);
        let column = &columns.items[0];
        assert_eq!(column.name, "id");
        assert_eq!(column.table_name, "items");
        assert_eq!(column.column_type, "INTEGER");
        assert_eq!(column.data_type, Some(4));
        assert_eq!(column.column_size, Some(10));
        assert_eq!(column.decimal_digits, Some(0));
        assert_eq!(column.nullable, Some(0));
        assert_eq!(column.ordinal_position, Some(1));
        assert_eq!(column.default_value.as_deref(), Some("0"));
        assert_eq!(column.comment, "primary key");
        assert_eq!(column.auto_increment, Some(true));
    }

    #[test]
    fn schemas_flag_system_catalogs() {
        let result = MetadataResult {
            columns: vec![column("TABLE_SCHEM", 1), column("TABLE_CATALOG", 2)],
            rows: vec![
                text_row(&["APP", "inventory"]),
                text_row(&["INFORMATION_SCHEMA", "inventory"]),
            ],
            truncated: false,
        };

        let schemas = schemas(&result, "inventory");
        assert_eq!(schemas.items.len(), 2);
        assert!(!schemas.items[0].system);
        assert!(schemas.items[1].system);
        assert_eq!(schemas.items[1].name, "INFORMATION_SCHEMA");
    }
}

