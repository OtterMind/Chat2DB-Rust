use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chat2db_contract as contract;
use chat2db_engine_protocol::wire;
use chat2db_java_bridge as bridge;
use chat2db_storage as storage;

use crate::{
    AppError,
    query::{DatabaseValue, QueryExecutionOptions, QueryParameter},
};

pub(crate) const fn provider_kind_to_storage(
    kind: contract::ProviderKind,
) -> storage::ProviderKind {
    match kind {
        contract::ProviderKind::OpenAiCompatible => storage::ProviderKind::OpenAiCompatible,
        contract::ProviderKind::Anthropic => storage::ProviderKind::Anthropic,
        contract::ProviderKind::Gemini => storage::ProviderKind::Gemini,
    }
}

pub(crate) const fn provider_kind(kind: storage::ProviderKind) -> contract::ProviderKind {
    match kind {
        storage::ProviderKind::OpenAiCompatible => contract::ProviderKind::OpenAiCompatible,
        storage::ProviderKind::Anthropic => contract::ProviderKind::Anthropic,
        storage::ProviderKind::Gemini => contract::ProviderKind::Gemini,
    }
}

pub(crate) fn provider_profile(
    record: storage::ProviderProfileRecord,
) -> contract::ProviderProfile {
    contract::ProviderProfile {
        id: record.id,
        name: record.name,
        kind: provider_kind(record.kind),
        base_url: record.base_url,
        model: record.model,
        context_window_tokens: record.context_window_tokens.to_string(),
        max_output_tokens: record.max_output_tokens.to_string(),
        has_secret: record.secret_ref.is_some(),
        revision: record.revision.to_string(),
        created_at_ms: record.created_at_ms.to_string(),
        updated_at_ms: record.updated_at_ms.to_string(),
    }
}

pub(crate) fn agent_session(record: storage::AgentSessionRecord) -> contract::AgentSession {
    contract::AgentSession {
        id: record.id,
        title: record.title,
        provider_id: record.provider_id,
        datasource_id: record.datasource_id,
        revision: record.revision.to_string(),
        created_at_ms: record.created_at_ms.to_string(),
        updated_at_ms: record.updated_at_ms.to_string(),
    }
}

pub(crate) fn agent_message(
    record: storage::AgentMessageRecord,
) -> Result<contract::AgentMessage, AppError> {
    let content = serde_json::from_str::<Vec<contract::AgentMessageContent>>(&record.content_json)
        .map_err(|_| AppError::internal())?;
    Ok(contract::AgentMessage {
        id: record.id,
        session_id: record.session_id,
        run_id: record.run_id,
        role: agent_message_role(record.role),
        content,
        ordinal: record.ordinal.to_string(),
        created_at_ms: record.created_at_ms.to_string(),
    })
}

const fn agent_message_role(role: storage::AgentMessageRole) -> contract::AgentMessageRole {
    match role {
        storage::AgentMessageRole::System => contract::AgentMessageRole::System,
        storage::AgentMessageRole::User => contract::AgentMessageRole::User,
        storage::AgentMessageRole::Assistant => contract::AgentMessageRole::Assistant,
        storage::AgentMessageRole::Tool => contract::AgentMessageRole::Tool,
        storage::AgentMessageRole::Summary => contract::AgentMessageRole::Summary,
    }
}

pub(crate) fn datasource(record: storage::DatasourceRecord) -> contract::Datasource {
    contract::Datasource {
        id: record.id,
        name: record.name,
        driver_id: record.driver_id,
        has_secret: record.secret_ref.is_some(),
        revision: record.revision.to_string(),
        created_at_ms: record.created_at_ms.to_string(),
        updated_at_ms: record.updated_at_ms.to_string(),
    }
}

pub(crate) fn result_metadata(metadata: storage::ResultMetadata) -> contract::ResultMetadata {
    contract::ResultMetadata {
        id: metadata.id,
        row_count: metadata.row_count.to_string(),
        byte_count: metadata.byte_count.to_string(),
        truncated_by_max_rows: metadata.truncated_by_max_rows,
        truncated_by_max_result_bytes: metadata.truncated_by_max_result_bytes,
        created_at_ms: metadata.created_at_ms.to_string(),
        expires_at_ms: metadata.expires_at_ms.to_string(),
    }
}

pub(crate) fn result_page(page: storage::ResultPage) -> Result<contract::ResultPage, AppError> {
    Ok(contract::ResultPage {
        metadata: result_metadata(page.metadata),
        columns: page
            .schema
            .columns
            .into_iter()
            .map(result_column)
            .collect::<Result<_, _>>()?,
        offset: page.offset.to_string(),
        rows: page
            .rows
            .into_iter()
            .map(result_row)
            .collect::<Result<_, _>>()?,
        has_more: page.has_more,
    })
}

pub(crate) fn query_parameter(
    parameter: contract::QueryParameter,
) -> Result<QueryParameter, AppError> {
    Ok(QueryParameter {
        position: parameter.position,
        value: database_value_from_contract(parameter.value)?,
    })
}

fn database_value_from_contract(value: contract::JdbcValue) -> Result<DatabaseValue, AppError> {
    Ok(match value {
        contract::JdbcValue::Null => DatabaseValue::Null,
        contract::JdbcValue::Boolean { value } => DatabaseValue::Boolean(value),
        contract::JdbcValue::SignedInteger { value } => {
            DatabaseValue::SignedInteger(parse_number(&value, "signed integer")?)
        }
        contract::JdbcValue::UnsignedInteger { value } => {
            DatabaseValue::UnsignedInteger(parse_number(&value, "unsigned integer")?)
        }
        contract::JdbcValue::Float32 { value } => DatabaseValue::Float32(parse_float32(&value)?),
        contract::JdbcValue::Float64 { value } => DatabaseValue::Float64(parse_float64(&value)?),
        contract::JdbcValue::Decimal { value } => DatabaseValue::Decimal(value),
        contract::JdbcValue::Text { value } => DatabaseValue::Text(value),
        contract::JdbcValue::Binary { value } => {
            DatabaseValue::Binary(BASE64_STANDARD.decode(value).map_err(|_| {
                AppError::invalid(
                    "invalid_query_parameter",
                    "Binary parameters must use base64",
                )
            })?)
        }
        contract::JdbcValue::Date { value } => DatabaseValue::Date(value),
        contract::JdbcValue::Time { value } => DatabaseValue::Time(value),
        contract::JdbcValue::Timestamp { value } => DatabaseValue::Timestamp(value),
        contract::JdbcValue::TimestampWithTimeZone { value } => {
            DatabaseValue::TimestampWithTimeZone(value)
        }
        contract::JdbcValue::Json { value } => DatabaseValue::Json(value),
        contract::JdbcValue::Uuid { value } => DatabaseValue::Uuid(value),
        contract::JdbcValue::Opaque { .. } => {
            return Err(AppError::invalid(
                "invalid_query_parameter",
                "Opaque result values cannot be query parameters",
            ));
        }
    })
}

pub(crate) fn query_parameter_to_java(parameter: QueryParameter) -> bridge::JdbcParameter {
    bridge::JdbcParameter {
        position: parameter.position,
        value: database_value_to_java(parameter.value),
        jdbc_type: None,
        jdbc_type_name: None,
    }
}

pub(crate) const fn query_options_to_java(options: QueryExecutionOptions) -> bridge::QueryOptions {
    bridge::QueryOptions {
        max_rows: options.max_rows,
        target_batch_rows: options.target_batch_rows,
        target_batch_bytes: options.target_batch_bytes,
        initial_batch_credits: options.initial_batch_credits,
        max_result_bytes: options.max_result_bytes,
    }
}

fn database_value_to_java(value: DatabaseValue) -> bridge::JdbcValue {
    match value {
        DatabaseValue::Null => bridge::JdbcValue::Null,
        DatabaseValue::Boolean(value) => bridge::JdbcValue::Boolean(value),
        DatabaseValue::SignedInteger(value) => bridge::JdbcValue::SignedInteger(value),
        DatabaseValue::UnsignedInteger(value) => bridge::JdbcValue::UnsignedInteger(value),
        DatabaseValue::Float32(value) => bridge::JdbcValue::Float32(value),
        DatabaseValue::Float64(value) => bridge::JdbcValue::Float64(value),
        DatabaseValue::Decimal(value) => bridge::JdbcValue::Decimal(value),
        DatabaseValue::Text(value) => bridge::JdbcValue::Text(value),
        DatabaseValue::Binary(value) => bridge::JdbcValue::Binary(value),
        DatabaseValue::Date(value) => bridge::JdbcValue::Date(value),
        DatabaseValue::Time(value) => bridge::JdbcValue::Time(value),
        DatabaseValue::Timestamp(value) => bridge::JdbcValue::Timestamp(value),
        DatabaseValue::TimestampWithTimeZone(value) => {
            bridge::JdbcValue::TimestampWithTimeZone(value)
        }
        DatabaseValue::Json(value) => bridge::JdbcValue::Json(value),
        DatabaseValue::Uuid(value) => bridge::JdbcValue::Uuid(value),
    }
}

fn result_row(row: wire::JdbcRow) -> Result<contract::ResultRow, AppError> {
    Ok(contract::ResultRow {
        values: row
            .values
            .into_iter()
            .map(result_value)
            .collect::<Result<_, _>>()?,
    })
}

fn result_value(value: wire::JdbcValue) -> Result<contract::JdbcValue, AppError> {
    use wire::jdbc_value::Value;

    Ok(match value.value.ok_or_else(AppError::internal)? {
        Value::NullValue(_) => contract::JdbcValue::Null,
        Value::BooleanValue(value) => contract::JdbcValue::Boolean { value },
        Value::SignedIntegerValue(value) => contract::JdbcValue::SignedInteger {
            value: value.to_string(),
        },
        Value::UnsignedIntegerValue(value) => contract::JdbcValue::UnsignedInteger {
            value: value.to_string(),
        },
        Value::Float32Value(value) => contract::JdbcValue::Float32 {
            value: float32_string(value),
        },
        Value::Float64Value(value) => contract::JdbcValue::Float64 {
            value: float64_string(value),
        },
        Value::DecimalValue(value) => contract::JdbcValue::Decimal { value },
        Value::TextValue(value) => contract::JdbcValue::Text { value },
        Value::BinaryValue(value) => contract::JdbcValue::Binary {
            value: BASE64_STANDARD.encode(value),
        },
        Value::DateValue(value) => contract::JdbcValue::Date { value },
        Value::TimeValue(value) => contract::JdbcValue::Time { value },
        Value::TimestampValue(value) => contract::JdbcValue::Timestamp { value },
        Value::TimestampWithTimeZoneValue(value) => {
            contract::JdbcValue::TimestampWithTimeZone { value }
        }
        Value::JsonValue(value) => contract::JdbcValue::Json { value },
        Value::UuidValue(value) => contract::JdbcValue::Uuid { value },
        Value::OpaqueValue(value) => contract::JdbcValue::Opaque {
            type_name: value.type_name,
            display_value: value.display_value,
        },
    })
}

fn result_column(column: wire::JdbcColumn) -> Result<contract::ResultColumn, AppError> {
    let value_type = match wire::JdbcValueType::try_from(column.value_type) {
        Ok(wire::JdbcValueType::Boolean) => contract::JdbcValueType::Boolean,
        Ok(wire::JdbcValueType::SignedInteger) => contract::JdbcValueType::SignedInteger,
        Ok(wire::JdbcValueType::UnsignedInteger) => contract::JdbcValueType::UnsignedInteger,
        Ok(wire::JdbcValueType::Float32) => contract::JdbcValueType::Float32,
        Ok(wire::JdbcValueType::Float64) => contract::JdbcValueType::Float64,
        Ok(wire::JdbcValueType::Decimal) => contract::JdbcValueType::Decimal,
        Ok(wire::JdbcValueType::Text) => contract::JdbcValueType::Text,
        Ok(wire::JdbcValueType::Binary) => contract::JdbcValueType::Binary,
        Ok(wire::JdbcValueType::Date) => contract::JdbcValueType::Date,
        Ok(wire::JdbcValueType::Time) => contract::JdbcValueType::Time,
        Ok(wire::JdbcValueType::Timestamp) => contract::JdbcValueType::Timestamp,
        Ok(wire::JdbcValueType::TimestampWithTimeZone) => {
            contract::JdbcValueType::TimestampWithTimeZone
        }
        Ok(wire::JdbcValueType::Json) => contract::JdbcValueType::Json,
        Ok(wire::JdbcValueType::Uuid) => contract::JdbcValueType::Uuid,
        Ok(wire::JdbcValueType::Opaque) => contract::JdbcValueType::Opaque,
        Ok(wire::JdbcValueType::Unspecified) | Err(_) => return Err(AppError::internal()),
    };
    let nullability = match wire::ColumnNullability::try_from(column.nullability) {
        Ok(wire::ColumnNullability::Unknown) => contract::ColumnNullability::Unknown,
        Ok(wire::ColumnNullability::NoNulls) => contract::ColumnNullability::NoNulls,
        Ok(wire::ColumnNullability::Nullable) => contract::ColumnNullability::Nullable,
        Err(_) => return Err(AppError::internal()),
    };
    Ok(contract::ResultColumn {
        ordinal: column.ordinal,
        label: column.label,
        name: column.name,
        jdbc_type: column.jdbc_type,
        jdbc_type_name: column.jdbc_type_name,
        value_type,
        nullability,
        precision: column.precision,
        scale: column.scale,
        display_size: column.display_size,
        signed: column.signed,
        catalog_name: column.catalog_name,
        schema_name: column.schema_name,
        table_name: column.table_name,
    })
}

fn parse_number<T>(value: &str, label: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
{
    value.parse().map_err(|_| {
        AppError::invalid(
            "invalid_query_parameter",
            format!("The {label} parameter is outside its supported range"),
        )
    })
}

fn parse_float32(value: &str) -> Result<f32, AppError> {
    match value {
        "NaN" => Ok(f32::NAN),
        "Infinity" => Ok(f32::INFINITY),
        "-Infinity" => Ok(f32::NEG_INFINITY),
        _ => {
            let parsed: f32 = parse_number(value, "float32")?;
            if parsed.is_finite() {
                Ok(parsed)
            } else {
                Err(invalid_float())
            }
        }
    }
}

fn parse_float64(value: &str) -> Result<f64, AppError> {
    match value {
        "NaN" => Ok(f64::NAN),
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        _ => {
            let parsed: f64 = parse_number(value, "float64")?;
            if parsed.is_finite() {
                Ok(parsed)
            } else {
                Err(invalid_float())
            }
        }
    }
}

fn invalid_float() -> AppError {
    AppError::invalid(
        "invalid_query_parameter",
        "Floating-point parameters must be finite decimals, NaN, Infinity, or -Infinity",
    )
}

fn float32_string(value: f32) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f32::INFINITY {
        "Infinity".to_owned()
    } else if value == f32::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        value.to_string()
    }
}

fn float64_string(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Infinity".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::JdbcValue;
    use chat2db_engine_protocol::wire;

    use super::{
        database_value_from_contract, database_value_to_java, query_options_to_java,
        query_parameter_to_java, result_value,
    };
    use crate::query::{DatabaseValue, QueryExecutionOptions, QueryParameter};

    #[test]
    fn all_sixteen_portable_jdbc_values_round_trip() {
        let values = vec![
            JdbcValue::Null,
            JdbcValue::Boolean { value: true },
            JdbcValue::SignedInteger {
                value: i64::MIN.to_string(),
            },
            JdbcValue::UnsignedInteger {
                value: u64::MAX.to_string(),
            },
            JdbcValue::Float32 {
                value: "NaN".to_owned(),
            },
            JdbcValue::Float64 {
                value: "-Infinity".to_owned(),
            },
            JdbcValue::Decimal {
                value: "123.4500".to_owned(),
            },
            JdbcValue::Text {
                value: "text".to_owned(),
            },
            JdbcValue::Binary {
                value: "AAH/".to_owned(),
            },
            JdbcValue::Date {
                value: "2026-07-25".to_owned(),
            },
            JdbcValue::Time {
                value: "12:34:56".to_owned(),
            },
            JdbcValue::Timestamp {
                value: "2026-07-25T12:34:56".to_owned(),
            },
            JdbcValue::TimestampWithTimeZone {
                value: "2026-07-25T12:34:56+08:00".to_owned(),
            },
            JdbcValue::Json {
                value: "{\"ok\":true}".to_owned(),
            },
            JdbcValue::Uuid {
                value: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            },
        ];

        for value in values {
            let neutral = database_value_from_contract(value.clone()).expect("request conversion");
            let bridge = database_value_to_java(neutral);
            let wire = wire::JdbcValue::from(bridge);
            assert_eq!(result_value(wire).expect("result conversion"), value);
        }

        let opaque = wire::JdbcValue {
            value: Some(wire::jdbc_value::Value::OpaqueValue(wire::OpaqueValue {
                type_name: "vendor.Type".to_owned(),
                display_value: "opaque".to_owned(),
            })),
        };
        assert!(matches!(
            result_value(opaque).expect("opaque output"),
            JdbcValue::Opaque { .. }
        ));
    }

    #[test]
    fn opaque_and_noncanonical_infinity_are_rejected_as_inputs() {
        assert!(
            database_value_from_contract(JdbcValue::Opaque {
                type_name: "vendor.Type".to_owned(),
                display_value: "value".to_owned(),
            })
            .is_err()
        );
        assert!(
            database_value_from_contract(JdbcValue::Float64 {
                value: "inf".to_owned(),
            })
            .is_err()
        );
    }

    #[test]
    fn java_query_adapter_preserves_parameter_and_execution_options() {
        let parameter = query_parameter_to_java(QueryParameter {
            position: 7,
            value: DatabaseValue::Text("inventory".to_owned()),
        });
        assert_eq!(parameter.position, 7);
        assert!(matches!(
            parameter.value,
            chat2db_java_bridge::JdbcValue::Text(value) if value == "inventory"
        ));
        assert_eq!(parameter.jdbc_type, None);
        assert_eq!(parameter.jdbc_type_name, None);

        let options = query_options_to_java(QueryExecutionOptions {
            max_rows: 101,
            target_batch_rows: 102,
            target_batch_bytes: 103,
            initial_batch_credits: 104,
            max_result_bytes: 105,
        });
        assert_eq!(options.max_rows, 101);
        assert_eq!(options.target_batch_rows, 102);
        assert_eq!(options.target_batch_bytes, 103);
        assert_eq!(options.initial_batch_credits, 104);
        assert_eq!(options.max_result_bytes, 105);
    }
}
