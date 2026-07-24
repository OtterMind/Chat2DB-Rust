use std::{
    collections::HashSet,
    fmt::Write as _,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use chat2db_engine_protocol::wire;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::{BridgeError, SessionState, error::PendingFailure, state::SessionStateCell};

use super::{
    EngineClient,
    pending::{ControlEffect, PendingLane},
};

pub const DRIVER_EXTERNAL_JAR_CAPABILITY: &str = "driver.external-jar.v1";
pub const SESSION_JDBC_CAPABILITY: &str = "session.jdbc.v1";
pub const QUERY_TYPED_BATCHES_CAPABILITY: &str = "query.typed-batches.v1";
pub const FLOW_CREDIT_CAPABILITY: &str = "flow.credit.v1";
pub const OPERATION_CANCEL_CAPABILITY: &str = "operation.cancel.v1";
pub const UPDATE_JDBC_CAPABILITY: &str = "update.jdbc.v1";
pub const TRANSACTION_LOCAL_CAPABILITY: &str = "transaction.local.v1";

const MAX_DRIVER_ARTIFACTS: usize = wire::JdbcProtocolLimit::MaxDriverArtifacts as usize;
const MAX_CONNECTION_PROPERTIES: usize = wire::JdbcProtocolLimit::MaxConnectionProperties as usize;
const MAX_DRIVER_ID_BYTES: usize = wire::JdbcProtocolLimit::MaxDriverIdBytes as usize;
const MAX_PROPERTY_KEY_BYTES: usize = wire::JdbcProtocolLimit::MaxPropertyKeyBytes as usize;
const MAX_DRIVER_CLASS_BYTES: usize = wire::JdbcProtocolLimit::MaxDriverClassBytes as usize;
const MAX_BATCH_ROWS: u32 = wire::JdbcProtocolLimit::MaxBatchRows as u32;
const MAX_PATH_BYTES: usize = wire::JdbcProtocolLimit::MaxPathBytes as usize;
const MAX_PARAMETERS: usize = wire::JdbcProtocolLimit::MaxParameters as usize;
const MAX_JDBC_URL_BYTES: usize = wire::JdbcProtocolLimit::MaxJdbcUrlBytes as usize;
const MAX_PROPERTY_VALUE_BYTES: usize = wire::JdbcProtocolLimit::MaxPropertyValueBytes as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_BATCH_BYTES: u32 = wire::JdbcProtocolLimit::MaxBatchBytes as u32;
const MAX_CREDIT_GRANT: u32 = wire::JdbcProtocolLimit::MaxCreditGrant as u32;
const MAX_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::MaxResultBytes as u64;
const DRIVER_ID_DOMAIN_SEPARATOR: &[u8] = b"chat2db-jdbc-driver-v1\0";

fn invalid_request(message: impl Into<String>) -> BridgeError {
    BridgeError::InvalidRequest(message.into())
}

fn validate_utf8(value: &str, maximum_bytes: usize, field: &str) -> Result<(), BridgeError> {
    if value.len() > maximum_bytes {
        return Err(invalid_request(format!(
            "{field} cannot exceed {maximum_bytes} UTF-8 bytes"
        )));
    }
    Ok(())
}

fn validate_non_blank_utf8(
    value: &str,
    maximum_bytes: usize,
    field: &str,
) -> Result<(), BridgeError> {
    if value.trim().is_empty() {
        return Err(invalid_request(format!("{field} is required")));
    }
    validate_utf8(value, maximum_bytes, field)
}

fn validate_protocol_id(value: &str, field: &str) -> Result<(), BridgeError> {
    validate_non_blank_utf8(value, MAX_DRIVER_ID_BYTES, field)
}

fn validate_count(actual: usize, maximum: usize, field: &str) -> Result<(), BridgeError> {
    if actual > maximum {
        return Err(invalid_request(format!(
            "{field} cannot contain more than {maximum} entries"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverArtifact {
    canonical_path: PathBuf,
    sha256: [u8; 32],
}

impl DriverArtifact {
    /// Resolves an external JAR to a canonical path and hashes its bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be canonicalized, is not a file,
    /// cannot be read, or cannot be represented by the UTF-8 wire contract.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, BridgeError> {
        let input_path = path.as_ref();
        let canonical_path =
            std::fs::canonicalize(input_path).map_err(|source| BridgeError::DriverArtifact {
                path: input_path.to_path_buf(),
                source,
            })?;
        let metadata =
            std::fs::metadata(&canonical_path).map_err(|source| BridgeError::DriverArtifact {
                path: canonical_path.clone(),
                source,
            })?;
        if !metadata.is_file() {
            return Err(BridgeError::InvalidRequest(format!(
                "driver artifact {} is not a regular file",
                canonical_path.display()
            )));
        }
        let Some(canonical_path_text) = canonical_path.to_str() else {
            return Err(BridgeError::NonUtf8DriverArtifact(canonical_path));
        };
        validate_non_blank_utf8(canonical_path_text, MAX_PATH_BYTES, "driver artifact path")?;

        let mut file =
            File::open(&canonical_path).map_err(|source| BridgeError::DriverArtifact {
                path: canonical_path.clone(),
                source,
            })?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|source| BridgeError::DriverArtifact {
                    path: canonical_path.clone(),
                    source,
                })?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }

        Ok(Self {
            canonical_path,
            sha256: hasher.finalize().into(),
        })
    }

    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    #[must_use]
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    fn validate(&self) -> Result<(), BridgeError> {
        if !self.canonical_path.is_absolute() {
            return Err(invalid_request("driver artifact path must be absolute"));
        }
        let Some(path) = self.canonical_path.to_str() else {
            return Err(BridgeError::NonUtf8DriverArtifact(
                self.canonical_path.clone(),
            ));
        };
        validate_non_blank_utf8(path, MAX_PATH_BYTES, "driver artifact path")
    }

    fn to_wire(&self) -> wire::DriverArtifact {
        wire::DriverArtifact {
            path: self
                .canonical_path
                .to_str()
                .expect("driver artifact paths are validated at construction")
                .to_owned(),
            sha256: self.sha256.to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverSpec {
    pub driver_class: String,
    pub artifacts: Vec<DriverArtifact>,
}

impl DriverSpec {
    fn validate(&self) -> Result<(), BridgeError> {
        validate_non_blank_utf8(&self.driver_class, MAX_DRIVER_CLASS_BYTES, "driver_class")?;
        if self.artifacts.is_empty() {
            return Err(invalid_request(
                "at least one external driver artifact is required",
            ));
        }
        validate_count(
            self.artifacts.len(),
            MAX_DRIVER_ARTIFACTS,
            "driver artifacts",
        )?;
        self.artifacts.iter().try_for_each(DriverArtifact::validate)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedDriver {
    pub driver_id: String,
    pub driver_class: String,
    pub artifact_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionProperty {
    pub key: String,
    pub value: String,
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    pub driver_id: String,
    pub jdbc_url: String,
    pub properties: Vec<ConnectionProperty>,
    pub read_only: bool,
}

impl SessionConfig {
    fn validate(&self) -> Result<(), BridgeError> {
        validate_protocol_id(&self.driver_id, "driver_id")?;
        validate_non_blank_utf8(&self.jdbc_url, MAX_JDBC_URL_BYTES, "jdbc_url")?;
        validate_count(
            self.properties.len(),
            MAX_CONNECTION_PROPERTIES,
            "connection properties",
        )?;

        let mut keys = HashSet::with_capacity(self.properties.len());
        for property in &self.properties {
            validate_non_blank_utf8(&property.key, MAX_PROPERTY_KEY_BYTES, "property key")?;
            validate_utf8(&property.value, MAX_PROPERTY_VALUE_BYTES, "property value")?;
            if !keys.insert(property.key.as_str()) {
                return Err(invalid_request("connection property keys must be unique"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseProduct {
    pub name: String,
    pub version: String,
    pub driver_name: String,
    pub driver_version: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransactionIsolation {
    #[default]
    Default,
    ReadUncommitted,
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl TransactionIsolation {
    const fn to_wire(self) -> wire::TransactionIsolation {
        match self {
            Self::Default => wire::TransactionIsolation::Default,
            Self::ReadUncommitted => wire::TransactionIsolation::ReadUncommitted,
            Self::ReadCommitted => wire::TransactionIsolation::ReadCommitted,
            Self::RepeatableRead => wire::TransactionIsolation::RepeatableRead,
            Self::Serializable => wire::TransactionIsolation::Serializable,
        }
    }

    fn from_wire(value: i32) -> Result<Self, String> {
        match wire::TransactionIsolation::try_from(value) {
            Ok(wire::TransactionIsolation::Default) => Ok(Self::Default),
            Ok(wire::TransactionIsolation::ReadUncommitted) => Ok(Self::ReadUncommitted),
            Ok(wire::TransactionIsolation::ReadCommitted) => Ok(Self::ReadCommitted),
            Ok(wire::TransactionIsolation::RepeatableRead) => Ok(Self::RepeatableRead),
            Ok(wire::TransactionIsolation::Serializable) => Ok(Self::Serializable),
            Err(_) => Err(format!("unknown transaction isolation value {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransactionOptions {
    pub isolation: TransactionIsolation,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JdbcValue {
    Null,
    Boolean(bool),
    SignedInteger(i64),
    UnsignedInteger(u64),
    Float32(f32),
    Float64(f64),
    Decimal(String),
    Text(String),
    Binary(Vec<u8>),
    Date(String),
    Time(String),
    Timestamp(String),
    TimestampWithTimeZone(String),
    Json(String),
    Uuid(String),
    Opaque {
        type_name: String,
        display_value: String,
    },
}

impl JdbcValue {
    fn validate_parameter(&self) -> Result<(), BridgeError> {
        let (value, field) = match self {
            Self::Decimal(value) => (value.as_str(), "decimal value"),
            Self::Text(value) => (value.as_str(), "text value"),
            Self::Date(value) => (value.as_str(), "date value"),
            Self::Time(value) => (value.as_str(), "time value"),
            Self::Timestamp(value) => (value.as_str(), "timestamp value"),
            Self::TimestampWithTimeZone(value) => {
                (value.as_str(), "timestamp with time zone value")
            }
            Self::Json(value) => (value.as_str(), "JSON value"),
            Self::Uuid(value) => (value.as_str(), "UUID value"),
            Self::Binary(value) => {
                if value.len() > MAX_SCALAR_BYTES {
                    return Err(invalid_request(format!(
                        "binary value cannot exceed {MAX_SCALAR_BYTES} bytes"
                    )));
                }
                return Ok(());
            }
            Self::Opaque { .. } => {
                return Err(invalid_request(
                    "opaque JDBC values cannot be statement parameters",
                ));
            }
            Self::Null
            | Self::Boolean(_)
            | Self::SignedInteger(_)
            | Self::UnsignedInteger(_)
            | Self::Float32(_)
            | Self::Float64(_) => return Ok(()),
        };
        validate_utf8(value, MAX_SCALAR_BYTES, field)
    }

    fn to_parameter_wire(&self) -> Result<wire::JdbcValue, BridgeError> {
        use wire::jdbc_value::Value;

        self.validate_parameter()?;
        let value = match self {
            Self::Null => Value::NullValue(wire::JdbcNull {}),
            Self::Boolean(value) => Value::BooleanValue(*value),
            Self::SignedInteger(value) => Value::SignedIntegerValue(*value),
            Self::UnsignedInteger(value) => Value::UnsignedIntegerValue(*value),
            Self::Float32(value) => Value::Float32Value(*value),
            Self::Float64(value) => Value::Float64Value(*value),
            Self::Decimal(value) => Value::DecimalValue(value.clone()),
            Self::Text(value) => Value::TextValue(value.clone()),
            Self::Binary(value) => Value::BinaryValue(value.clone()),
            Self::Date(value) => Value::DateValue(value.clone()),
            Self::Time(value) => Value::TimeValue(value.clone()),
            Self::Timestamp(value) => Value::TimestampValue(value.clone()),
            Self::TimestampWithTimeZone(value) => Value::TimestampWithTimeZoneValue(value.clone()),
            Self::Json(value) => Value::JsonValue(value.clone()),
            Self::Uuid(value) => Value::UuidValue(value.clone()),
            Self::Opaque { .. } => {
                return Err(BridgeError::InvalidRequest(
                    "opaque JDBC values cannot be statement parameters".to_owned(),
                ));
            }
        };
        Ok(wire::JdbcValue { value: Some(value) })
    }

    fn from_wire(value: wire::JdbcValue) -> Result<Self, String> {
        use wire::jdbc_value::Value;

        match value.value {
            Some(Value::NullValue(_)) => Ok(Self::Null),
            Some(Value::BooleanValue(value)) => Ok(Self::Boolean(value)),
            Some(Value::SignedIntegerValue(value)) => Ok(Self::SignedInteger(value)),
            Some(Value::UnsignedIntegerValue(value)) => Ok(Self::UnsignedInteger(value)),
            Some(Value::Float32Value(value)) => Ok(Self::Float32(value)),
            Some(Value::Float64Value(value)) => Ok(Self::Float64(value)),
            Some(Value::DecimalValue(value)) => Ok(Self::Decimal(value)),
            Some(Value::TextValue(value)) => Ok(Self::Text(value)),
            Some(Value::BinaryValue(value)) => Ok(Self::Binary(value)),
            Some(Value::DateValue(value)) => Ok(Self::Date(value)),
            Some(Value::TimeValue(value)) => Ok(Self::Time(value)),
            Some(Value::TimestampValue(value)) => Ok(Self::Timestamp(value)),
            Some(Value::TimestampWithTimeZoneValue(value)) => {
                Ok(Self::TimestampWithTimeZone(value))
            }
            Some(Value::JsonValue(value)) => Ok(Self::Json(value)),
            Some(Value::UuidValue(value)) => Ok(Self::Uuid(value)),
            Some(Value::OpaqueValue(value)) => Ok(Self::Opaque {
                type_name: value.type_name,
                display_value: value.display_value,
            }),
            None => Err("JDBC value payload is missing".to_owned()),
        }
    }
}

impl From<JdbcValue> for wire::JdbcValue {
    fn from(value: JdbcValue) -> Self {
        use wire::jdbc_value::Value;

        let value = match value {
            JdbcValue::Null => Value::NullValue(wire::JdbcNull {}),
            JdbcValue::Boolean(value) => Value::BooleanValue(value),
            JdbcValue::SignedInteger(value) => Value::SignedIntegerValue(value),
            JdbcValue::UnsignedInteger(value) => Value::UnsignedIntegerValue(value),
            JdbcValue::Float32(value) => Value::Float32Value(value),
            JdbcValue::Float64(value) => Value::Float64Value(value),
            JdbcValue::Decimal(value) => Value::DecimalValue(value),
            JdbcValue::Text(value) => Value::TextValue(value),
            JdbcValue::Binary(value) => Value::BinaryValue(value),
            JdbcValue::Date(value) => Value::DateValue(value),
            JdbcValue::Time(value) => Value::TimeValue(value),
            JdbcValue::Timestamp(value) => Value::TimestampValue(value),
            JdbcValue::TimestampWithTimeZone(value) => Value::TimestampWithTimeZoneValue(value),
            JdbcValue::Json(value) => Value::JsonValue(value),
            JdbcValue::Uuid(value) => Value::UuidValue(value),
            JdbcValue::Opaque {
                type_name,
                display_value,
            } => Value::OpaqueValue(wire::OpaqueValue {
                type_name,
                display_value,
            }),
        };
        Self { value: Some(value) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JdbcParameter {
    pub position: u32,
    pub value: JdbcValue,
    pub jdbc_type: Option<i32>,
    pub jdbc_type_name: Option<String>,
}

impl JdbcParameter {
    fn validate(&self) -> Result<(), BridgeError> {
        if self.position == 0 || self.position > u32::try_from(MAX_PARAMETERS).unwrap_or(u32::MAX) {
            return Err(invalid_request(format!(
                "parameter position must be between 1 and {MAX_PARAMETERS}"
            )));
        }
        if let Some(jdbc_type_name) = &self.jdbc_type_name {
            validate_utf8(jdbc_type_name, MAX_SCALAR_BYTES, "jdbc_type_name")?;
        }
        self.value.validate_parameter()
    }

    fn to_wire(&self) -> Result<wire::JdbcParameter, BridgeError> {
        self.validate()?;
        Ok(wire::JdbcParameter {
            position: self.position,
            value: Some(self.value.to_parameter_wire()?),
            jdbc_type: self.jdbc_type,
            jdbc_type_name: self.jdbc_type_name.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JdbcValueType {
    Boolean,
    SignedInteger,
    UnsignedInteger,
    Float32,
    Float64,
    Decimal,
    Text,
    Binary,
    Date,
    Time,
    Timestamp,
    TimestampWithTimeZone,
    Json,
    Uuid,
    Opaque,
}

impl JdbcValueType {
    fn from_wire(value: i32) -> Result<Self, String> {
        match wire::JdbcValueType::try_from(value) {
            Ok(wire::JdbcValueType::Boolean) => Ok(Self::Boolean),
            Ok(wire::JdbcValueType::SignedInteger) => Ok(Self::SignedInteger),
            Ok(wire::JdbcValueType::UnsignedInteger) => Ok(Self::UnsignedInteger),
            Ok(wire::JdbcValueType::Float32) => Ok(Self::Float32),
            Ok(wire::JdbcValueType::Float64) => Ok(Self::Float64),
            Ok(wire::JdbcValueType::Decimal) => Ok(Self::Decimal),
            Ok(wire::JdbcValueType::Text) => Ok(Self::Text),
            Ok(wire::JdbcValueType::Binary) => Ok(Self::Binary),
            Ok(wire::JdbcValueType::Date) => Ok(Self::Date),
            Ok(wire::JdbcValueType::Time) => Ok(Self::Time),
            Ok(wire::JdbcValueType::Timestamp) => Ok(Self::Timestamp),
            Ok(wire::JdbcValueType::TimestampWithTimeZone) => Ok(Self::TimestampWithTimeZone),
            Ok(wire::JdbcValueType::Json) => Ok(Self::Json),
            Ok(wire::JdbcValueType::Uuid) => Ok(Self::Uuid),
            Ok(wire::JdbcValueType::Opaque) => Ok(Self::Opaque),
            Ok(wire::JdbcValueType::Unspecified) | Err(_) => {
                Err(format!("unknown JDBC value type {value}"))
            }
        }
    }
}

impl From<JdbcValueType> for wire::JdbcValueType {
    fn from(value: JdbcValueType) -> Self {
        match value {
            JdbcValueType::Boolean => Self::Boolean,
            JdbcValueType::SignedInteger => Self::SignedInteger,
            JdbcValueType::UnsignedInteger => Self::UnsignedInteger,
            JdbcValueType::Float32 => Self::Float32,
            JdbcValueType::Float64 => Self::Float64,
            JdbcValueType::Decimal => Self::Decimal,
            JdbcValueType::Text => Self::Text,
            JdbcValueType::Binary => Self::Binary,
            JdbcValueType::Date => Self::Date,
            JdbcValueType::Time => Self::Time,
            JdbcValueType::Timestamp => Self::Timestamp,
            JdbcValueType::TimestampWithTimeZone => Self::TimestampWithTimeZone,
            JdbcValueType::Json => Self::Json,
            JdbcValueType::Uuid => Self::Uuid,
            JdbcValueType::Opaque => Self::Opaque,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnNullability {
    Unknown,
    NoNulls,
    Nullable,
}

impl ColumnNullability {
    fn from_wire(value: i32) -> Result<Self, String> {
        match wire::ColumnNullability::try_from(value) {
            Ok(wire::ColumnNullability::Unknown) => Ok(Self::Unknown),
            Ok(wire::ColumnNullability::NoNulls) => Ok(Self::NoNulls),
            Ok(wire::ColumnNullability::Nullable) => Ok(Self::Nullable),
            Err(_) => Err(format!("unknown column nullability {value}")),
        }
    }
}

impl From<ColumnNullability> for wire::ColumnNullability {
    fn from(value: ColumnNullability) -> Self {
        match value {
            ColumnNullability::Unknown => Self::Unknown,
            ColumnNullability::NoNulls => Self::NoNulls,
            ColumnNullability::Nullable => Self::Nullable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JdbcColumn {
    pub ordinal: u32,
    pub label: String,
    pub name: String,
    pub jdbc_type: i32,
    pub jdbc_type_name: String,
    pub value_type: JdbcValueType,
    pub nullability: ColumnNullability,
    pub precision: Option<u32>,
    pub scale: Option<i32>,
    pub display_size: Option<u32>,
    pub signed: Option<bool>,
    pub catalog_name: Option<String>,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
}

impl JdbcColumn {
    fn from_wire(column: wire::JdbcColumn) -> Result<Self, String> {
        Ok(Self {
            ordinal: column.ordinal,
            label: column.label,
            name: column.name,
            jdbc_type: column.jdbc_type,
            jdbc_type_name: column.jdbc_type_name,
            value_type: JdbcValueType::from_wire(column.value_type)?,
            nullability: ColumnNullability::from_wire(column.nullability)?,
            precision: column.precision,
            scale: column.scale,
            display_size: column.display_size,
            signed: column.signed,
            catalog_name: column.catalog_name,
            schema_name: column.schema_name,
            table_name: column.table_name,
        })
    }
}

impl From<JdbcColumn> for wire::JdbcColumn {
    fn from(column: JdbcColumn) -> Self {
        Self {
            ordinal: column.ordinal,
            label: column.label,
            name: column.name,
            jdbc_type: column.jdbc_type,
            jdbc_type_name: column.jdbc_type_name,
            value_type: wire::JdbcValueType::from(column.value_type) as i32,
            nullability: wire::ColumnNullability::from(column.nullability) as i32,
            precision: column.precision,
            scale: column.scale,
            display_size: column.display_size,
            signed: column.signed,
            catalog_name: column.catalog_name,
            schema_name: column.schema_name,
            table_name: column.table_name,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JdbcRow {
    pub values: Vec<JdbcValue>,
}

impl JdbcRow {
    fn from_wire(row: wire::JdbcRow) -> Result<Self, String> {
        Ok(Self {
            values: row
                .values
                .into_iter()
                .map(JdbcValue::from_wire)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl From<JdbcRow> for wire::JdbcRow {
    fn from(row: JdbcRow) -> Self {
        Self {
            values: row.values.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryOptions {
    pub max_rows: u64,
    pub target_batch_rows: u32,
    pub target_batch_bytes: u32,
    pub initial_batch_credits: u32,
    pub max_result_bytes: u64,
}

impl QueryOptions {
    fn validate(self) -> Result<(), BridgeError> {
        if self.target_batch_rows > MAX_BATCH_ROWS {
            return Err(invalid_request(format!(
                "target_batch_rows must be zero or at most {MAX_BATCH_ROWS}"
            )));
        }
        if self.target_batch_bytes != 0
            && !(1024..=MAX_BATCH_BYTES).contains(&self.target_batch_bytes)
        {
            return Err(invalid_request(format!(
                "target_batch_bytes must be zero or between 1024 and {MAX_BATCH_BYTES}"
            )));
        }
        if self.initial_batch_credits > MAX_CREDIT_GRANT {
            return Err(invalid_request(format!(
                "initial query credits cannot exceed {MAX_CREDIT_GRANT}"
            )));
        }
        if self.max_result_bytes > MAX_RESULT_BYTES {
            return Err(invalid_request(format!(
                "max_result_bytes cannot exceed {MAX_RESULT_BYTES}"
            )));
        }
        Ok(())
    }
}

impl From<QueryOptions> for wire::QueryOptions {
    fn from(options: QueryOptions) -> Self {
        Self {
            max_rows: options.max_rows,
            target_batch_rows: options.target_batch_rows,
            target_batch_bytes: options.target_batch_bytes,
            initial_batch_credits: options.initial_batch_credits,
            max_result_bytes: options.max_result_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryRequest {
    pub sql: String,
    pub parameters: Vec<JdbcParameter>,
    pub transaction_id: Option<String>,
    pub options: QueryOptions,
}

impl QueryRequest {
    fn validate(&self) -> Result<(), BridgeError> {
        validate_statement(&self.sql, &self.parameters, self.transaction_id.as_deref())?;
        self.options.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateRequest {
    pub sql: String,
    pub parameters: Vec<JdbcParameter>,
    pub transaction_id: Option<String>,
}

impl UpdateRequest {
    fn validate(&self) -> Result<(), BridgeError> {
        validate_statement(&self.sql, &self.parameters, self.transaction_id.as_deref())
    }
}

fn validate_statement(
    sql: &str,
    parameters: &[JdbcParameter],
    transaction_id: Option<&str>,
) -> Result<(), BridgeError> {
    validate_non_blank_utf8(sql, MAX_SQL_BYTES, "sql")?;
    validate_count(parameters.len(), MAX_PARAMETERS, "parameters")?;
    if let Some(transaction_id) = transaction_id {
        validate_protocol_id(transaction_id, "transaction_id")?;
    }

    let mut positions = HashSet::with_capacity(parameters.len());
    for parameter in parameters {
        parameter.validate()?;
        if !positions.insert(parameter.position) {
            return Err(invalid_request("parameter positions must be unique"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryStarted {
    pub columns: Vec<JdbcColumn>,
}

impl From<QueryStarted> for wire::QueryStarted {
    fn from(started: QueryStarted) -> Self {
        Self {
            columns: started.columns.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowBatch {
    pub start_row_offset: u64,
    pub rows: Vec<JdbcRow>,
}

impl From<RowBatch> for wire::RowBatch {
    fn from(batch: RowBatch) -> Self {
        Self {
            start_row_offset: batch.start_row_offset,
            rows: batch.rows.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryCompleted {
    pub row_count: u64,
    pub truncated_by_max_rows: bool,
    pub truncated_by_max_result_bytes: bool,
}

impl From<QueryCompleted> for wire::QueryCompleted {
    fn from(completed: QueryCompleted) -> Self {
        Self {
            row_count: completed.row_count,
            truncated_by_max_rows: completed.truncated_by_max_rows,
            truncated_by_max_result_bytes: completed.truncated_by_max_result_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum QueryEvent {
    Started(QueryStarted),
    Batch(RowBatch),
    Completed(QueryCompleted),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateResult {
    pub affected_rows: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelDisposition {
    Accepted,
    AlreadyTerminal,
    UnknownRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    id: String,
    isolation: TransactionIsolation,
    read_only: bool,
    session_id: String,
    binding: EngineBinding,
}

impl Transaction {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn isolation(&self) -> TransactionIsolation {
        self.isolation
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }
}

#[derive(Clone)]
pub struct DriverClient {
    client: EngineClient,
    binding: EngineBinding,
}

impl DriverClient {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.binding.generation
    }

    #[must_use]
    pub fn engine_instance_id(&self) -> &str {
        &self.binding.engine_instance_id
    }

    /// Loads one driver from pre-hashed external JARs.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid artifact set, a stale engine binding,
    /// transport failure, remote rejection, or invalid engine response.
    pub async fn load_driver(&self, driver: DriverSpec) -> Result<LoadedDriver, BridgeError> {
        driver.validate()?;
        let requested_class = driver.driver_class.clone();
        let expected_driver_id = derive_driver_id(&requested_class, &driver.artifacts);
        let expected_artifact_count = u32::try_from(driver.artifacts.len()).map_err(|_| {
            BridgeError::InvalidRequest(
                "driver artifact count does not fit the protocol".to_owned(),
            )
        })?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                DRIVER_EXTERNAL_JAR_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::LoadDriver(wire::LoadDriverRequest {
                    driver_class: driver.driver_class,
                    artifacts: driver
                        .artifacts
                        .iter()
                        .map(DriverArtifact::to_wire)
                        .collect(),
                }),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::DriverLoaded(loaded)) = response.payload else {
            return self
                .client
                .protocol_violation("expected driver-loaded response")
                .await;
        };
        if loaded.driver_id != expected_driver_id
            || loaded.driver_class != requested_class
            || loaded.artifact_count != expected_artifact_count
        {
            return self
                .client
                .protocol_violation("driver-loaded response did not match the request")
                .await;
        }
        Ok(LoadedDriver {
            driver_id: loaded.driver_id,
            driver_class: loaded.driver_class,
            artifact_count: loaded.artifact_count,
        })
    }

    /// Unloads a previously loaded driver.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding is stale, the driver is in use or
    /// unknown, transport fails, or the engine response is invalid.
    pub async fn unload_driver(&self, driver_id: impl Into<String>) -> Result<(), BridgeError> {
        let driver_id = driver_id.into();
        validate_protocol_id(&driver_id, "driver_id")?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                DRIVER_EXTERNAL_JAR_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::UnloadDriver(wire::UnloadDriverRequest {
                    driver_id: driver_id.clone(),
                }),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::DriverUnloaded(unloaded)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected driver-unloaded response")
                .await;
        };
        if unloaded.driver_id != driver_id {
            return self
                .client
                .protocol_violation("driver-unloaded response did not match the request")
                .await;
        }
        Ok(())
    }

    /// Opens a JDBC session bound to this exact engine generation.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid connection settings, a stale binding,
    /// transport failure, remote JDBC failure, or invalid engine response.
    pub async fn open_session(&self, config: SessionConfig) -> Result<Session, BridgeError> {
        config.validate()?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                SESSION_JDBC_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::OpenSession(wire::OpenSessionRequest {
                    driver_id: config.driver_id,
                    jdbc_url: config.jdbc_url,
                    properties: config
                        .properties
                        .into_iter()
                        .map(|property| wire::ConnectionProperty {
                            key: property.key,
                            value: property.value,
                            sensitive: property.sensitive,
                        })
                        .collect(),
                    read_only: config.read_only,
                }),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::SessionOpened(opened)) = response.payload else {
            return self
                .client
                .protocol_violation("expected session-opened response")
                .await;
        };
        if opened.session_id.is_empty() {
            return self
                .client
                .protocol_violation("session-opened response used an empty session id")
                .await;
        }
        let Some(database) = opened.database else {
            return self
                .client
                .protocol_violation("session-opened response omitted database metadata")
                .await;
        };
        let session_state = match SessionState::from_wire(opened.session_state) {
            Ok(state) => state,
            Err(message) => return self.client.protocol_violation(message).await,
        };
        if session_state != SessionState::AutoCommit {
            return self
                .client
                .protocol_violation("new JDBC session did not enter auto-commit state")
                .await;
        }
        Ok(Session {
            client: self.client.clone(),
            binding: self.binding.clone(),
            id: opened.session_id,
            database: DatabaseProduct {
                name: database.name,
                version: database.version,
                driver_name: database.driver_name,
                driver_version: database.driver_version,
            },
            read_only: opened.read_only,
            state: Arc::new(SessionStateCell::new(session_state)),
        })
    }
}

#[derive(Clone)]
pub struct Session {
    client: EngineClient,
    binding: EngineBinding,
    id: String,
    database: DatabaseProduct,
    read_only: bool,
    state: Arc<SessionStateCell>,
}

impl Session {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn database(&self) -> &DatabaseProduct {
        &self.database
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.binding.generation
    }

    #[must_use]
    pub fn engine_instance_id(&self) -> &str {
        &self.binding.engine_instance_id
    }

    #[allow(clippy::unused_async)]
    pub async fn state(&self) -> SessionState {
        self.state.get()
    }

    /// Closes the session, rolling back any active local transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is stale or unavailable, closing the
    /// JDBC connection fails, or the engine response is invalid.
    pub async fn close(&self) -> Result<(), BridgeError> {
        let response = self
            .send_session_request(
                SESSION_JDBC_CAPABILITY,
                wire::client_envelope::Payload::CloseSession(wire::CloseSessionRequest {}),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::SessionClosed(closed)) = response.payload else {
            return self
                .client
                .protocol_violation("expected session-closed response")
                .await;
        };
        let state = match SessionState::from_wire(closed.session_state) {
            Ok(state) => state,
            Err(message) => return self.client.protocol_violation(message).await,
        };
        if state != SessionState::Closed {
            return self
                .client
                .protocol_violation("session-closed response did not enter closed state")
                .await;
        }
        Ok(())
    }

    /// Begins one local JDBC transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when the session cannot begin a transaction, transport
    /// fails, or the engine response is invalid.
    pub async fn begin_transaction(
        &self,
        options: TransactionOptions,
    ) -> Result<Transaction, BridgeError> {
        let response = self
            .send_session_request(
                TRANSACTION_LOCAL_CAPABILITY,
                wire::client_envelope::Payload::BeginTransaction(wire::BeginTransactionRequest {
                    isolation: options.isolation.to_wire() as i32,
                    read_only: options.read_only,
                }),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::TransactionStarted(started)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected transaction-started response")
                .await;
        };
        if started.transaction_id.is_empty() {
            return self
                .client
                .protocol_violation("transaction-started response used an empty transaction id")
                .await;
        }
        let isolation = match TransactionIsolation::from_wire(started.isolation) {
            Ok(isolation) => isolation,
            Err(message) => return self.client.protocol_violation(message).await,
        };
        let state = match SessionState::from_wire(started.session_state) {
            Ok(state) => state,
            Err(message) => return self.client.protocol_violation(message).await,
        };
        if state != SessionState::TransactionActive {
            return self
                .client
                .protocol_violation(
                    "transaction-started response did not enter transaction-active state",
                )
                .await;
        }
        Ok(Transaction {
            id: started.transaction_id,
            isolation,
            read_only: started.read_only,
            session_id: self.id.clone(),
            binding: self.binding.clone(),
        })
    }

    /// Commits a transaction that belongs to this session and engine instance.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale transaction handle, an unknown commit
    /// outcome, transport failure, or invalid engine response.
    pub async fn commit_transaction(&self, transaction: &Transaction) -> Result<(), BridgeError> {
        self.validate_transaction(transaction)?;
        let response = self
            .send_session_request(
                TRANSACTION_LOCAL_CAPABILITY,
                wire::client_envelope::Payload::CommitTransaction(wire::CommitTransactionRequest {
                    transaction_id: transaction.id.clone(),
                }),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::TransactionCommitted(committed)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected transaction-committed response")
                .await;
        };
        if committed.transaction_id != transaction.id {
            return self
                .client
                .protocol_violation("transaction-committed response did not match the request")
                .await;
        }
        self.apply_transaction_terminal_state(committed.session_state)
            .await?;
        Ok(())
    }

    /// Rolls back a transaction that belongs to this session and engine instance.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale transaction handle, JDBC rollback failure,
    /// transport failure, or invalid engine response.
    pub async fn rollback_transaction(&self, transaction: &Transaction) -> Result<(), BridgeError> {
        self.validate_transaction(transaction)?;
        let response = self
            .send_session_request(
                TRANSACTION_LOCAL_CAPABILITY,
                wire::client_envelope::Payload::RollbackTransaction(
                    wire::RollbackTransactionRequest {
                        transaction_id: transaction.id.clone(),
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::TransactionRolledBack(rolled_back)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected transaction-rolled-back response")
                .await;
        };
        if rolled_back.transaction_id != transaction.id {
            return self
                .client
                .protocol_violation("transaction-rolled-back response did not match the request")
                .await;
        }
        self.apply_transaction_terminal_state(rolled_back.session_state)
            .await?;
        Ok(())
    }

    /// Executes one JDBC update without retrying unknown outcomes.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid parameters, session or transaction state,
    /// JDBC failure, transport failure, or invalid engine response.
    pub async fn execute_update(
        &self,
        request: UpdateRequest,
    ) -> Result<UpdateResult, BridgeError> {
        request.validate()?;
        let parameters = request
            .parameters
            .iter()
            .map(JdbcParameter::to_wire)
            .collect::<Result<Vec<_>, _>>()?;
        let response = self
            .send_session_request(
                UPDATE_JDBC_CAPABILITY,
                wire::client_envelope::Payload::ExecuteUpdate(wire::ExecuteUpdateRequest {
                    sql: request.sql,
                    parameters,
                    transaction_id: request.transaction_id,
                }),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::UpdateCompleted(completed)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected update-completed response")
                .await;
        };
        Ok(UpdateResult {
            affected_rows: completed.affected_rows,
        })
    }

    /// Starts a credit-controlled typed JDBC query stream.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits or parameters, session state,
    /// transport failure, remote JDBC failure, or invalid engine response.
    pub async fn execute_query(&self, request: QueryRequest) -> Result<QueryStream, BridgeError> {
        request.validate()?;
        validate_protocol_id(&self.id, "session_id")?;
        let parameters = request
            .parameters
            .iter()
            .map(JdbcParameter::to_wire)
            .collect::<Result<Vec<_>, _>>()?;
        self.client
            .start_bound_query(
                &self.binding,
                &self.id,
                wire::ExecuteQueryRequest {
                    sql: request.sql,
                    parameters,
                    transaction_id: request.transaction_id,
                    options: Some(request.options.into()),
                },
                request.options.initial_batch_credits,
                self.state.clone(),
            )
            .await
    }

    fn validate_transaction(&self, transaction: &Transaction) -> Result<(), BridgeError> {
        if transaction.binding != self.binding || transaction.session_id != self.id {
            return Err(BridgeError::StaleHandle(
                "transaction does not belong to this session".to_owned(),
            ));
        }
        validate_protocol_id(&transaction.id, "transaction_id")?;
        Ok(())
    }

    async fn send_session_request(
        &self,
        capability: &str,
        payload: wire::client_envelope::Payload,
        lane: PendingLane,
    ) -> Result<wire::ServerEnvelope, BridgeError> {
        validate_protocol_id(&self.id, "session_id")?;
        self.client
            .send_bound_request(
                &self.binding,
                capability,
                Some(&self.id),
                Some(self.state.clone()),
                payload,
                lane,
            )
            .await
    }

    async fn apply_transaction_terminal_state(&self, value: i32) -> Result<(), BridgeError> {
        let state = match SessionState::from_wire(value) {
            Ok(state) => state,
            Err(message) => return self.client.protocol_violation(message).await,
        };
        if state != SessionState::AutoCommit {
            return self
                .client
                .protocol_violation("completed transaction did not return to auto-commit state")
                .await;
        }
        Ok(())
    }
}

pub struct QueryStream {
    client: EngineClient,
    binding: EngineBinding,
    session_id: String,
    request_id: String,
    events: mpsc::Receiver<Result<QueryEvent, PendingFailure>>,
    session_state: Arc<SessionStateCell>,
    request_cancelled: Arc<AtomicBool>,
    terminal: bool,
}

impl QueryStream {
    pub(super) fn new(
        client: EngineClient,
        binding: EngineBinding,
        session_id: String,
        request_id: String,
        events: mpsc::Receiver<Result<QueryEvent, PendingFailure>>,
        session_state: Arc<SessionStateCell>,
        request_cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            client,
            binding,
            session_id,
            request_id,
            events,
            session_state,
            request_cancelled,
            terminal: false,
        }
    }

    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.binding.generation
    }

    #[must_use]
    pub fn engine_instance_id(&self) -> &str {
        &self.binding.engine_instance_id
    }

    /// Receives the next validated query event.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine rejects the query, the stream times
    /// out, transport fails, or the stream violates the protocol.
    pub async fn next_event(&mut self) -> Result<Option<QueryEvent>, BridgeError> {
        match self.events.recv().await {
            Some(Ok(event)) => {
                if matches!(event, QueryEvent::Completed(_)) {
                    self.terminal = true;
                }
                Ok(Some(event))
            }
            Some(Err(error)) => {
                self.terminal = true;
                Err(error.into_bridge_error())
            }
            None => {
                self.terminal = true;
                Ok(None)
            }
        }
    }

    /// Grants additional row-batch credits to this query.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid or overflowing credit grant, a terminal
    /// stream, transport failure, remote rejection, or invalid response.
    pub async fn grant_credits(&self, batch_credits: u32) -> Result<u32, BridgeError> {
        if self.terminal {
            return Err(BridgeError::InvalidRequest(
                "cannot grant credits to a terminal query stream".to_owned(),
            ));
        }
        if batch_credits == 0 || batch_credits > MAX_CREDIT_GRANT {
            return Err(BridgeError::InvalidRequest(format!(
                "credit grant must be between 1 and {MAX_CREDIT_GRANT}"
            )));
        }
        validate_protocol_id(&self.session_id, "session_id")?;
        validate_protocol_id(&self.request_id, "target_request_id")?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                FLOW_CREDIT_CAPABILITY,
                Some(&self.session_id),
                Some(self.session_state.clone()),
                wire::client_envelope::Payload::GrantCredits(wire::GrantCreditsRequest {
                    target_request_id: self.request_id.clone(),
                    batch_credits,
                }),
                PendingLane::Control(ControlEffect::GrantCredits {
                    target_request_id: self.request_id.clone(),
                    batch_credits,
                }),
            )
            .await?;
        let Some(wire::server_envelope::Payload::CreditsGranted(granted)) = response.payload else {
            return self
                .client
                .protocol_violation("expected credits-granted response")
                .await;
        };
        Ok(granted.accepted_batch_credits)
    }

    pub(super) fn disarm(&mut self) {
        self.terminal = true;
        self.request_cancelled.store(true, Ordering::Release);
    }

    /// Requests cancellation without assuming that JDBC has already stopped.
    ///
    /// # Errors
    ///
    /// Returns an error when cancellation cannot be sent, the engine rejects
    /// it, or the response violates the protocol.
    pub async fn cancel(&self, reason: Option<String>) -> Result<CancelDisposition, BridgeError> {
        if self.terminal {
            self.client
                .ensure_bound_capability(&self.binding, OPERATION_CANCEL_CAPABILITY)?;
            return Ok(CancelDisposition::AlreadyTerminal);
        }
        validate_protocol_id(&self.session_id, "session_id")?;
        validate_protocol_id(&self.request_id, "target_request_id")?;
        if let Some(reason) = reason.as_deref() {
            validate_utf8(reason, MAX_SCALAR_BYTES, "cancellation reason")?;
        }
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                OPERATION_CANCEL_CAPABILITY,
                Some(&self.session_id),
                Some(self.session_state.clone()),
                wire::client_envelope::Payload::CancelOperation(wire::CancelOperationRequest {
                    target_request_id: self.request_id.clone(),
                    reason,
                }),
                PendingLane::Control(ControlEffect::Cancel {
                    target_request_id: self.request_id.clone(),
                }),
            )
            .await?;
        let Some(wire::server_envelope::Payload::OperationCancelled(cancelled)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected operation-cancelled response")
                .await;
        };
        match wire::CancelDisposition::try_from(cancelled.disposition) {
            Ok(wire::CancelDisposition::Accepted) => Ok(CancelDisposition::Accepted),
            Ok(wire::CancelDisposition::AlreadyTerminal) => Ok(CancelDisposition::AlreadyTerminal),
            Ok(wire::CancelDisposition::UnknownRequest) => Ok(CancelDisposition::UnknownRequest),
            Ok(wire::CancelDisposition::Unspecified) | Err(_) => {
                self.client
                    .protocol_violation("operation-cancelled response used an invalid disposition")
                    .await
            }
        }
    }
}

impl Drop for QueryStream {
    fn drop(&mut self) {
        if !self.terminal {
            self.terminal = true;
            self.client.best_effort_abandon_query(
                &self.binding,
                &self.session_id,
                &self.request_id,
                &self.request_cancelled,
                &self.session_state,
            );
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EngineBinding {
    pub(super) generation: u64,
    pub(super) engine_instance_id: String,
}

pub(super) fn query_event_from_payload(
    payload: wire::server_envelope::Payload,
) -> Result<QueryEvent, String> {
    match payload {
        wire::server_envelope::Payload::QueryStarted(started) => {
            Ok(QueryEvent::Started(QueryStarted {
                columns: started
                    .columns
                    .into_iter()
                    .map(JdbcColumn::from_wire)
                    .collect::<Result<_, _>>()?,
            }))
        }
        wire::server_envelope::Payload::RowBatch(batch) => Ok(QueryEvent::Batch(RowBatch {
            start_row_offset: batch.start_row_offset,
            rows: batch
                .rows
                .into_iter()
                .map(JdbcRow::from_wire)
                .collect::<Result<_, _>>()?,
        })),
        wire::server_envelope::Payload::QueryCompleted(completed) => {
            Ok(QueryEvent::Completed(QueryCompleted {
                row_count: completed.row_count,
                truncated_by_max_rows: completed.truncated_by_max_rows,
                truncated_by_max_result_bytes: completed.truncated_by_max_result_bytes,
            }))
        }
        _ => Err("query stream used an unexpected response payload".to_owned()),
    }
}

impl EngineClient {
    /// Creates a JDBC driver handle bound to the current ready generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine is not ready or has no validated
    /// identity for the current generation.
    pub fn driver_client(&self) -> Result<DriverClient, BridgeError> {
        Ok(DriverClient {
            client: self.clone(),
            binding: self.capture_binding()?,
        })
    }
}

fn derive_driver_id(driver_class: &str, artifacts: &[DriverArtifact]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DRIVER_ID_DOMAIN_SEPARATOR);
    hasher.update(driver_class.as_bytes());
    hasher.update([0]);
    for artifact in artifacts {
        hasher.update(artifact.sha256);
    }

    let digest = hasher.finalize();
    let mut driver_id = String::with_capacity("sha256:".len() + digest.len() * 2);
    driver_id.push_str("sha256:");
    for byte in digest {
        write!(&mut driver_id, "{byte:02x}").expect("writing to String cannot fail");
    }
    driver_id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(path: impl Into<PathBuf>) -> DriverArtifact {
        DriverArtifact {
            canonical_path: path.into(),
            sha256: [7; 32],
        }
    }

    fn null_parameter(position: u32) -> JdbcParameter {
        JdbcParameter {
            position,
            value: JdbcValue::Null,
            jdbc_type: None,
            jdbc_type_name: None,
        }
    }

    fn assert_invalid(result: Result<(), BridgeError>, expected: &str) {
        match result {
            Err(BridgeError::InvalidRequest(message)) => assert!(
                message.contains(expected),
                "expected {message:?} to contain {expected:?}"
            ),
            other => panic!("expected invalid request containing {expected:?}, got {other:?}"),
        }
    }

    #[test]
    fn retained_result_wire_conversion_preserves_schema_rows_and_completion() {
        let started = QueryStarted {
            columns: vec![JdbcColumn {
                ordinal: 1,
                label: "payload".to_owned(),
                name: "payload".to_owned(),
                jdbc_type: 1111,
                jdbc_type_name: "OTHER".to_owned(),
                value_type: JdbcValueType::Opaque,
                nullability: ColumnNullability::Nullable,
                precision: Some(38),
                scale: Some(4),
                display_size: Some(128),
                signed: Some(false),
                catalog_name: Some("catalog".to_owned()),
                schema_name: Some("public".to_owned()),
                table_name: Some("events".to_owned()),
            }],
        };
        let batch = RowBatch {
            start_row_offset: 4,
            rows: vec![JdbcRow {
                values: vec![JdbcValue::Opaque {
                    type_name: "vendor_type".to_owned(),
                    display_value: "value".to_owned(),
                }],
            }],
        };
        let completed = QueryCompleted {
            row_count: 5,
            truncated_by_max_rows: true,
            truncated_by_max_result_bytes: false,
        };

        let wire_started = wire::QueryStarted::from(started);
        let wire_batch = wire::RowBatch::from(batch);
        let wire_completed = wire::QueryCompleted::from(completed);
        assert_eq!(
            wire_started.columns[0].value_type,
            wire::JdbcValueType::Opaque as i32
        );
        assert_eq!(wire_batch.start_row_offset, 4);
        assert_eq!(wire_started.columns[0].precision, Some(38));
        assert_eq!(wire_started.columns[0].scale, Some(4));
        assert_eq!(wire_started.columns[0].signed, Some(false));
        assert!(matches!(
            wire_batch.rows[0].values[0].value,
            Some(wire::jdbc_value::Value::OpaqueValue(_))
        ));
        assert_eq!(wire_completed.row_count, 5);
        assert!(wire_completed.truncated_by_max_rows);
    }

    #[test]
    fn retained_result_wire_conversion_preserves_every_jdbc_value_variant() {
        let values = vec![
            JdbcValue::Null,
            JdbcValue::Boolean(true),
            JdbcValue::SignedInteger(-7),
            JdbcValue::UnsignedInteger(9),
            JdbcValue::Float32(1.25),
            JdbcValue::Float64(-2.5),
            JdbcValue::Decimal("123.450".to_owned()),
            JdbcValue::Text("text".to_owned()),
            JdbcValue::Binary(vec![0, 1, 255]),
            JdbcValue::Date("2026-07-24".to_owned()),
            JdbcValue::Time("12:34:56".to_owned()),
            JdbcValue::Timestamp("2026-07-24T12:34:56".to_owned()),
            JdbcValue::TimestampWithTimeZone("2026-07-24T12:34:56+08:00".to_owned()),
            JdbcValue::Json("{\"ok\":true}".to_owned()),
            JdbcValue::Uuid("550e8400-e29b-41d4-a716-446655440000".to_owned()),
            JdbcValue::Opaque {
                type_name: "vendor_type".to_owned(),
                display_value: "opaque".to_owned(),
            },
        ];

        for value in values {
            let decoded = JdbcValue::from_wire(wire::JdbcValue::from(value.clone()))
                .expect("converted value decodes");
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn string_limits_count_utf8_bytes() {
        let oversized = "界".repeat(MAX_DRIVER_ID_BYTES / 3 + 1);
        assert!(oversized.chars().count() < MAX_DRIVER_ID_BYTES);

        assert_invalid(validate_protocol_id(&oversized, "driver_id"), "UTF-8 bytes");
        assert_invalid(
            validate_protocol_id(" \t\n", "driver_id"),
            "driver_id is required",
        );
    }

    #[test]
    fn driver_validation_enforces_class_artifact_and_path_limits() {
        let valid_artifact = artifact("/tmp/chat2db-driver.jar");
        DriverSpec {
            driver_class: "org.example.Driver".to_owned(),
            artifacts: vec![valid_artifact.clone()],
        }
        .validate()
        .expect("valid driver spec must pass");

        assert_invalid(
            DriverSpec {
                driver_class: " ".to_owned(),
                artifacts: vec![valid_artifact.clone()],
            }
            .validate(),
            "driver_class is required",
        );
        assert_invalid(
            DriverSpec {
                driver_class: "org.example.Driver".to_owned(),
                artifacts: Vec::new(),
            }
            .validate(),
            "at least one",
        );
        assert_invalid(
            DriverSpec {
                driver_class: "org.example.Driver".to_owned(),
                artifacts: vec![valid_artifact; MAX_DRIVER_ARTIFACTS + 1],
            }
            .validate(),
            "driver artifacts",
        );
        assert_invalid(
            artifact(format!("/{}", "x".repeat(MAX_PATH_BYTES))).validate(),
            "driver artifact path",
        );
        assert_invalid(
            artifact("relative-driver.jar").validate(),
            "must be absolute",
        );
    }

    #[test]
    fn session_validation_enforces_ids_urls_and_properties() {
        let property = ConnectionProperty {
            key: "user".to_owned(),
            value: "chat2db".to_owned(),
            sensitive: false,
        };
        SessionConfig {
            driver_id: "sha256:driver".to_owned(),
            jdbc_url: "jdbc:test:memory".to_owned(),
            properties: vec![property.clone()],
            read_only: false,
        }
        .validate()
        .expect("valid session config must pass");

        assert_invalid(
            SessionConfig {
                driver_id: "x".repeat(MAX_DRIVER_ID_BYTES + 1),
                jdbc_url: "jdbc:test:memory".to_owned(),
                properties: Vec::new(),
                read_only: false,
            }
            .validate(),
            "driver_id",
        );
        assert_invalid(
            SessionConfig {
                driver_id: "driver".to_owned(),
                jdbc_url: "x".repeat(MAX_JDBC_URL_BYTES + 1),
                properties: Vec::new(),
                read_only: false,
            }
            .validate(),
            "jdbc_url",
        );
        assert_invalid(
            SessionConfig {
                driver_id: "driver".to_owned(),
                jdbc_url: "jdbc:test:memory".to_owned(),
                properties: vec![property.clone(); MAX_CONNECTION_PROPERTIES + 1],
                read_only: false,
            }
            .validate(),
            "connection properties",
        );
        assert_invalid(
            SessionConfig {
                driver_id: "driver".to_owned(),
                jdbc_url: "jdbc:test:memory".to_owned(),
                properties: vec![property.clone(), property],
                read_only: false,
            }
            .validate(),
            "must be unique",
        );
        assert_invalid(
            SessionConfig {
                driver_id: "driver".to_owned(),
                jdbc_url: "jdbc:test:memory".to_owned(),
                properties: vec![ConnectionProperty {
                    key: "key".to_owned(),
                    value: "x".repeat(MAX_PROPERTY_VALUE_BYTES + 1),
                    sensitive: true,
                }],
                read_only: false,
            }
            .validate(),
            "property value",
        );
    }

    #[test]
    fn statement_validation_enforces_sql_parameters_and_scalar_limits() {
        validate_statement("select ?", &[null_parameter(1)], Some("transaction-1"))
            .expect("valid statement must pass");

        assert_invalid(validate_statement(" \n", &[], None), "sql is required");
        assert_invalid(
            validate_statement(&"x".repeat(MAX_SQL_BYTES + 1), &[], None),
            "sql",
        );
        assert_invalid(
            validate_statement(
                "select 1",
                &vec![null_parameter(1); MAX_PARAMETERS + 1],
                None,
            ),
            "parameters",
        );
        assert_invalid(
            validate_statement("select ?", &[null_parameter(0)], None),
            "parameter position",
        );
        assert_invalid(
            validate_statement("select ?, ?", &[null_parameter(1), null_parameter(1)], None),
            "positions must be unique",
        );
        assert_invalid(
            validate_statement(
                "select ?",
                &[JdbcParameter {
                    position: 1,
                    value: JdbcValue::Text("x".repeat(MAX_SCALAR_BYTES + 1)),
                    jdbc_type: None,
                    jdbc_type_name: None,
                }],
                None,
            ),
            "text value",
        );
        assert_invalid(
            validate_statement("select 1", &[], Some(&"x".repeat(MAX_DRIVER_ID_BYTES + 1))),
            "transaction_id",
        );
    }

    #[test]
    fn query_options_enforce_batch_credit_and_result_limits() {
        QueryOptions {
            max_rows: u64::MAX,
            target_batch_rows: MAX_BATCH_ROWS,
            target_batch_bytes: MAX_BATCH_BYTES,
            initial_batch_credits: MAX_CREDIT_GRANT,
            max_result_bytes: MAX_RESULT_BYTES,
        }
        .validate()
        .expect("hard-limit boundary values must pass");
        QueryOptions::default()
            .validate()
            .expect("zero-valued engine defaults must pass");

        assert_invalid(
            QueryOptions {
                target_batch_rows: MAX_BATCH_ROWS + 1,
                ..QueryOptions::default()
            }
            .validate(),
            "target_batch_rows",
        );
        assert_invalid(
            QueryOptions {
                target_batch_bytes: 1,
                ..QueryOptions::default()
            }
            .validate(),
            "target_batch_bytes",
        );
        assert_invalid(
            QueryOptions {
                target_batch_bytes: MAX_BATCH_BYTES + 1,
                ..QueryOptions::default()
            }
            .validate(),
            "target_batch_bytes",
        );
        assert_invalid(
            QueryOptions {
                initial_batch_credits: MAX_CREDIT_GRANT + 1,
                ..QueryOptions::default()
            }
            .validate(),
            "initial query credits",
        );
        assert_invalid(
            QueryOptions {
                max_result_bytes: MAX_RESULT_BYTES + 1,
                ..QueryOptions::default()
            }
            .validate(),
            "max_result_bytes",
        );
    }
}
