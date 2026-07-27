use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use chat2db_engine_protocol::wire;
use prost::Message;
use sha2::{Digest, Sha256};

use crate::BridgeError;

use super::{
    EngineClient,
    jdbc::{EngineBinding, Session, driver_artifact_wire_path},
    pending::PendingLane,
};

/// Lists the Community database plugins available to the compatibility engine.
pub const COMMUNITY_PLUGIN_CATALOG_CAPABILITY: &str = "community.plugin-catalog.v1";
/// Reads database metadata through a Community `IDbMetaData` implementation.
pub const COMMUNITY_SCHEMA_METADATA_CAPABILITY: &str = "community.metadata.schemas.v1";
/// Reads databases, tables, columns, and indexes through Community metadata.
pub const COMMUNITY_OBJECT_METADATA_CAPABILITY: &str = "community.metadata.objects.v1";
/// Reads views and relational key metadata through Community metadata.
pub const COMMUNITY_RELATION_METADATA_CAPABILITY: &str = "community.metadata.relations.v1";
/// Reads functions, procedures, triggers, and routine parameters through Community metadata.
pub const COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY: &str =
    "community.metadata.programmability.v1";
/// Builds dialect SQL through a Community `ISqlBuilder` implementation.
pub const COMMUNITY_SQL_BUILDER_CAPABILITY: &str = "community.sql-builder.v1";
/// Parses SQL through a Community `ISqlSyntaxPlugin` implementation.
pub const COMMUNITY_SQL_PARSER_CAPABILITY: &str = "community.sql-parser.v1";
/// Validates SQL through a Community `ISqlSyntaxPlugin` implementation.
pub const COMMUNITY_SQL_VALIDATION_CAPABILITY: &str = "community.sql-validation.v1";
/// Formats SQL through the retained Community-compatible `SqlFormatter`.
pub const COMMUNITY_SQL_FORMATTER_CAPABILITY: &str = "community.sql-formatter.v1";
/// Completes SQL through the retained Community completion engines.
pub const COMMUNITY_SQL_COMPLETION_CAPABILITY: &str = "community.sql-completion.v1";
/// Builds typed dialect DML without opening a JDBC session or executing SQL.
pub const COMMUNITY_DML_BUILDER_CAPABILITY: &str = "community.dml-builder.v1";
/// Builds database and schema lifecycle SQL without executing it.
pub const COMMUNITY_NAMESPACE_BUILDER_CAPABILITY: &str = "community.namespace-builder.v1";

pub(super) const COMMUNITY_CLASSPATH_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
pub(super) const COMMUNITY_SOURCE_COMMIT_ENV: &str = "CHAT2DB_COMMUNITY_SOURCE_COMMIT";

const MAX_CLASSPATH_ARTIFACTS: usize = wire::CommunityCountLimit::MaxClasspathArtifacts as usize;
const MAX_CLASSPATH_BYTES: u64 = wire::CommunityByteLimit::MaxClasspathBytes as u64;
const MAX_DATABASE_TYPE_BYTES: usize = wire::CommunityByteLimit::MaxDatabaseTypeBytes as usize;
const MAX_SOURCE_COMMIT_BYTES: usize = wire::CommunityByteLimit::MaxSourceCommitBytes as usize;
const MAX_COMMENT_BYTES: usize = wire::CommunityByteLimit::MaxCommentBytes as usize;
const MAX_SQL_FORMATTER_COMPLEXITY_UNITS: usize =
    wire::CommunitySqlFormatterLimit::MaxComplexityUnits as usize;
const MAX_SQL_COMPLETION_PREFIX_LENGTH: u32 =
    wire::CommunitySqlCompletionPrefixLimit::MaxMinPrefixLength as u32;
const MAX_DML_COLUMNS: usize = wire::CommunityDmlCountLimit::MaxColumns as usize;
const MAX_DML_ROWS: usize = wire::CommunityDmlCountLimit::MaxRows as usize;
const MAX_DML_VALUES: usize = wire::CommunityDmlCountLimit::MaxValues as usize;
const MAX_DML_IDENTIFIER_BYTES: usize = wire::CommunityDmlByteLimit::MaxIdentifierBytes as usize;
const MAX_DML_DATA_TYPE_NAME_BYTES: usize =
    wire::CommunityDmlByteLimit::MaxDataTypeNameBytes as usize;
const MAX_DML_DECIMAL_BYTES: usize = wire::CommunityDmlByteLimit::MaxDecimalBytes as usize;
const MAX_DML_TEMPORAL_BYTES: usize = wire::CommunityDmlByteLimit::MaxTemporalBytes as usize;
const MAX_DML_VALUE_BYTES: usize = wire::CommunityDmlByteLimit::MaxValueBytes as usize;
const MAX_NAMESPACE_IDENTIFIER_BYTES: usize =
    wire::CommunityNamespaceByteLimit::MaxIdentifierBytes as usize;
const MAX_NAMESPACE_PROPERTY_BYTES: usize =
    wire::CommunityNamespaceByteLimit::MaxPropertyBytes as usize;
const MAX_COMMUNITY_RESPONSE_BYTES: usize = wire::CommunityByteLimit::MaxResponseBytes as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_PROTOCOL_ID_BYTES: usize = wire::JdbcProtocolLimit::MaxDriverIdBytes as usize;
const MAX_PATH_BYTES: usize = wire::JdbcProtocolLimit::MaxPathBytes as usize;
static NEXT_SQL_COMPLETION_DATASOURCE_SCOPE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
struct CommunityArtifact {
    canonical_path: PathBuf,
    sha256: [u8; 32],
    byte_len: u64,
}

#[derive(Debug)]
struct LockedCommunityArtifact {
    file_name: String,
    sha256: [u8; 32],
    byte_len: u64,
}

/// Validated, immutable Community compatibility classpath for one engine process.
#[derive(Clone, Debug)]
pub struct CommunityClasspath {
    source_commit: String,
    artifacts: Vec<CommunityArtifact>,
}

impl CommunityClasspath {
    /// Resolves a fixed Community build from an exact filename, length, and
    /// SHA-256 lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock is malformed, the directory contains a
    /// missing or extra entry, an entry is unsafe, or any locked artifact does
    /// not match its expected byte length and digest.
    pub fn from_locked_directory(
        directory: impl AsRef<Path>,
        lock: &str,
    ) -> Result<Self, BridgeError> {
        let directory = directory.as_ref();
        let metadata =
            fs::symlink_metadata(directory).map_err(|source| BridgeError::CommunityArtifact {
                operation: "inspect locked directory",
                path: directory.to_path_buf(),
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BridgeError::InvalidConfig(
                "Community classpath lock root must be a regular directory".to_owned(),
            ));
        }

        let (source_commit, expected) = parse_classpath_lock(lock)?;
        let expected_by_name = expected
            .iter()
            .map(|artifact| (artifact.file_name.as_str(), artifact))
            .collect::<HashMap<_, _>>();
        let mut discovered = HashSet::with_capacity(expected.len());
        for entry in fs::read_dir(directory).map_err(|source| BridgeError::CommunityArtifact {
            operation: "read locked directory",
            path: directory.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| BridgeError::CommunityArtifact {
                operation: "read locked directory entry",
                path: directory.to_path_buf(),
                source,
            })?;
            let file_name = entry.file_name().into_string().map_err(|_| {
                BridgeError::InvalidConfig(
                    "Community classpath directory entries must be valid UTF-8".to_owned(),
                )
            })?;
            if !expected_by_name.contains_key(file_name.as_str()) {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath contains unlocked entry {file_name}"
                )));
            }
            if !discovered.insert(file_name.clone()) {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath contains duplicate entry {file_name}"
                )));
            }
        }
        if discovered.len() != expected.len() {
            let missing = expected
                .iter()
                .find(|artifact| !discovered.contains(&artifact.file_name))
                .map_or("unknown", |artifact| artifact.file_name.as_str());
            return Err(BridgeError::InvalidConfig(format!(
                "Community classpath is missing locked artifact {missing}"
            )));
        }

        let requested = expected
            .iter()
            .map(|artifact| directory.join(&artifact.file_name))
            .collect::<Vec<_>>();
        let classpath = Self::from_paths(source_commit, requested)?;
        for artifact in &classpath.artifacts {
            let file_name = artifact
                .canonical_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    BridgeError::InvalidConfig(
                        "Community classpath artifact filename is not valid UTF-8".to_owned(),
                    )
                })?;
            let locked = expected_by_name.get(file_name).ok_or_else(|| {
                BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {file_name} is not locked"
                ))
            })?;
            if artifact.byte_len != locked.byte_len || artifact.sha256 != locked.sha256 {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {file_name} does not match its lock"
                )));
            }
        }
        Ok(classpath)
    }

    /// Resolves a fixed Community build and its JARs for one engine generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the commit is not a lowercase full Git SHA, a path
    /// is unsafe or not a regular JAR, the set contains duplicates, or the
    /// classpath exceeds its count or byte budget.
    pub fn from_paths(
        source_commit: impl Into<String>,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, BridgeError> {
        let source_commit = source_commit.into();
        validate_source_commit(&source_commit)?;
        let requested = paths.into_iter().collect::<Vec<_>>();
        if requested.is_empty() {
            return Err(BridgeError::InvalidConfig(
                "Community compatibility classpath cannot be empty".to_owned(),
            ));
        }
        if requested.len() > MAX_CLASSPATH_ARTIFACTS {
            return Err(BridgeError::InvalidConfig(format!(
                "Community compatibility classpath cannot contain more than {MAX_CLASSPATH_ARTIFACTS} artifacts"
            )));
        }

        let mut artifacts = Vec::with_capacity(requested.len());
        let mut identities = HashSet::with_capacity(requested.len());
        let mut total_bytes = 0_u64;
        for requested_path in requested {
            let link_metadata = std::fs::symlink_metadata(&requested_path).map_err(|error| {
                BridgeError::InvalidConfig(format!(
                    "cannot inspect Community classpath artifact {}: {error}",
                    requested_path.display()
                ))
            })?;
            if link_metadata.file_type().is_symlink() {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {} cannot be a symbolic link",
                    requested_path.display()
                )));
            }
            let canonical = std::fs::canonicalize(&requested_path).map_err(|error| {
                BridgeError::InvalidConfig(format!(
                    "cannot resolve Community classpath artifact {}: {error}",
                    requested_path.display()
                ))
            })?;
            let metadata =
                std::fs::metadata(&canonical).map_err(|source| BridgeError::CommunityArtifact {
                    operation: "inspect",
                    path: canonical.clone(),
                    source,
                })?;
            if !metadata.is_file() {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {} is not a regular file",
                    canonical.display()
                )));
            }
            if !canonical
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
            {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {} is not a JAR",
                    canonical.display()
                )));
            }
            if !identities.insert(canonical.clone()) {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {} is duplicated",
                    canonical.display()
                )));
            }
            if metadata.len() > MAX_CLASSPATH_BYTES.saturating_sub(total_bytes) {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community compatibility classpath cannot exceed {MAX_CLASSPATH_BYTES} bytes"
                )));
            }
            let (sha256, byte_len) =
                hash_regular_file(&canonical, MAX_CLASSPATH_BYTES.saturating_sub(total_bytes))?;
            total_bytes = total_bytes.checked_add(byte_len).ok_or_else(|| {
                BridgeError::InvalidConfig("Community classpath byte count overflowed".to_owned())
            })?;
            if total_bytes > MAX_CLASSPATH_BYTES {
                return Err(BridgeError::InvalidConfig(format!(
                    "Community compatibility classpath cannot exceed {MAX_CLASSPATH_BYTES} bytes"
                )));
            }
            canonical.to_str().ok_or_else(|| {
                BridgeError::InvalidConfig(format!(
                    "Community classpath artifact {} is not valid UTF-8",
                    canonical.display()
                ))
            })?;
            artifacts.push(CommunityArtifact {
                canonical_path: canonical,
                sha256,
                byte_len,
            });
        }
        artifacts.sort_by(|left, right| left.canonical_path.cmp(&right.canonical_path));
        Ok(Self {
            source_commit,
            artifacts,
        })
    }

    pub(super) fn source_commit(&self) -> &str {
        &self.source_commit
    }

    pub(super) fn snapshot_into(&self, generation_root: &Path) -> Result<String, BridgeError> {
        let snapshot_directory = generation_root.join("community-classpath");
        fs::create_dir(&snapshot_directory).map_err(|source| BridgeError::CommunityArtifact {
            operation: "create snapshot directory for",
            path: snapshot_directory.clone(),
            source,
        })?;
        for (index, artifact) in self.artifacts.iter().enumerate() {
            let snapshot_path = snapshot_directory.join(format!("artifact-{index:04}.jar"));
            snapshot_artifact(artifact, &snapshot_path)?;
        }
        let canonical = fs::canonicalize(&snapshot_directory).map_err(|source| {
            BridgeError::CommunityArtifact {
                operation: "resolve snapshot directory for",
                path: snapshot_directory,
                source,
            }
        })?;
        let canonical_text = canonical.to_str().ok_or_else(|| {
            BridgeError::InvalidConfig(format!(
                "Community classpath snapshot {} is not valid UTF-8",
                canonical.display()
            ))
        })?;
        let wire_path = driver_artifact_wire_path(canonical_text)?;
        if wire_path.len() > MAX_PATH_BYTES {
            return Err(BridgeError::InvalidConfig(format!(
                "Community classpath snapshot path cannot exceed {MAX_PATH_BYTES} UTF-8 bytes"
            )));
        }
        Ok(wire_path)
    }

    /// Returns the canonical JARs in deterministic classpath order.
    #[must_use]
    pub fn artifacts(&self) -> impl ExactSizeIterator<Item = &Path> {
        self.artifacts
            .iter()
            .map(|artifact| artifact.canonical_path.as_path())
    }
}

fn parse_classpath_lock(lock: &str) -> Result<(String, Vec<LockedCommunityArtifact>), BridgeError> {
    let mut lines = lock.lines();
    require_lock_header(lines.next(), "format_version", "1")?;
    let source_commit = lock_header_value(lines.next(), "source_commit")?.to_owned();
    validate_source_commit(&source_commit)?;
    let artifact_count = lock_header_value(lines.next(), "artifact_count")?
        .parse::<usize>()
        .map_err(|_| invalid_classpath_lock("artifact_count must be an unsigned integer"))?;
    if artifact_count == 0 || artifact_count > MAX_CLASSPATH_ARTIFACTS {
        return Err(invalid_classpath_lock(format!(
            "artifact_count must be between 1 and {MAX_CLASSPATH_ARTIFACTS}"
        )));
    }

    let mut artifacts = Vec::with_capacity(artifact_count);
    let mut names = HashSet::with_capacity(artifact_count);
    let mut previous_name: Option<String> = None;
    for line in lines {
        if line.is_empty() {
            return Err(invalid_classpath_lock("blank lines are not allowed"));
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 4 || fields[0] != "artifact" {
            return Err(invalid_classpath_lock(
                "artifact rows must contain name, SHA-256, and byte length",
            ));
        }
        let file_name = fields[1];
        if !is_safe_locked_jar_name(file_name) {
            return Err(invalid_classpath_lock(
                "artifact names must be plain UTF-8 JAR filenames",
            ));
        }
        if previous_name
            .as_deref()
            .is_some_and(|previous| previous >= file_name)
        {
            return Err(invalid_classpath_lock(
                "artifact rows must be strictly sorted by filename",
            ));
        }
        if !names.insert(file_name.to_owned()) {
            return Err(invalid_classpath_lock("artifact names must be unique"));
        }
        let byte_len = fields[3]
            .parse::<u64>()
            .map_err(|_| invalid_classpath_lock("artifact byte length must be an integer"))?;
        if byte_len == 0 {
            return Err(invalid_classpath_lock(
                "artifact byte length must be greater than zero",
            ));
        }
        artifacts.push(LockedCommunityArtifact {
            file_name: file_name.to_owned(),
            sha256: decode_lock_sha256(fields[2])?,
            byte_len,
        });
        previous_name = Some(file_name.to_owned());
    }
    if artifacts.len() != artifact_count {
        return Err(invalid_classpath_lock(format!(
            "artifact_count declared {artifact_count} entries but {} were present",
            artifacts.len()
        )));
    }
    Ok((source_commit, artifacts))
}

fn require_lock_header(
    line: Option<&str>,
    expected_key: &str,
    expected_value: &str,
) -> Result<(), BridgeError> {
    let value = lock_header_value(line, expected_key)?;
    if value == expected_value {
        Ok(())
    } else {
        Err(invalid_classpath_lock(format!(
            "{expected_key} must be {expected_value}"
        )))
    }
}

fn lock_header_value<'a>(
    line: Option<&'a str>,
    expected_key: &str,
) -> Result<&'a str, BridgeError> {
    let line =
        line.ok_or_else(|| invalid_classpath_lock(format!("missing {expected_key} header")))?;
    let mut fields = line.split('\t');
    let key = fields.next();
    let value = fields.next();
    if key != Some(expected_key) || value.is_none() || fields.next().is_some() {
        return Err(invalid_classpath_lock(format!(
            "invalid {expected_key} header"
        )));
    }
    Ok(value.expect("value checked above"))
}

fn is_safe_locked_jar_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && !file_name.contains(['/', '\\', '\0'])
        && file_name != "."
        && file_name != ".."
        && Path::new(file_name).components().count() == 1
        && Path::new(file_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
}

fn decode_lock_sha256(value: &str) -> Result<[u8; 32], BridgeError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_classpath_lock(
            "artifact SHA-256 must contain 64 lowercase hexadecimal characters",
        ));
    }
    if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(invalid_classpath_lock(
            "artifact SHA-256 must contain 64 lowercase hexadecimal characters",
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    Ok(digest)
}

const fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => 0,
    }
}

fn invalid_classpath_lock(message: impl Into<String>) -> BridgeError {
    BridgeError::InvalidConfig(format!(
        "invalid Community classpath lock: {}",
        message.into()
    ))
}

/// One Community JDBC driver declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityDriverConfig {
    pub url: String,
    pub jdbc_driver: String,
    pub jdbc_driver_class: String,
    pub download_urls: Vec<String>,
    pub custom: bool,
    pub default_driver: bool,
}

/// Stable, process-neutral projection of one Community `IPlugin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityPlugin {
    pub database_type: String,
    pub name: String,
    pub behavior: CommunityPluginBehavior,
    pub drivers: Vec<CommunityDriverConfig>,
    pub services: CommunityPluginServices,
}

/// Database-model and script behavior declared by one Community plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityPluginBehavior {
    pub supports_database: bool,
    pub supports_schema: bool,
    pub preserves_script_batch_execution: bool,
}

/// Optional Community services exposed by one plugin.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityPluginServices {
    pub metadata_available: bool,
    pub sql_builder_available: bool,
    pub sql_parser_available: bool,
    pub dml_builder_available: bool,
    pub value_processor_available: bool,
    pub identifier_processor_available: bool,
}

/// Community plugin inventory for one fixed source commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityPluginCatalog {
    pub source_commit: String,
    pub plugins: Vec<CommunityPlugin>,
}

/// Process-neutral database schema metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunitySchema {
    pub database_name: String,
    pub name: String,
    pub comment: String,
    pub owner: String,
    pub system: bool,
}

/// Process-neutral database metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityDatabase {
    pub name: String,
    pub comment: String,
    pub charset: String,
    pub collation: String,
    pub owner: String,
    pub system: bool,
}

/// Process-neutral table metadata without nested column or index payloads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityTable {
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
    pub increment_value: Option<i64>,
    pub partition: String,
    pub tablespace: String,
    pub rows: Option<i64>,
    pub data_length: Option<i64>,
    pub create_time: String,
    pub update_time: String,
}

/// Process-neutral table-column metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityTableColumn {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub name: String,
    pub column_type: String,
    pub data_type: Option<i32>,
    pub default_value: String,
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

/// Process-neutral metadata for one indexed column.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityTableIndexColumn {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub index_name: String,
    pub column_name: String,
    pub column_type: String,
    pub comment: String,
    pub ordinal_position: Option<i32>,
    pub collation: String,
    pub non_unique: Option<bool>,
    pub index_qualifier: String,
    pub sort_order: String,
    pub cardinality: Option<i64>,
    pub pages: Option<i64>,
    pub filter_condition: String,
    pub sub_part: Option<i64>,
}

/// Process-neutral table-index metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityTableIndex {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub name: String,
    pub index_type: String,
    pub unique: Option<bool>,
    pub comment: String,
    pub columns: Vec<CommunityTableIndexColumn>,
    pub concurrently: Option<bool>,
    pub method: String,
    pub foreign_schema_name: String,
    pub foreign_table_name: String,
    pub foreign_column_names: Vec<String>,
}

/// Process-neutral foreign-key metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityForeignKey {
    pub primary_table_database: String,
    pub primary_table_schema: String,
    pub primary_table_name: String,
    pub primary_column_name: String,
    pub foreign_table_database: String,
    pub foreign_table_schema: String,
    pub foreign_table_name: String,
    pub foreign_column_name: String,
    pub key_sequence: i32,
    pub update_rule: i32,
    pub delete_rule: i32,
    pub foreign_key_name: String,
    pub primary_key_name: String,
    pub deferrability: i32,
}

/// Process-neutral primary-key metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityPrimaryKey {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub column_name: String,
    pub name: String,
}

/// Process-neutral database function metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityFunction {
    pub database_name: String,
    pub schema_name: String,
    pub name: String,
    pub remarks: String,
    pub function_type: Option<i32>,
    pub specific_name: String,
    pub body: String,
    pub template: String,
}

/// Process-neutral function-parameter metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityFunctionParameter {
    pub function_database: String,
    pub function_schema: String,
    pub function_name: String,
    pub column_name: String,
    pub column_type: Option<i32>,
    pub data_type: Option<i32>,
    pub type_name: String,
    pub precision: Option<i32>,
    pub length: Option<i32>,
    pub scale: Option<i32>,
    pub radix: Option<i32>,
    pub nullable: Option<i32>,
    pub remarks: String,
    pub char_octet_length: Option<i32>,
    pub ordinal_position: Option<i32>,
    pub is_nullable: String,
    pub specific_name: String,
}

/// Process-neutral database procedure metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityProcedure {
    pub database_name: String,
    pub schema_name: String,
    pub name: String,
    pub remarks: String,
    pub procedure_type: Option<i32>,
    pub specific_name: String,
    pub body: String,
}

/// Process-neutral procedure-parameter metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityProcedureParameter {
    pub procedure_database: String,
    pub procedure_schema: String,
    pub procedure_name: String,
    pub column_name: String,
    pub column_type: Option<i32>,
    pub data_type: Option<i32>,
    pub type_name: String,
    pub precision: Option<i32>,
    pub length: Option<i32>,
    pub scale: Option<i32>,
    pub radix: Option<i32>,
    pub nullable: Option<i32>,
    pub remarks: String,
    pub column_default: String,
    pub sql_data_type: Option<i32>,
    pub sql_datetime_sub: Option<i32>,
    pub char_octet_length: Option<i32>,
    pub ordinal_position: Option<i32>,
    pub is_nullable: String,
    pub specific_name: String,
}

/// Process-neutral database trigger metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunityTrigger {
    pub database_name: String,
    pub schema_name: String,
    pub name: String,
    pub event_manipulation: String,
    pub body: String,
}

/// One statement returned by the retained Community parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityParsedStatement {
    pub sql: String,
    pub statement_type: String,
    pub kind: String,
}

/// Bounded parser projection for a SQL input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunitySqlAnalysis {
    pub is_select: bool,
    pub statements: Vec<CommunityParsedStatement>,
}

/// One syntax diagnostic returned by the retained Community parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunitySqlDiagnostic {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub token_text: String,
    pub message: String,
}

/// Bounded syntax-validation projection for a SQL input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunitySqlValidation {
    pub valid: bool,
    pub statements: Vec<CommunityParsedStatement>,
    pub diagnostics: Vec<CommunitySqlDiagnostic>,
}

/// Bounded SQL text returned by the retained Community formatter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityFormattedSql {
    pub sql: String,
}

/// Pure namespace-SQL generation request without product datasource identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCommunityNamespaceSqlRequest {
    pub database_type: String,
    pub operation: CommunityNamespaceSqlOperation,
}

/// Supported database and schema lifecycle operations. Raw SQL is absent by design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommunityNamespaceSqlOperation {
    CreateDatabase {
        database: CommunityDatabase,
    },
    AlterDatabase {
        old_database: CommunityDatabase,
        new_database: CommunityDatabase,
    },
    DropDatabase {
        database_name: String,
    },
    UseDatabase {
        database_name: String,
    },
    CreateSchema {
        schema: CommunitySchema,
    },
    AlterSchema {
        old_schema_name: String,
        new_schema_name: String,
    },
    DropSchema {
        schema_name: String,
    },
}

/// Pure typed-DML generation request without product datasource identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCommunityDmlRequest {
    pub database_type: String,
    pub target: CommunityDmlTarget,
    pub statement: CommunityDmlStatement,
}

/// Independently quoted database, schema, and table identifier segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityDmlTarget {
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    pub table_name: String,
}

/// One raw column name and its database type metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityDmlColumn {
    pub name: String,
    pub data_type_name: String,
    pub precision: Option<u32>,
    pub scale: Option<i32>,
}

/// One typed value. There is deliberately no raw-SQL or expression variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommunityDmlValue {
    Null,
    String(String),
    Decimal(String),
    Boolean(bool),
    Temporal(CommunityDmlTemporal),
    Binary(Vec<u8>),
}

/// Strict ISO-8601 temporal value and its semantic kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityDmlTemporal {
    pub kind: CommunityDmlTemporalKind,
    pub iso8601: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommunityDmlTemporalKind {
    Date,
    Time,
    LocalDatetime,
    OffsetDatetime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityDmlRow {
    pub values: Vec<CommunityDmlValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityDmlAssignment {
    pub column: CommunityDmlColumn,
    pub value: CommunityDmlValue,
}

/// Supported Stage 7K statements. Delete and expression-bearing statements are absent by design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommunityDmlStatement {
    SingleInsert {
        columns: Vec<CommunityDmlColumn>,
        row: CommunityDmlRow,
    },
    MultiInsert {
        columns: Vec<CommunityDmlColumn>,
        rows: Vec<CommunityDmlRow>,
    },
    Update {
        assignments: Vec<CommunityDmlAssignment>,
        predicates: Vec<CommunityDmlAssignment>,
    },
}

/// Session-bound SQL-completion input without product storage identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompleteCommunitySqlRequest {
    pub database_type: String,
    pub database_name: String,
    pub schema_name: String,
    pub datasource_name: String,
    pub sql: String,
    pub cursor_utf16: u32,
    pub min_prefix_length: u32,
    pub need_full_name: bool,
    pub keyword_case: String,
    pub active_snippet_slot: Option<CommunitySqlCompletionActiveSnippetSlot>,
    pub transaction_id: Option<String>,
}

/// Active snippet edit range supplied by the editor, in UTF-16 code units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunitySqlCompletionActiveSnippetSlot {
    pub slot_type: String,
    pub replace_start_utf16: u32,
    pub replace_end_utf16: u32,
}

/// One SQL-completion result projected out of the Community runtime.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunitySqlCompletion {
    pub status: String,
    pub replace_start_utf16: u32,
    pub replace_end_utf16: u32,
    pub candidates: Vec<CommunitySqlCompletionCandidate>,
    pub editor_hints: Vec<CommunitySqlCompletionEditorHint>,
    pub reason_code: Option<String>,
}

/// One bounded SQL-completion candidate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunitySqlCompletionCandidate {
    pub id: Option<String>,
    pub label: String,
    pub candidate_type: String,
    pub insert_text: Option<String>,
    pub insert_type: String,
    pub replace_start_utf16: Option<u32>,
    pub replace_end_utf16: Option<u32>,
    pub detail: Option<String>,
    pub description: Option<String>,
    pub data_type: Option<String>,
    pub object_type: Option<String>,
    pub comment: Option<String>,
    pub datasource_name: Option<String>,
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
    pub table_alias: Option<String>,
    pub column_name: Option<String>,
    pub object_name: Option<String>,
    pub parameter_mode: Option<String>,
    pub sort_rank: Option<i32>,
    pub sort_text: Option<String>,
    pub snippet_slots: Vec<String>,
}

/// One Community editor hint and its bounded items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunitySqlCompletionEditorHint {
    pub hint_type: String,
    pub statement_range: Option<CommunitySqlCompletionRange>,
    pub row_range: Option<CommunitySqlCompletionRange>,
    pub value_range: Option<CommunitySqlCompletionRange>,
    pub items: Vec<CommunitySqlCompletionEditorHintItem>,
}

/// One item inside a Community editor hint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunitySqlCompletionEditorHintItem {
    pub row_index: u32,
    pub column_index: u32,
    pub field_name: Option<String>,
    pub field_type: Option<String>,
    pub label: Option<String>,
    pub range: Option<CommunitySqlCompletionRange>,
    pub active: bool,
}

/// One-based line and UTF-16-column range returned by Community.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommunitySqlCompletionRange {
    pub start_line_number: u32,
    pub start_column: u32,
    pub end_line_number: u32,
    pub end_column: u32,
}

/// Client bound to one validated Java engine generation.
#[derive(Clone)]
pub struct CommunityClient {
    client: EngineClient,
    binding: EngineBinding,
    expected_source_commit: String,
}

impl CommunityClient {
    /// Lists every Community plugin discovered through `ServiceLoader`.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine is stale, unavailable, rejects the
    /// request, or returns an invalid catalog.
    pub async fn list_plugins(&self) -> Result<CommunityPluginCatalog, BridgeError> {
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::ListCommunityPlugins(
                    wire::ListCommunityPluginsRequest {},
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityPluginCatalog(catalog)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community plugin catalog response")
                .await;
        };
        if catalog.source_commit != self.expected_source_commit {
            return self
                .client
                .protocol_violation(
                    "Community plugin catalog source commit did not match the configured classpath",
                )
                .await;
        }
        Ok(catalog.into())
    }

    /// Lists schemas through the selected plugin's real `IDbMetaData` object.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_schemas(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunitySchema>, BridgeError> {
        let database_type = database_type.into();
        let database_name = database_name.into();
        validate_database_type(&database_type)?;
        validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
        if let Some(transaction_id) = transaction_id.as_deref() {
            validate_non_blank_utf8(transaction_id, MAX_PROTOCOL_ID_BYTES, "transaction id")?;
        }
        if self.binding != session.binding {
            return Err(BridgeError::StaleHandle(
                "Community metadata session belongs to another engine".to_owned(),
            ));
        }
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_SCHEMA_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunitySchemas(
                    wire::ListCommunitySchemasRequest {
                        database_type,
                        database_name,
                        transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunitySchemaList(schemas)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community schema-list response")
                .await;
        };
        Ok(schemas.schemas.into_iter().map(Into::into).collect())
    }

    /// Lists databases through the selected plugin's real `IDbMetaData` object.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_databases(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityDatabase>, BridgeError> {
        let database_type = database_type.into();
        validate_database_type(&database_type)?;
        validate_metadata_session(&self.binding, session, transaction_id.as_deref())?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_OBJECT_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityDatabases(
                    wire::ListCommunityDatabasesRequest {
                        database_type,
                        transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityDatabaseList(databases)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community database-list response")
                .await;
        };
        Ok(databases.databases.into_iter().map(Into::into).collect())
    }

    /// Lists tables through the selected plugin's real `IDbMetaData` object.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_tables(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        table_name_pattern: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityTable>, BridgeError> {
        let database_type = database_type.into();
        let database_name = database_name.into();
        let schema_name = schema_name.into();
        let table_name_pattern = table_name_pattern.into();
        validate_database_type(&database_type)?;
        validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
        validate_utf8(&schema_name, MAX_SCALAR_BYTES, "schema name")?;
        validate_utf8(&table_name_pattern, MAX_SCALAR_BYTES, "table name pattern")?;
        validate_metadata_session(&self.binding, session, transaction_id.as_deref())?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_OBJECT_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityTables(
                    wire::ListCommunityTablesRequest {
                        database_type,
                        database_name,
                        schema_name,
                        table_name_pattern,
                        transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityTableList(tables)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community table-list response")
                .await;
        };
        Ok(tables.tables.into_iter().map(Into::into).collect())
    }

    /// Lists columns through the selected plugin's real `IDbMetaData` object.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_columns(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        table_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityTableColumn>, BridgeError> {
        let request = metadata_table_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            table_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_OBJECT_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityColumns(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityTableColumnList(columns)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community column-list response")
                .await;
        };
        Ok(columns.columns.into_iter().map(Into::into).collect())
    }

    /// Lists indexes through the selected plugin's real `IDbMetaData` object.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_indexes(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        table_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityTableIndex>, BridgeError> {
        let request = metadata_table_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            table_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_OBJECT_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityIndexes(
                    wire::ListCommunityIndexesRequest {
                        database_type: request.database_type,
                        database_name: request.database_name,
                        schema_name: request.schema_name,
                        table_name: request.table_name,
                        transaction_id: request.transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityTableIndexList(indexes)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community index-list response")
                .await;
        };
        Ok(indexes.indexes.into_iter().map(Into::into).collect())
    }

    /// Lists views through the selected plugin's real `IDbMetaData` object.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_views(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        view_name_pattern: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityTable>, BridgeError> {
        let database_type = database_type.into();
        let database_name = database_name.into();
        let schema_name = schema_name.into();
        let view_name_pattern = view_name_pattern.into();
        validate_database_type(&database_type)?;
        validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
        validate_utf8(&schema_name, MAX_SCALAR_BYTES, "schema name")?;
        validate_utf8(&view_name_pattern, MAX_SCALAR_BYTES, "view name pattern")?;
        validate_metadata_session(&self.binding, session, transaction_id.as_deref())?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_RELATION_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityViews(
                    wire::ListCommunityViewsRequest {
                        database_type,
                        database_name,
                        schema_name,
                        view_name_pattern,
                        transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityViewList(views)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community view-list response")
                .await;
        };
        Ok(views.views.into_iter().map(Into::into).collect())
    }

    /// Lists foreign keys imported by one table through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_imported_keys(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        table_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityForeignKey>, BridgeError> {
        let request = metadata_key_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            table_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_RELATION_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityImportedKeys(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityImportedKeyList(keys)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community imported-key-list response")
                .await;
        };
        Ok(keys.keys.into_iter().map(Into::into).collect())
    }

    /// Lists foreign keys exported by one table through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_exported_keys(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        table_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityForeignKey>, BridgeError> {
        let request = metadata_key_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            table_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_RELATION_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityExportedKeys(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityExportedKeyList(keys)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community exported-key-list response")
                .await;
        };
        Ok(keys.keys.into_iter().map(Into::into).collect())
    }

    /// Lists primary-key columns for one table through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_primary_keys(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        table_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityPrimaryKey>, BridgeError> {
        let request = metadata_key_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            table_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_RELATION_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityPrimaryKeys(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityPrimaryKeyList(keys)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community primary-key-list response")
                .await;
        };
        Ok(keys.keys.into_iter().map(Into::into).collect())
    }

    /// Lists database functions through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_functions(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityFunction>, BridgeError> {
        let scope = metadata_list_scope_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityFunctions(
                    wire::ListCommunityFunctionsRequest {
                        database_type: scope.database_type,
                        database_name: scope.database_name,
                        schema_name: scope.schema_name,
                        transaction_id: scope.transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityFunctionList(functions)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community function-list response")
                .await;
        };
        Ok(functions.functions.into_iter().map(Into::into).collect())
    }

    /// Reads one database function through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn get_function(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        function_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<CommunityFunction, BridgeError> {
        let request = metadata_function_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            function_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::GetCommunityFunction(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityFunction(function)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community function response")
                .await;
        };
        Ok(function.into())
    }

    /// Lists parameters for one database function through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_function_parameters(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        function_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityFunctionParameter>, BridgeError> {
        let request = metadata_function_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            function_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityFunctionParameters(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityFunctionParameterList(parameters)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community function-parameter-list response")
                .await;
        };
        Ok(parameters.parameters.into_iter().map(Into::into).collect())
    }

    /// Lists database procedures through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_procedures(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityProcedure>, BridgeError> {
        let scope = metadata_list_scope_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityProcedures(
                    wire::ListCommunityProceduresRequest {
                        database_type: scope.database_type,
                        database_name: scope.database_name,
                        schema_name: scope.schema_name,
                        transaction_id: scope.transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityProcedureList(procedures)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community procedure-list response")
                .await;
        };
        Ok(procedures.procedures.into_iter().map(Into::into).collect())
    }

    /// Reads one database procedure through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn get_procedure(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        procedure_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<CommunityProcedure, BridgeError> {
        let request = metadata_procedure_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            procedure_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::GetCommunityProcedure(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityProcedure(procedure)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community procedure response")
                .await;
        };
        Ok(procedure.into())
    }

    /// Lists parameters for one database procedure through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_procedure_parameters(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        procedure_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityProcedureParameter>, BridgeError> {
        let request = metadata_procedure_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            procedure_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityProcedureParameters(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityProcedureParameterList(parameters)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community procedure-parameter-list response")
                .await;
        };
        Ok(parameters.parameters.into_iter().map(Into::into).collect())
    }

    /// Lists database triggers through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn list_triggers(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<Vec<CommunityTrigger>, BridgeError> {
        let scope = metadata_list_scope_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            transaction_id,
        )?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::ListCommunityTriggers(
                    wire::ListCommunityTriggersRequest {
                        database_type: scope.database_type,
                        database_name: scope.database_name,
                        schema_name: scope.schema_name,
                        transaction_id: scope.transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityTriggerList(triggers)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community trigger-list response")
                .await;
        };
        Ok(triggers.triggers.into_iter().map(Into::into).collect())
    }

    /// Reads one database trigger through Community metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// plugin/JDBC failure, timeout, or an invalid response.
    pub async fn get_trigger(
        &self,
        session: &Session,
        database_type: impl Into<String>,
        database_name: impl Into<String>,
        schema_name: impl Into<String>,
        trigger_name: impl Into<String>,
        transaction_id: Option<String>,
    ) -> Result<CommunityTrigger, BridgeError> {
        let scope = metadata_scope_request(
            &self.binding,
            session,
            database_type.into(),
            database_name.into(),
            schema_name.into(),
            transaction_id,
        )?;
        let trigger_name = trigger_name.into();
        validate_non_blank_utf8(&trigger_name, MAX_SCALAR_BYTES, "trigger name")?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::GetCommunityTrigger(
                    wire::GetCommunityTriggerRequest {
                        database_type: scope.database_type,
                        database_name: scope.database_name,
                        schema_name: scope.schema_name,
                        trigger_name,
                        transaction_id: scope.transaction_id,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityTrigger(trigger)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community trigger response")
                .await;
        };
        Ok(trigger.into())
    }

    /// Builds `CREATE SCHEMA` through the selected plugin's SQL builder.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, missing builder support, engine
    /// failure, or an invalid response.
    pub async fn build_create_schema(
        &self,
        database_type: impl Into<String>,
        schema: CommunitySchema,
    ) -> Result<String, BridgeError> {
        let database_type = database_type.into();
        validate_database_type(&database_type)?;
        validate_schema(&schema, true)?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_SQL_BUILDER_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::BuildCommunityCreateSchema(
                    wire::BuildCommunityCreateSchemaRequest {
                        database_type,
                        schema: Some(schema.into()),
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityBuiltSql(built)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community built-SQL response")
                .await;
        };
        Ok(built.sql)
    }

    /// Builds typed INSERT or UPDATE SQL through the selected Community plugin.
    /// This pure generation call neither opens a JDBC session nor executes SQL.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid shape, identifiers or values, unavailable
    /// plugin services, engine failure, or an invalid response.
    pub async fn build_dml(
        &self,
        request: BuildCommunityDmlRequest,
    ) -> Result<String, BridgeError> {
        let request = community_dml_request(request)?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_DML_BUILDER_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::BuildCommunityDml(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityBuiltDml(built)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community built-DML response")
                .await;
        };
        Ok(built.sql)
    }

    /// Builds database or schema lifecycle SQL through the selected Community plugin.
    /// This pure generation call neither opens a JDBC session nor executes SQL.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid identifiers or properties, unavailable
    /// plugin services, engine failure, or an invalid response.
    pub async fn build_namespace_sql(
        &self,
        request: BuildCommunityNamespaceSqlRequest,
    ) -> Result<String, BridgeError> {
        let request = community_namespace_sql_request(request)?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_NAMESPACE_BUILDER_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::BuildCommunityNamespaceSql(request),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityBuiltNamespaceSql(built)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community built-namespace-SQL response")
                .await;
        };
        Ok(built.sql)
    }

    /// Parses SQL through the selected plugin's retained syntax plugin.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, unsupported syntax, parser failure,
    /// transport failure, or an invalid response.
    pub async fn parse_sql(
        &self,
        database_type: impl Into<String>,
        sql: impl Into<String>,
    ) -> Result<CommunitySqlAnalysis, BridgeError> {
        let database_type = database_type.into();
        let sql = sql.into();
        validate_database_type(&database_type)?;
        validate_non_blank_utf8(&sql, MAX_SQL_BYTES, "SQL")?;
        let response =
            self.client
                .send_bound_request(
                    &self.binding,
                    COMMUNITY_SQL_PARSER_CAPABILITY,
                    None,
                    None,
                    wire::client_envelope::Payload::ParseCommunitySql(
                        wire::ParseCommunitySqlRequest { database_type, sql },
                    ),
                    PendingLane::FatalOnUnknown,
                )
                .await?;
        let Some(wire::server_envelope::Payload::CommunitySqlAnalysis(analysis)) = response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community SQL-analysis response")
                .await;
        };
        Ok(analysis.into())
    }

    /// Validates SQL through the selected plugin's retained syntax plugin.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, unsupported syntax, parser failure,
    /// transport failure, or an invalid response.
    pub async fn validate_sql(
        &self,
        database_type: impl Into<String>,
        sql: impl Into<String>,
    ) -> Result<CommunitySqlValidation, BridgeError> {
        let database_type = database_type.into();
        let sql = sql.into();
        validate_database_type(&database_type)?;
        validate_non_blank_utf8(&sql, MAX_SQL_BYTES, "SQL")?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_SQL_VALIDATION_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::ValidateCommunitySql(
                    wire::ValidateCommunitySqlRequest { database_type, sql },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunitySqlValidation(validation)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community SQL-validation response")
                .await;
        };
        Ok(validation.into())
    }

    /// Formats SQL through the retained Community-compatible `SqlFormatter`,
    /// using `database_type` to select its SQL dialect.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, unsupported formatting, formatter
    /// failure, transport failure, or an invalid response.
    pub async fn format_sql(
        &self,
        database_type: impl Into<String>,
        sql: impl Into<String>,
    ) -> Result<CommunityFormattedSql, BridgeError> {
        let database_type = database_type.into();
        let sql = sql.into();
        validate_database_type(&database_type)?;
        validate_non_blank_utf8(&sql, MAX_SQL_BYTES, "SQL")?;
        validate_sql_formatter_complexity(&sql)?;
        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_SQL_FORMATTER_CAPABILITY,
                None,
                None,
                wire::client_envelope::Payload::FormatCommunitySql(
                    wire::FormatCommunitySqlRequest { database_type, sql },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunityFormattedSql(formatted)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community formatted-SQL response")
                .await;
        };
        Ok(formatted.into())
    }

    /// Completes SQL through Community using one claimed read-only JDBC session.
    ///
    /// The product datasource identifier never crosses this boundary. A fresh,
    /// non-zero monotonic scope isolates Community's process-global caches.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a session from another engine,
    /// completion/JDBC failure, timeout, or an invalid response.
    pub async fn complete_sql(
        &self,
        session: &Session,
        request: CompleteCommunitySqlRequest,
    ) -> Result<CommunitySqlCompletion, BridgeError> {
        let CompleteCommunitySqlRequest {
            database_type,
            database_name,
            schema_name,
            datasource_name,
            sql,
            cursor_utf16,
            min_prefix_length,
            need_full_name,
            keyword_case,
            active_snippet_slot,
            transaction_id,
        } = request;
        validate_database_type(&database_type)?;
        validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
        validate_utf8(&schema_name, MAX_SCALAR_BYTES, "schema name")?;
        validate_utf8(&datasource_name, MAX_SCALAR_BYTES, "datasource name")?;
        validate_non_blank_utf8(&sql, MAX_SQL_BYTES, "SQL")?;
        let layout = SqlUtf16Layout::new(&sql)?;
        if cursor_utf16 > layout.length {
            return Err(BridgeError::InvalidRequest(format!(
                "SQL completion cursor {cursor_utf16} exceeds the {} UTF-16-unit SQL length",
                layout.length
            )));
        }
        validate_completion_prefix_length(min_prefix_length)?;
        let keyword_case = normalize_keyword_case(&keyword_case)?;
        let active_snippet_slot = active_snippet_slot
            .as_ref()
            .map(|slot| completion_active_snippet_slot(slot, layout.length))
            .transpose()?;
        validate_metadata_session(&self.binding, session, transaction_id.as_deref())?;
        let datasource_scope = next_sql_completion_datasource_scope()?;

        let response = self
            .client
            .send_bound_request(
                &self.binding,
                COMMUNITY_SQL_COMPLETION_CAPABILITY,
                Some(&session.id),
                Some(session.state.clone()),
                wire::client_envelope::Payload::CompleteCommunitySql(
                    wire::CompleteCommunitySqlRequest {
                        database_type,
                        database_name,
                        schema_name,
                        datasource_name,
                        sql,
                        cursor_utf16,
                        min_prefix_length,
                        need_full_name,
                        keyword_case,
                        active_snippet_slot,
                        transaction_id,
                        datasource_scope,
                    },
                ),
                PendingLane::FatalOnUnknown,
            )
            .await?;
        let Some(wire::server_envelope::Payload::CommunitySqlCompletion(completion)) =
            response.payload
        else {
            return self
                .client
                .protocol_violation("expected Community SQL-completion response")
                .await;
        };
        if let Err(message) = validate_completion_for_sql(&completion, &layout) {
            return self.client.protocol_violation(message).await;
        }
        Ok(completion.into())
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.binding.generation
    }

    #[must_use]
    pub fn engine_instance_id(&self) -> &str {
        &self.binding.engine_instance_id
    }
}

impl EngineClient {
    /// Creates a Community SPI client bound to the ready engine generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the fixed Community classpath is not configured or
    /// the engine is not ready.
    pub fn community_client(&self) -> Result<CommunityClient, BridgeError> {
        if !self.community_compatibility_configured() {
            return Err(BridgeError::InvalidConfig(
                "Community compatibility classpath is not configured".to_owned(),
            ));
        }
        Ok(CommunityClient {
            client: self.clone(),
            binding: self.capture_binding()?,
            expected_source_commit: self.inner.community_source_commit.clone(),
        })
    }
}

impl From<wire::CommunityPluginCatalog> for CommunityPluginCatalog {
    fn from(catalog: wire::CommunityPluginCatalog) -> Self {
        Self {
            source_commit: catalog.source_commit,
            plugins: catalog.plugins.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<wire::CommunityPluginDescriptor> for CommunityPlugin {
    fn from(plugin: wire::CommunityPluginDescriptor) -> Self {
        Self {
            database_type: plugin.database_type,
            name: plugin.name,
            behavior: CommunityPluginBehavior {
                supports_database: plugin.supports_database,
                supports_schema: plugin.supports_schema,
                preserves_script_batch_execution: plugin.preserves_script_batch_execution,
            },
            drivers: plugin.drivers.into_iter().map(Into::into).collect(),
            services: CommunityPluginServices {
                metadata_available: plugin.metadata_available,
                sql_builder_available: plugin.sql_builder_available,
                sql_parser_available: plugin.sql_parser_available,
                dml_builder_available: plugin.dml_builder_available,
                value_processor_available: plugin.value_processor_available,
                identifier_processor_available: plugin.identifier_processor_available,
            },
        }
    }
}

impl From<wire::CommunityDriverConfig> for CommunityDriverConfig {
    fn from(driver: wire::CommunityDriverConfig) -> Self {
        Self {
            url: driver.url,
            jdbc_driver: driver.jdbc_driver,
            jdbc_driver_class: driver.jdbc_driver_class,
            download_urls: driver.download_urls,
            custom: driver.custom,
            default_driver: driver.default_driver,
        }
    }
}

impl From<wire::CommunitySchema> for CommunitySchema {
    fn from(schema: wire::CommunitySchema) -> Self {
        Self {
            database_name: schema.database_name,
            name: schema.name,
            comment: schema.comment,
            owner: schema.owner,
            system: schema.system,
        }
    }
}

impl From<CommunitySchema> for wire::CommunitySchema {
    fn from(schema: CommunitySchema) -> Self {
        Self {
            database_name: schema.database_name,
            name: schema.name,
            comment: schema.comment,
            owner: schema.owner,
            system: schema.system,
        }
    }
}

impl From<wire::CommunityDatabase> for CommunityDatabase {
    fn from(database: wire::CommunityDatabase) -> Self {
        Self {
            name: database.name,
            comment: database.comment,
            charset: database.charset,
            collation: database.collation,
            owner: database.owner,
            system: database.system,
        }
    }
}

impl From<CommunityDatabase> for wire::CommunityDatabase {
    fn from(database: CommunityDatabase) -> Self {
        Self {
            name: database.name,
            comment: database.comment,
            charset: database.charset,
            collation: database.collation,
            owner: database.owner,
            system: database.system,
        }
    }
}

impl From<wire::CommunityTable> for CommunityTable {
    fn from(table: wire::CommunityTable) -> Self {
        Self {
            database_name: table.database_name,
            schema_name: table.schema_name,
            name: table.name,
            table_type: table.r#type,
            comment: table.comment,
            database_type: table.database_type,
            pinned: table.pinned,
            ddl: table.ddl,
            engine: table.engine,
            charset: table.charset,
            collation: table.collation,
            increment_value: table.increment_value,
            partition: table.partition,
            tablespace: table.tablespace,
            rows: table.rows,
            data_length: table.data_length,
            create_time: table.create_time,
            update_time: table.update_time,
        }
    }
}

impl From<wire::CommunityTableColumn> for CommunityTableColumn {
    fn from(column: wire::CommunityTableColumn) -> Self {
        Self {
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
}

impl From<wire::CommunityTableIndexColumn> for CommunityTableIndexColumn {
    fn from(column: wire::CommunityTableIndexColumn) -> Self {
        Self {
            database_name: column.database_name,
            schema_name: column.schema_name,
            table_name: column.table_name,
            index_name: column.index_name,
            column_name: column.column_name,
            column_type: column.r#type,
            comment: column.comment,
            ordinal_position: column.ordinal_position,
            collation: column.collation,
            non_unique: column.non_unique,
            index_qualifier: column.index_qualifier,
            sort_order: column.sort_order,
            cardinality: column.cardinality,
            pages: column.pages,
            filter_condition: column.filter_condition,
            sub_part: column.sub_part,
        }
    }
}

impl From<wire::CommunityTableIndex> for CommunityTableIndex {
    fn from(index: wire::CommunityTableIndex) -> Self {
        Self {
            database_name: index.database_name,
            schema_name: index.schema_name,
            table_name: index.table_name,
            name: index.name,
            index_type: index.r#type,
            unique: index.unique,
            comment: index.comment,
            columns: index.columns.into_iter().map(Into::into).collect(),
            concurrently: index.concurrently,
            method: index.method,
            foreign_schema_name: index.foreign_schema_name,
            foreign_table_name: index.foreign_table_name,
            foreign_column_names: index.foreign_column_names,
        }
    }
}

impl From<wire::CommunityForeignKey> for CommunityForeignKey {
    fn from(key: wire::CommunityForeignKey) -> Self {
        Self {
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
}

impl From<wire::CommunityPrimaryKey> for CommunityPrimaryKey {
    fn from(key: wire::CommunityPrimaryKey) -> Self {
        Self {
            database_name: key.database_name,
            schema_name: key.schema_name,
            table_name: key.table_name,
            column_name: key.column_name,
            name: key.name,
        }
    }
}

impl From<wire::CommunityFunction> for CommunityFunction {
    fn from(function: wire::CommunityFunction) -> Self {
        Self {
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
}

impl From<wire::CommunityFunctionParameter> for CommunityFunctionParameter {
    fn from(parameter: wire::CommunityFunctionParameter) -> Self {
        Self {
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
}

impl From<wire::CommunityProcedure> for CommunityProcedure {
    fn from(procedure: wire::CommunityProcedure) -> Self {
        Self {
            database_name: procedure.database_name,
            schema_name: procedure.schema_name,
            name: procedure.name,
            remarks: procedure.remarks,
            procedure_type: procedure.procedure_type,
            specific_name: procedure.specific_name,
            body: procedure.body,
        }
    }
}

impl From<wire::CommunityProcedureParameter> for CommunityProcedureParameter {
    fn from(parameter: wire::CommunityProcedureParameter) -> Self {
        Self {
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
}

impl From<wire::CommunityTrigger> for CommunityTrigger {
    fn from(trigger: wire::CommunityTrigger) -> Self {
        Self {
            database_name: trigger.database_name,
            schema_name: trigger.schema_name,
            name: trigger.name,
            event_manipulation: trigger.event_manipulation,
            body: trigger.body,
        }
    }
}

impl From<wire::CommunitySqlAnalysis> for CommunitySqlAnalysis {
    fn from(analysis: wire::CommunitySqlAnalysis) -> Self {
        Self {
            is_select: analysis.is_select,
            statements: analysis.statements.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<wire::CommunitySqlValidation> for CommunitySqlValidation {
    fn from(validation: wire::CommunitySqlValidation) -> Self {
        Self {
            valid: validation.valid,
            statements: validation.statements.into_iter().map(Into::into).collect(),
            diagnostics: validation.diagnostics.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<wire::CommunitySqlDiagnostic> for CommunitySqlDiagnostic {
    fn from(diagnostic: wire::CommunitySqlDiagnostic) -> Self {
        Self {
            start_line: diagnostic.start_line,
            start_column: diagnostic.start_column,
            end_line: diagnostic.end_line,
            end_column: diagnostic.end_column,
            token_text: diagnostic.token_text,
            message: diagnostic.message,
        }
    }
}

impl From<wire::CommunityFormattedSql> for CommunityFormattedSql {
    fn from(formatted: wire::CommunityFormattedSql) -> Self {
        Self { sql: formatted.sql }
    }
}

impl From<wire::CommunitySqlCompletion> for CommunitySqlCompletion {
    fn from(completion: wire::CommunitySqlCompletion) -> Self {
        Self {
            status: completion.status,
            replace_start_utf16: completion.replace_start_utf16,
            replace_end_utf16: completion.replace_end_utf16,
            candidates: completion.candidates.into_iter().map(Into::into).collect(),
            editor_hints: completion
                .editor_hints
                .into_iter()
                .map(Into::into)
                .collect(),
            reason_code: completion.reason_code,
        }
    }
}

impl From<wire::CommunitySqlCompletionCandidate> for CommunitySqlCompletionCandidate {
    fn from(candidate: wire::CommunitySqlCompletionCandidate) -> Self {
        Self {
            id: candidate.id,
            label: candidate.label,
            candidate_type: candidate.r#type,
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
}

impl From<wire::CommunitySqlCompletionEditorHint> for CommunitySqlCompletionEditorHint {
    fn from(hint: wire::CommunitySqlCompletionEditorHint) -> Self {
        Self {
            hint_type: hint.r#type,
            statement_range: hint.statement_range.map(Into::into),
            row_range: hint.row_range.map(Into::into),
            value_range: hint.value_range.map(Into::into),
            items: hint.items.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<wire::CommunitySqlCompletionEditorHintItem> for CommunitySqlCompletionEditorHintItem {
    fn from(item: wire::CommunitySqlCompletionEditorHintItem) -> Self {
        Self {
            row_index: item.row_index,
            column_index: item.column_index,
            field_name: item.field_name,
            field_type: item.field_type,
            label: item.label,
            range: item.range.map(Into::into),
            active: item.active,
        }
    }
}

impl From<wire::CommunitySqlCompletionRange> for CommunitySqlCompletionRange {
    fn from(range: wire::CommunitySqlCompletionRange) -> Self {
        Self {
            start_line_number: range.start_line_number,
            start_column: range.start_column,
            end_line_number: range.end_line_number,
            end_column: range.end_column,
        }
    }
}

impl From<wire::CommunityParsedStatement> for CommunityParsedStatement {
    fn from(statement: wire::CommunityParsedStatement) -> Self {
        Self {
            sql: statement.sql,
            statement_type: statement.statement_type,
            kind: statement.r#type,
        }
    }
}

fn validate_metadata_session(
    binding: &EngineBinding,
    session: &Session,
    transaction_id: Option<&str>,
) -> Result<(), BridgeError> {
    if let Some(transaction_id) = transaction_id {
        validate_non_blank_utf8(transaction_id, MAX_PROTOCOL_ID_BYTES, "transaction id")?;
    }
    if binding != &session.binding {
        return Err(BridgeError::StaleHandle(
            "Community metadata session belongs to another engine".to_owned(),
        ));
    }
    Ok(())
}

fn next_sql_completion_datasource_scope() -> Result<u64, BridgeError> {
    NEXT_SQL_COMPLETION_DATASOURCE_SCOPE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, next_java_long_scope)
        .map_err(|_| {
            BridgeError::InvalidRequest(
                "Community SQL-completion datasource scope exhausted".to_owned(),
            )
        })
}

fn next_java_long_scope(scope: u64) -> Option<u64> {
    if i64::try_from(scope).is_ok() {
        Some(scope + 1)
    } else {
        None
    }
}

fn normalize_keyword_case(value: &str) -> Result<String, BridgeError> {
    validate_non_blank_utf8(value, MAX_SCALAR_BYTES, "SQL completion keyword case")?;
    match value.trim().to_ascii_uppercase().as_str() {
        "UPPER" => Ok("UPPER".to_owned()),
        "LOWER" => Ok("LOWER".to_owned()),
        _ => Err(BridgeError::InvalidRequest(
            "SQL completion keyword case must be UPPER or LOWER".to_owned(),
        )),
    }
}

fn validate_completion_prefix_length(value: u32) -> Result<(), BridgeError> {
    if value > MAX_SQL_COMPLETION_PREFIX_LENGTH {
        return Err(BridgeError::InvalidRequest(format!(
            "SQL completion minimum prefix length cannot exceed {MAX_SQL_COMPLETION_PREFIX_LENGTH}"
        )));
    }
    Ok(())
}

fn completion_active_snippet_slot(
    slot: &CommunitySqlCompletionActiveSnippetSlot,
    sql_utf16_length: u32,
) -> Result<wire::CommunitySqlCompletionActiveSnippetSlot, BridgeError> {
    validate_non_blank_utf8(
        &slot.slot_type,
        MAX_SCALAR_BYTES,
        "SQL completion snippet slot type",
    )?;
    let slot_type = match slot.slot_type.trim().to_ascii_uppercase().as_str() {
        "SELECT_FUNCTION" => "SELECT_FUNCTION",
        "CALL_PROCEDURE" => "CALL_PROCEDURE",
        "INSERT_COLUMN_LIST" => "INSERT_COLUMN_LIST",
        _ => {
            return Err(BridgeError::InvalidRequest(
                "SQL completion snippet slot type is invalid".to_owned(),
            ));
        }
    };
    validate_utf16_offset_range(
        slot.replace_start_utf16,
        slot.replace_end_utf16,
        sql_utf16_length,
        "SQL completion active snippet slot",
    )
    .map_err(BridgeError::InvalidRequest)?;
    Ok(wire::CommunitySqlCompletionActiveSnippetSlot {
        r#type: slot_type.to_owned(),
        replace_start_utf16: slot.replace_start_utf16,
        replace_end_utf16: slot.replace_end_utf16,
    })
}

fn validate_completion_for_sql(
    completion: &wire::CommunitySqlCompletion,
    layout: &SqlUtf16Layout,
) -> Result<(), String> {
    validate_utf16_offset_range(
        completion.replace_start_utf16,
        completion.replace_end_utf16,
        layout.length,
        "Community SQL-completion replacement",
    )?;
    for candidate in &completion.candidates {
        match (candidate.replace_start_utf16, candidate.replace_end_utf16) {
            (Some(start), Some(end)) => validate_utf16_offset_range(
                start,
                end,
                layout.length,
                "Community SQL-completion candidate replacement",
            )?,
            (None, None) => {}
            _ => {
                return Err(
                    "Community SQL-completion candidate provided only one replacement endpoint"
                        .to_owned(),
                );
            }
        }
    }
    for hint in &completion.editor_hints {
        for (label, range) in [
            ("statement", hint.statement_range.as_ref()),
            ("row", hint.row_range.as_ref()),
            ("value", hint.value_range.as_ref()),
        ] {
            if let Some(range) = range {
                layout.validate_range(range, label)?;
            }
        }
        for item in &hint.items {
            if let Some(range) = item.range.as_ref() {
                layout.validate_range(range, "item")?;
            }
        }
    }
    Ok(())
}

fn validate_utf16_offset_range(
    start: u32,
    end: u32,
    maximum: u32,
    field: &str,
) -> Result<(), String> {
    if start > end {
        return Err(format!("{field} start exceeds its end"));
    }
    if end > maximum {
        return Err(format!(
            "{field} end {end} exceeds the {maximum}-unit UTF-16 SQL length"
        ));
    }
    Ok(())
}

struct SqlUtf16Layout {
    length: u32,
    line_lengths: Vec<u32>,
}

impl SqlUtf16Layout {
    fn new(sql: &str) -> Result<Self, BridgeError> {
        let mut length = 0_u32;
        let mut current_line = 0_u32;
        let mut line_lengths = Vec::new();
        for character in sql.chars() {
            let units = u32::try_from(character.len_utf16()).map_err(|_| {
                BridgeError::InvalidRequest("SQL UTF-16 length is not representable".to_owned())
            })?;
            length = length.checked_add(units).ok_or_else(|| {
                BridgeError::InvalidRequest("SQL UTF-16 length is not representable".to_owned())
            })?;
            if character == '\n' {
                line_lengths.push(current_line);
                current_line = 0;
            } else {
                current_line = current_line.checked_add(units).ok_or_else(|| {
                    BridgeError::InvalidRequest(
                        "SQL UTF-16 line length is not representable".to_owned(),
                    )
                })?;
            }
        }
        line_lengths.push(current_line);
        Ok(Self {
            length,
            line_lengths,
        })
    }

    fn validate_range(
        &self,
        range: &wire::CommunitySqlCompletionRange,
        label: &str,
    ) -> Result<(), String> {
        let start = self.validate_position(
            range.start_line_number,
            range.start_column,
            &format!("Community SQL-completion {label} range start"),
        )?;
        let end = self.validate_position(
            range.end_line_number,
            range.end_column,
            &format!("Community SQL-completion {label} range end"),
        )?;
        if start > end {
            return Err(format!(
                "Community SQL-completion {label} range start exceeds its end"
            ));
        }
        Ok(())
    }

    fn validate_position(&self, line: u32, column: u32, field: &str) -> Result<(u32, u32), String> {
        let line_index = line
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < self.line_lengths.len())
            .ok_or_else(|| format!("{field} line {line} is outside the SQL"))?;
        let maximum_column = self.line_lengths[line_index].saturating_add(1);
        if column == 0 || column > maximum_column {
            return Err(format!(
                "{field} column {column} exceeds the UTF-16 line boundary {maximum_column}"
            ));
        }
        Ok((line, column))
    }
}

fn metadata_scope_request(
    binding: &EngineBinding,
    session: &Session,
    database_type: String,
    database_name: String,
    schema_name: String,
    transaction_id: Option<String>,
) -> Result<wire::ListCommunityFunctionsRequest, BridgeError> {
    validate_database_type(&database_type)?;
    validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
    validate_utf8(&schema_name, MAX_SCALAR_BYTES, "schema name")?;
    validate_metadata_session(binding, session, transaction_id.as_deref())?;
    Ok(wire::ListCommunityFunctionsRequest {
        database_type,
        database_name,
        schema_name,
        transaction_id,
    })
}

fn metadata_list_scope_request(
    binding: &EngineBinding,
    session: &Session,
    database_type: String,
    database_name: String,
    schema_name: String,
    transaction_id: Option<String>,
) -> Result<wire::ListCommunityFunctionsRequest, BridgeError> {
    validate_non_blank_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
    metadata_scope_request(
        binding,
        session,
        database_type,
        database_name,
        schema_name,
        transaction_id,
    )
}

fn metadata_function_request(
    binding: &EngineBinding,
    session: &Session,
    database_type: String,
    database_name: String,
    schema_name: String,
    function_name: String,
    transaction_id: Option<String>,
) -> Result<wire::GetCommunityFunctionRequest, BridgeError> {
    let scope = metadata_scope_request(
        binding,
        session,
        database_type,
        database_name,
        schema_name,
        transaction_id,
    )?;
    validate_non_blank_utf8(&function_name, MAX_SCALAR_BYTES, "function name")?;
    Ok(wire::GetCommunityFunctionRequest {
        database_type: scope.database_type,
        database_name: scope.database_name,
        schema_name: scope.schema_name,
        function_name,
        transaction_id: scope.transaction_id,
    })
}

fn metadata_procedure_request(
    binding: &EngineBinding,
    session: &Session,
    database_type: String,
    database_name: String,
    schema_name: String,
    procedure_name: String,
    transaction_id: Option<String>,
) -> Result<wire::GetCommunityProcedureRequest, BridgeError> {
    let scope = metadata_scope_request(
        binding,
        session,
        database_type,
        database_name,
        schema_name,
        transaction_id,
    )?;
    validate_non_blank_utf8(&procedure_name, MAX_SCALAR_BYTES, "procedure name")?;
    Ok(wire::GetCommunityProcedureRequest {
        database_type: scope.database_type,
        database_name: scope.database_name,
        schema_name: scope.schema_name,
        procedure_name,
        transaction_id: scope.transaction_id,
    })
}

fn metadata_table_request(
    binding: &EngineBinding,
    session: &Session,
    database_type: String,
    database_name: String,
    schema_name: String,
    table_name: String,
    transaction_id: Option<String>,
) -> Result<wire::ListCommunityColumnsRequest, BridgeError> {
    validate_database_type(&database_type)?;
    validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
    validate_utf8(&schema_name, MAX_SCALAR_BYTES, "schema name")?;
    validate_non_blank_utf8(&table_name, MAX_SCALAR_BYTES, "table name")?;
    validate_metadata_session(binding, session, transaction_id.as_deref())?;
    Ok(wire::ListCommunityColumnsRequest {
        database_type,
        database_name,
        schema_name,
        table_name,
        transaction_id,
    })
}

fn metadata_key_request(
    binding: &EngineBinding,
    session: &Session,
    database_type: String,
    database_name: String,
    schema_name: String,
    table_name: String,
    transaction_id: Option<String>,
) -> Result<wire::ListCommunityTableKeysRequest, BridgeError> {
    validate_database_type(&database_type)?;
    validate_utf8(&database_name, MAX_SCALAR_BYTES, "database name")?;
    validate_utf8(&schema_name, MAX_SCALAR_BYTES, "schema name")?;
    validate_non_blank_utf8(&table_name, MAX_SCALAR_BYTES, "table name")?;
    validate_metadata_session(binding, session, transaction_id.as_deref())?;
    Ok(wire::ListCommunityTableKeysRequest {
        database_type,
        database_name,
        schema_name,
        table_name,
        transaction_id,
    })
}

fn hash_regular_file(path: &Path, maximum_bytes: u64) -> Result<([u8; 32], u64), BridgeError> {
    let mut input =
        open_regular_file_no_follow(path).map_err(|source| BridgeError::CommunityArtifact {
            operation: "open and hash",
            path: path.to_path_buf(),
            source,
        })?;
    if input
        .metadata()
        .map_err(|source| BridgeError::CommunityArtifact {
            operation: "inspect",
            path: path.to_path_buf(),
            source,
        })?
        .len()
        > maximum_bytes
    {
        return Err(BridgeError::InvalidConfig(format!(
            "Community classpath artifact {} exceeds the remaining byte budget",
            path.display()
        )));
    }

    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut byte_len = 0_u64;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|source| BridgeError::CommunityArtifact {
                operation: "read and hash",
                path: path.to_path_buf(),
                source,
            })?;
        if count == 0 {
            break;
        }
        let count_u64 = u64::try_from(count).map_err(|_| {
            BridgeError::InvalidConfig(
                "Community classpath artifact read size is not representable".to_owned(),
            )
        })?;
        if count_u64 > maximum_bytes.saturating_sub(byte_len) {
            return Err(BridgeError::InvalidConfig(format!(
                "Community classpath artifact {} exceeds the remaining byte budget",
                path.display()
            )));
        }
        hasher.update(&buffer[..count]);
        byte_len += count_u64;
    }
    Ok((hasher.finalize().into(), byte_len))
}

fn snapshot_artifact(
    artifact: &CommunityArtifact,
    snapshot_path: &Path,
) -> Result<(), BridgeError> {
    let mut input = open_regular_file_no_follow(&artifact.canonical_path).map_err(|source| {
        BridgeError::CommunityArtifact {
            operation: "open for snapshot",
            path: artifact.canonical_path.clone(),
            source,
        }
    })?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(snapshot_path)
        .map_err(|source| BridgeError::CommunityArtifact {
            operation: "create snapshot for",
            path: snapshot_path.to_path_buf(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut copied = 0_u64;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|source| BridgeError::CommunityArtifact {
                operation: "read for snapshot",
                path: artifact.canonical_path.clone(),
                source,
            })?;
        if count == 0 {
            break;
        }
        let count_u64 = u64::try_from(count).map_err(|_| {
            BridgeError::InvalidConfig(
                "Community classpath artifact read size is not representable".to_owned(),
            )
        })?;
        if count_u64 > artifact.byte_len.saturating_sub(copied) {
            return Err(BridgeError::InvalidConfig(format!(
                "Community classpath artifact {} changed after validation",
                artifact.canonical_path.display()
            )));
        }
        output
            .write_all(&buffer[..count])
            .map_err(|source| BridgeError::CommunityArtifact {
                operation: "write snapshot for",
                path: snapshot_path.to_path_buf(),
                source,
            })?;
        hasher.update(&buffer[..count]);
        copied += count_u64;
    }
    if copied != artifact.byte_len || <[u8; 32]>::from(hasher.finalize()) != artifact.sha256 {
        return Err(BridgeError::InvalidConfig(format!(
            "Community classpath artifact {} changed after validation",
            artifact.canonical_path.display()
        )));
    }
    output
        .flush()
        .and_then(|()| output.sync_all())
        .map_err(|source| BridgeError::CommunityArtifact {
            operation: "sync snapshot for",
            path: snapshot_path.to_path_buf(),
            source,
        })
}

fn open_regular_file_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        let flags =
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits())
                .map_err(|_| io::Error::other("artifact open flags are not representable"))?;
        options.custom_flags(flags);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;

        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn validate_source_commit(commit: &str) -> Result<(), BridgeError> {
    if commit.len() != 40
        || commit.len() > MAX_SOURCE_COMMIT_BYTES
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(BridgeError::InvalidConfig(
            "Community source commit must be a 40-character lowercase Git SHA".to_owned(),
        ));
    }
    Ok(())
}

fn community_namespace_sql_request(
    request: BuildCommunityNamespaceSqlRequest,
) -> Result<wire::BuildCommunityNamespaceSqlRequest, BridgeError> {
    validate_database_type(&request.database_type)?;
    let operation = match request.operation {
        CommunityNamespaceSqlOperation::CreateDatabase { database } => {
            validate_namespace_database(&database)?;
            wire::build_community_namespace_sql_request::Operation::CreateDatabase(
                wire::CommunityCreateDatabaseSql {
                    database: Some(database.into()),
                },
            )
        }
        CommunityNamespaceSqlOperation::AlterDatabase {
            old_database,
            new_database,
        } => {
            validate_namespace_database(&old_database)?;
            validate_namespace_database(&new_database)?;
            wire::build_community_namespace_sql_request::Operation::AlterDatabase(
                wire::CommunityAlterDatabaseSql {
                    old_database: Some(old_database.into()),
                    new_database: Some(new_database.into()),
                },
            )
        }
        CommunityNamespaceSqlOperation::DropDatabase { database_name } => {
            validate_namespace_identifier(&database_name, "database name")?;
            wire::build_community_namespace_sql_request::Operation::DropDatabase(
                wire::CommunityDropDatabaseSql { database_name },
            )
        }
        CommunityNamespaceSqlOperation::UseDatabase { database_name } => {
            validate_namespace_identifier(&database_name, "database name")?;
            wire::build_community_namespace_sql_request::Operation::UseDatabase(
                wire::CommunityUseDatabaseSql { database_name },
            )
        }
        CommunityNamespaceSqlOperation::CreateSchema { schema } => {
            validate_namespace_schema(&schema)?;
            wire::build_community_namespace_sql_request::Operation::CreateSchema(
                wire::CommunityCreateSchemaSql {
                    schema: Some(schema.into()),
                },
            )
        }
        CommunityNamespaceSqlOperation::AlterSchema {
            old_schema_name,
            new_schema_name,
        } => {
            validate_namespace_identifier(&old_schema_name, "old schema name")?;
            validate_namespace_identifier(&new_schema_name, "new schema name")?;
            wire::build_community_namespace_sql_request::Operation::AlterSchema(
                wire::CommunityAlterSchemaSql {
                    old_schema_name,
                    new_schema_name,
                },
            )
        }
        CommunityNamespaceSqlOperation::DropSchema { schema_name } => {
            validate_namespace_identifier(&schema_name, "schema name")?;
            wire::build_community_namespace_sql_request::Operation::DropSchema(
                wire::CommunityDropSchemaSql { schema_name },
            )
        }
    };
    let request = wire::BuildCommunityNamespaceSqlRequest {
        database_type: request.database_type,
        operation: Some(operation),
    };
    if request.encoded_len() > MAX_COMMUNITY_RESPONSE_BYTES {
        return Err(BridgeError::InvalidRequest(format!(
            "Community namespace request cannot exceed {MAX_COMMUNITY_RESPONSE_BYTES} encoded bytes"
        )));
    }
    Ok(request)
}

fn validate_namespace_database(database: &CommunityDatabase) -> Result<(), BridgeError> {
    validate_namespace_identifier(&database.name, "database name")?;
    validate_utf8(&database.comment, MAX_COMMENT_BYTES, "database comment")?;
    validate_namespace_property(&database.charset, "database charset")?;
    validate_namespace_property(&database.collation, "database collation")?;
    validate_namespace_property(&database.owner, "database owner")
}

fn validate_namespace_schema(schema: &CommunitySchema) -> Result<(), BridgeError> {
    if !schema.database_name.is_empty() {
        validate_namespace_identifier(&schema.database_name, "schema database name")?;
    }
    validate_namespace_identifier(&schema.name, "schema name")?;
    validate_utf8(&schema.comment, MAX_COMMENT_BYTES, "schema comment")?;
    validate_namespace_property(&schema.owner, "schema owner")
}

fn validate_namespace_identifier(value: &str, field: &str) -> Result<(), BridgeError> {
    validate_non_blank_utf8(value, MAX_NAMESPACE_IDENTIFIER_BYTES, field)?;
    if value.trim() != value
        || value.chars().any(char::is_control)
        || value.contains(['.', ';', '\'', '"', '`', '[', ']'])
        || value.contains("--")
        || value.contains("/*")
        || value.contains("*/")
    {
        return Err(BridgeError::InvalidRequest(format!(
            "Community namespace {field} contains unsafe identifier syntax"
        )));
    }
    Ok(())
}

fn validate_namespace_property(value: &str, field: &str) -> Result<(), BridgeError> {
    validate_utf8(value, MAX_NAMESPACE_PROPERTY_BYTES, field)?;
    if !value.is_empty()
        && (value.trim() != value
            || value.chars().any(char::is_control)
            || value.contains([';', '\'', '"', '`', '[', ']'])
            || value.contains("--")
            || value.contains("/*")
            || value.contains("*/"))
    {
        return Err(BridgeError::InvalidRequest(format!(
            "Community namespace {field} contains unsafe property syntax"
        )));
    }
    Ok(())
}

fn community_dml_request(
    request: BuildCommunityDmlRequest,
) -> Result<wire::BuildCommunityDmlRequest, BridgeError> {
    validate_database_type(&request.database_type)?;
    let target = community_dml_target(request.target)?;
    let statement = match request.statement {
        CommunityDmlStatement::SingleInsert { columns, row } => {
            validate_dml_columns(&columns, "insert columns")?;
            if row.values.len() != columns.len() {
                return Err(BridgeError::InvalidRequest(
                    "Community DML insert row width must equal its column count".to_owned(),
                ));
            }
            let values = dml_values(row.values, false)?;
            wire::build_community_dml_request::Statement::SingleInsert(
                wire::CommunityDmlSingleInsert {
                    columns: columns
                        .into_iter()
                        .map(dml_column)
                        .collect::<Result<_, _>>()?,
                    row: Some(wire::CommunityDmlRow { values }),
                },
            )
        }
        CommunityDmlStatement::MultiInsert { columns, rows } => {
            validate_dml_columns(&columns, "batch insert columns")?;
            if rows.is_empty() || rows.len() > MAX_DML_ROWS {
                return Err(BridgeError::InvalidRequest(format!(
                    "Community DML batch row count must be between 1 and {MAX_DML_ROWS}"
                )));
            }
            let total = columns.len().checked_mul(rows.len()).ok_or_else(|| {
                BridgeError::InvalidRequest("Community DML value count overflowed".to_owned())
            })?;
            if total > MAX_DML_VALUES {
                return Err(BridgeError::InvalidRequest(format!(
                    "Community DML cannot contain more than {MAX_DML_VALUES} values"
                )));
            }
            let mut wire_rows = Vec::with_capacity(rows.len());
            for row in rows {
                if row.values.len() != columns.len() {
                    return Err(BridgeError::InvalidRequest(
                        "Community DML batch row width must equal its column count".to_owned(),
                    ));
                }
                wire_rows.push(wire::CommunityDmlRow {
                    values: dml_values(row.values, false)?,
                });
            }
            wire::build_community_dml_request::Statement::MultiInsert(
                wire::CommunityDmlMultiInsert {
                    columns: columns
                        .into_iter()
                        .map(dml_column)
                        .collect::<Result<_, _>>()?,
                    rows: wire_rows,
                },
            )
        }
        CommunityDmlStatement::Update {
            assignments,
            predicates,
        } => {
            validate_dml_assignments(&assignments, "update assignments", false)?;
            validate_dml_assignments(&predicates, "update predicates", true)?;
            if assignments.len().saturating_add(predicates.len()) > MAX_DML_VALUES {
                return Err(BridgeError::InvalidRequest(format!(
                    "Community DML cannot contain more than {MAX_DML_VALUES} values"
                )));
            }
            wire::build_community_dml_request::Statement::Update(wire::CommunityDmlUpdate {
                assignments: assignments
                    .into_iter()
                    .map(|assignment| dml_assignment(assignment, false))
                    .collect::<Result<_, _>>()?,
                predicates: predicates
                    .into_iter()
                    .map(|assignment| dml_assignment(assignment, true))
                    .collect::<Result<_, _>>()?,
            })
        }
    };
    let request = wire::BuildCommunityDmlRequest {
        database_type: request.database_type,
        target: Some(target),
        statement: Some(statement),
    };
    if request.encoded_len() > MAX_COMMUNITY_RESPONSE_BYTES {
        return Err(BridgeError::InvalidRequest(format!(
            "Community DML request cannot exceed {MAX_COMMUNITY_RESPONSE_BYTES} encoded bytes"
        )));
    }
    Ok(request)
}

fn community_dml_target(
    target: CommunityDmlTarget,
) -> Result<wire::CommunityDmlTarget, BridgeError> {
    Ok(wire::CommunityDmlTarget {
        database_name: target
            .database_name
            .map(|value| validate_dml_identifier_owned(value, "database name"))
            .transpose()?,
        schema_name: target
            .schema_name
            .map(|value| validate_dml_identifier_owned(value, "schema name"))
            .transpose()?,
        table_name: validate_dml_identifier_owned(target.table_name, "table name")?,
    })
}

fn validate_dml_columns(columns: &[CommunityDmlColumn], field: &str) -> Result<(), BridgeError> {
    if columns.is_empty() || columns.len() > MAX_DML_COLUMNS {
        return Err(BridgeError::InvalidRequest(format!(
            "Community DML {field} count must be between 1 and {MAX_DML_COLUMNS}"
        )));
    }
    let mut names = HashSet::with_capacity(columns.len());
    for column in columns {
        validate_dml_column(column)?;
        if !names.insert(column.name.as_str()) {
            return Err(BridgeError::InvalidRequest(format!(
                "Community DML {field} cannot contain duplicate column names"
            )));
        }
    }
    Ok(())
}

fn validate_dml_assignments(
    assignments: &[CommunityDmlAssignment],
    field: &str,
    reject_null: bool,
) -> Result<(), BridgeError> {
    if assignments.is_empty() || assignments.len() > MAX_DML_COLUMNS {
        return Err(BridgeError::InvalidRequest(format!(
            "Community DML {field} count must be between 1 and {MAX_DML_COLUMNS}"
        )));
    }
    let mut names = HashSet::with_capacity(assignments.len());
    for assignment in assignments {
        validate_dml_column(&assignment.column)?;
        validate_dml_value(&assignment.value, reject_null)?;
        if !names.insert(assignment.column.name.as_str()) {
            return Err(BridgeError::InvalidRequest(format!(
                "Community DML {field} cannot contain duplicate column names"
            )));
        }
    }
    Ok(())
}

fn validate_dml_column(column: &CommunityDmlColumn) -> Result<(), BridgeError> {
    validate_dml_identifier(&column.name, "column name")?;
    validate_non_blank_utf8(
        &column.data_type_name,
        MAX_DML_DATA_TYPE_NAME_BYTES,
        "DML data type name",
    )?;
    if column.data_type_name.chars().any(char::is_control) {
        return Err(BridgeError::InvalidRequest(
            "Community DML data type names cannot contain control characters".to_owned(),
        ));
    }
    if column
        .precision
        .is_some_and(|value| value > i32::MAX as u32)
    {
        return Err(BridgeError::InvalidRequest(
            "Community DML precision cannot exceed the Java Integer range".to_owned(),
        ));
    }
    Ok(())
}

fn dml_column(column: CommunityDmlColumn) -> Result<wire::CommunityDmlColumn, BridgeError> {
    validate_dml_column(&column)?;
    Ok(wire::CommunityDmlColumn {
        name: column.name,
        data_type_name: column.data_type_name,
        precision: column.precision,
        scale: column.scale,
    })
}

fn dml_assignment(
    assignment: CommunityDmlAssignment,
    reject_null: bool,
) -> Result<wire::CommunityDmlAssignment, BridgeError> {
    Ok(wire::CommunityDmlAssignment {
        column: Some(dml_column(assignment.column)?),
        value: Some(dml_value(assignment.value, reject_null)?),
    })
}

fn dml_values(
    values: Vec<CommunityDmlValue>,
    reject_null: bool,
) -> Result<Vec<wire::CommunityDmlValue>, BridgeError> {
    if values.len() > MAX_DML_VALUES {
        return Err(BridgeError::InvalidRequest(format!(
            "Community DML cannot contain more than {MAX_DML_VALUES} values"
        )));
    }
    values
        .into_iter()
        .map(|value| dml_value(value, reject_null))
        .collect()
}

fn validate_dml_value(value: &CommunityDmlValue, reject_null: bool) -> Result<(), BridgeError> {
    match value {
        CommunityDmlValue::Null if reject_null => Err(BridgeError::InvalidRequest(
            "Community DML equality predicates cannot compare NULL".to_owned(),
        )),
        CommunityDmlValue::String(value) => {
            validate_utf8(value, MAX_DML_VALUE_BYTES, "DML string value")
        }
        CommunityDmlValue::Decimal(value) => validate_dml_decimal(value),
        CommunityDmlValue::Temporal(value) => {
            validate_non_blank_utf8(&value.iso8601, MAX_DML_TEMPORAL_BYTES, "DML temporal value")?;
            if value.iso8601.chars().any(char::is_control) {
                return Err(BridgeError::InvalidRequest(
                    "Community DML temporal values cannot contain control characters".to_owned(),
                ));
            }
            Ok(())
        }
        CommunityDmlValue::Binary(value) if value.len() > MAX_DML_VALUE_BYTES => {
            Err(BridgeError::InvalidRequest(format!(
                "Community DML binary values cannot exceed {MAX_DML_VALUE_BYTES} bytes"
            )))
        }
        CommunityDmlValue::Null | CommunityDmlValue::Boolean(_) | CommunityDmlValue::Binary(_) => {
            Ok(())
        }
    }
}

fn dml_value(
    value: CommunityDmlValue,
    reject_null: bool,
) -> Result<wire::CommunityDmlValue, BridgeError> {
    validate_dml_value(&value, reject_null)?;
    let value = match value {
        CommunityDmlValue::Null => {
            wire::community_dml_value::Value::NullValue(wire::CommunityDmlNull {})
        }
        CommunityDmlValue::String(value) => wire::community_dml_value::Value::StringValue(value),
        CommunityDmlValue::Decimal(value) => wire::community_dml_value::Value::DecimalValue(value),
        CommunityDmlValue::Boolean(value) => wire::community_dml_value::Value::BooleanValue(value),
        CommunityDmlValue::Temporal(value) => {
            let kind = match value.kind {
                CommunityDmlTemporalKind::Date => wire::CommunityDmlTemporalKind::Date,
                CommunityDmlTemporalKind::Time => wire::CommunityDmlTemporalKind::Time,
                CommunityDmlTemporalKind::LocalDatetime => {
                    wire::CommunityDmlTemporalKind::LocalDatetime
                }
                CommunityDmlTemporalKind::OffsetDatetime => {
                    wire::CommunityDmlTemporalKind::OffsetDatetime
                }
            };
            wire::community_dml_value::Value::TemporalValue(wire::CommunityDmlTemporal {
                kind: kind.into(),
                iso8601: value.iso8601,
            })
        }
        CommunityDmlValue::Binary(value) => wire::community_dml_value::Value::BinaryValue(value),
    };
    Ok(wire::CommunityDmlValue { value: Some(value) })
}

fn validate_dml_decimal(value: &str) -> Result<(), BridgeError> {
    validate_non_blank_utf8(value, MAX_DML_DECIMAL_BYTES, "DML decimal value")?;
    let bytes = value.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    let integer_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if index == integer_start {
        return Err(BridgeError::InvalidRequest(
            "Community DML decimal values must use plain base-10 notation".to_owned(),
        ));
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return Err(BridgeError::InvalidRequest(
                "Community DML decimal fractions require at least one digit".to_owned(),
            ));
        }
    }
    if index != bytes.len() {
        return Err(BridgeError::InvalidRequest(
            "Community DML decimal values must use plain base-10 notation".to_owned(),
        ));
    }
    Ok(())
}

fn validate_dml_identifier_owned(value: String, field: &str) -> Result<String, BridgeError> {
    validate_dml_identifier(&value, field)?;
    Ok(value)
}

fn validate_dml_identifier(value: &str, field: &str) -> Result<(), BridgeError> {
    validate_non_blank_utf8(value, MAX_DML_IDENTIFIER_BYTES, field)?;
    if value.trim() != value
        || value.chars().any(char::is_control)
        || value.contains(['.', ';', '\'', '"', '`', '[', ']'])
        || value.contains("--")
        || value.contains("/*")
        || value.contains("*/")
    {
        return Err(BridgeError::InvalidRequest(format!(
            "Community DML {field} contains unsafe identifier syntax"
        )));
    }
    Ok(())
}

fn validate_database_type(database_type: &str) -> Result<(), BridgeError> {
    validate_non_blank_utf8(database_type, MAX_DATABASE_TYPE_BYTES, "database type")
}

fn validate_sql_formatter_complexity(sql: &str) -> Result<(), BridgeError> {
    let mut units = 0_usize;
    let mut in_ascii_word = false;
    for character in sql.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '$') {
            if !in_ascii_word {
                units += 1;
            }
            in_ascii_word = true;
        } else {
            in_ascii_word = false;
            if !matches!(
                character,
                ' ' | '\t' | '\n' | '\u{000b}' | '\u{000c}' | '\r'
            ) {
                units += 1;
            }
        }
        if units > MAX_SQL_FORMATTER_COMPLEXITY_UNITS {
            return Err(BridgeError::InvalidRequest(format!(
                "SQL formatter complexity cannot exceed {MAX_SQL_FORMATTER_COMPLEXITY_UNITS} units"
            )));
        }
    }
    Ok(())
}

fn validate_schema(schema: &CommunitySchema, require_name: bool) -> Result<(), BridgeError> {
    if require_name {
        validate_non_blank_utf8(&schema.name, MAX_SCALAR_BYTES, "schema name")?;
    } else {
        validate_utf8(&schema.name, MAX_SCALAR_BYTES, "schema name")?;
    }
    validate_utf8(
        &schema.database_name,
        MAX_SCALAR_BYTES,
        "schema database name",
    )?;
    validate_utf8(&schema.comment, MAX_COMMENT_BYTES, "schema comment")?;
    validate_utf8(&schema.owner, MAX_SCALAR_BYTES, "schema owner")
}

fn validate_utf8(value: &str, maximum: usize, field: &str) -> Result<(), BridgeError> {
    if value.len() > maximum {
        return Err(BridgeError::InvalidRequest(format!(
            "{field} cannot exceed {maximum} UTF-8 bytes"
        )));
    }
    Ok(())
}

fn validate_non_blank_utf8(value: &str, maximum: usize, field: &str) -> Result<(), BridgeError> {
    if value.trim().is_empty() {
        return Err(BridgeError::InvalidRequest(format!("{field} is required")));
    }
    validate_utf8(value, maximum, field)
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, fs, path::PathBuf};

    use chat2db_engine_protocol::wire;
    use sha2::{Digest, Sha256};

    use tempfile::TempDir;

    use super::{
        BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest, CommunityClasspath,
        CommunityDatabase, CommunityDmlAssignment, CommunityDmlColumn, CommunityDmlRow,
        CommunityDmlStatement, CommunityDmlTarget, CommunityDmlTemporal, CommunityDmlTemporalKind,
        CommunityDmlValue, CommunityForeignKey, CommunityFormattedSql, CommunityFunction,
        CommunityFunctionParameter, CommunityNamespaceSqlOperation, CommunityPrimaryKey,
        CommunityProcedure, CommunityProcedureParameter, CommunitySchema, CommunitySqlCompletion,
        CommunitySqlCompletionCandidate, CommunitySqlCompletionEditorHint,
        CommunitySqlCompletionEditorHintItem, CommunitySqlCompletionRange, CommunitySqlDiagnostic,
        CommunitySqlValidation, CommunityTrigger, MAX_COMMENT_BYTES, MAX_DML_VALUE_BYTES,
        MAX_NAMESPACE_IDENTIFIER_BYTES, MAX_NAMESPACE_PROPERTY_BYTES, MAX_SCALAR_BYTES,
        MAX_SQL_COMPLETION_PREFIX_LENGTH, MAX_SQL_FORMATTER_COMPLEXITY_UNITS, SqlUtf16Layout,
        community_dml_request, community_namespace_sql_request, next_java_long_scope,
        next_sql_completion_datasource_scope, normalize_keyword_case, validate_completion_for_sql,
        validate_completion_prefix_length, validate_non_blank_utf8,
        validate_sql_formatter_complexity,
    };

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn dml_test_column(name: &str, data_type_name: &str) -> CommunityDmlColumn {
        CommunityDmlColumn {
            name: name.to_owned(),
            data_type_name: data_type_name.to_owned(),
            precision: None,
            scale: None,
        }
    }

    fn dml_test_request(statement: CommunityDmlStatement) -> BuildCommunityDmlRequest {
        BuildCommunityDmlRequest {
            database_type: "H2".to_owned(),
            target: CommunityDmlTarget {
                database_name: Some("inventory".to_owned()),
                schema_name: Some("APP".to_owned()),
                table_name: "items".to_owned(),
            },
            statement,
        }
    }

    fn namespace_database(name: impl Into<String>) -> CommunityDatabase {
        CommunityDatabase {
            name: name.into(),
            comment: "inventory database".to_owned(),
            charset: "UTF8".to_owned(),
            collation: "en_US.UTF-8".to_owned(),
            owner: "app_owner".to_owned(),
            system: false,
        }
    }

    fn namespace_schema(name: impl Into<String>) -> CommunitySchema {
        CommunitySchema {
            database_name: "inventory".to_owned(),
            name: name.into(),
            comment: "application schema".to_owned(),
            owner: "app_owner".to_owned(),
            system: false,
        }
    }

    fn namespace_request(
        operation: CommunityNamespaceSqlOperation,
    ) -> BuildCommunityNamespaceSqlRequest {
        BuildCommunityNamespaceSqlRequest {
            database_type: "POSTGRESQL".to_owned(),
            operation,
        }
    }

    #[test]
    fn namespace_request_maps_every_closed_operation() {
        let operations = [
            CommunityNamespaceSqlOperation::CreateDatabase {
                database: namespace_database("inventory"),
            },
            CommunityNamespaceSqlOperation::AlterDatabase {
                old_database: namespace_database("inventory"),
                new_database: namespace_database("inventory_v2"),
            },
            CommunityNamespaceSqlOperation::DropDatabase {
                database_name: "inventory".to_owned(),
            },
            CommunityNamespaceSqlOperation::UseDatabase {
                database_name: "inventory".to_owned(),
            },
            CommunityNamespaceSqlOperation::CreateSchema {
                schema: namespace_schema("app"),
            },
            CommunityNamespaceSqlOperation::AlterSchema {
                old_schema_name: "app".to_owned(),
                new_schema_name: "app_v2".to_owned(),
            },
            CommunityNamespaceSqlOperation::DropSchema {
                schema_name: "app".to_owned(),
            },
        ];

        for (index, operation) in operations.into_iter().enumerate() {
            let wire = community_namespace_sql_request(namespace_request(operation))
                .expect("closed namespace operation must map");
            assert_eq!(wire.database_type, "POSTGRESQL");
            let mapped = wire.operation.expect("wire operation must be present");
            match (index, mapped) {
                (
                    0,
                    wire::build_community_namespace_sql_request::Operation::CreateDatabase(create),
                ) => assert_eq!(
                    create.database.expect("database must be present").name,
                    "inventory"
                ),
                (
                    1,
                    wire::build_community_namespace_sql_request::Operation::AlterDatabase(alter),
                ) => {
                    assert_eq!(
                        alter
                            .old_database
                            .expect("old database must be present")
                            .name,
                        "inventory"
                    );
                    assert_eq!(
                        alter
                            .new_database
                            .expect("new database must be present")
                            .name,
                        "inventory_v2"
                    );
                }
                (2, wire::build_community_namespace_sql_request::Operation::DropDatabase(drop)) => {
                    assert_eq!(drop.database_name, "inventory");
                }
                (
                    3,
                    wire::build_community_namespace_sql_request::Operation::UseDatabase(use_db),
                ) => assert_eq!(use_db.database_name, "inventory"),
                (
                    4,
                    wire::build_community_namespace_sql_request::Operation::CreateSchema(create),
                ) => assert_eq!(create.schema.expect("schema must be present").name, "app"),
                (5, wire::build_community_namespace_sql_request::Operation::AlterSchema(alter)) => {
                    assert_eq!(alter.old_schema_name, "app");
                    assert_eq!(alter.new_schema_name, "app_v2");
                }
                (6, wire::build_community_namespace_sql_request::Operation::DropSchema(drop)) => {
                    assert_eq!(drop.schema_name, "app");
                }
                _ => panic!("namespace operation mapped to the wrong wire variant"),
            }
        }
    }

    #[test]
    fn namespace_request_enforces_identifier_property_and_comment_limits() {
        let exact_identifier = community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::DropDatabase {
                database_name: "x".repeat(MAX_NAMESPACE_IDENTIFIER_BYTES),
            },
        ));
        exact_identifier.expect("exact namespace identifier limit must pass");

        let oversized_identifier = community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::DropDatabase {
                database_name: "x".repeat(MAX_NAMESPACE_IDENTIFIER_BYTES + 1),
            },
        ))
        .expect_err("namespace identifier above the limit must fail");
        assert!(oversized_identifier.to_string().contains("512 UTF-8 bytes"));

        let mut exact_database = namespace_database("inventory");
        exact_database.charset = "x".repeat(MAX_NAMESPACE_PROPERTY_BYTES);
        community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::CreateDatabase {
                database: exact_database,
            },
        ))
        .expect("exact namespace property limit must pass");

        let mut oversized_database = namespace_database("inventory");
        oversized_database.owner = "x".repeat(MAX_NAMESPACE_PROPERTY_BYTES + 1);
        let oversized_property = community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::CreateDatabase {
                database: oversized_database,
            },
        ))
        .expect_err("namespace property above the limit must fail");
        assert!(oversized_property.to_string().contains("4096 UTF-8 bytes"));

        let mut exact_schema = namespace_schema("app");
        exact_schema.comment = "x".repeat(MAX_COMMENT_BYTES);
        community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::CreateSchema {
                schema: exact_schema,
            },
        ))
        .expect("exact namespace comment limit must pass");

        let mut oversized_schema = namespace_schema("app");
        oversized_schema.comment = "x".repeat(MAX_COMMENT_BYTES + 1);
        let oversized_comment = community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::CreateSchema {
                schema: oversized_schema,
            },
        ))
        .expect_err("namespace comment above the limit must fail");
        assert!(oversized_comment.to_string().contains("65536 UTF-8 bytes"));
    }

    #[test]
    fn namespace_request_rejects_unsafe_segments_before_transport() {
        for database_name in ["inventory.public", "inventory; DROP DATABASE inventory"] {
            let error = community_namespace_sql_request(namespace_request(
                CommunityNamespaceSqlOperation::UseDatabase {
                    database_name: database_name.to_owned(),
                },
            ))
            .expect_err("unsafe database segment must fail");
            assert!(error.to_string().contains("unsafe identifier syntax"));
        }

        let mut database = namespace_database("inventory");
        database.charset = "UTF8; DROP DATABASE inventory".to_owned();
        let error = community_namespace_sql_request(namespace_request(
            CommunityNamespaceSqlOperation::CreateDatabase { database },
        ))
        .expect_err("unsafe namespace property must fail");
        assert!(error.to_string().contains("unsafe property syntax"));
    }

    #[test]
    fn dml_request_preserves_typed_values_and_qualified_segments() {
        let columns = vec![
            dml_test_column("nullable", "VARCHAR"),
            dml_test_column("label", "VARCHAR"),
            dml_test_column("amount", "DECIMAL"),
            dml_test_column("active", "BOOLEAN"),
            dml_test_column("created_at", "TIMESTAMP"),
            dml_test_column("payload", "VARBINARY"),
        ];
        let values = vec![
            CommunityDmlValue::Null,
            CommunityDmlValue::String(String::new()),
            CommunityDmlValue::Decimal("12.50".to_owned()),
            CommunityDmlValue::Boolean(true),
            CommunityDmlValue::Temporal(CommunityDmlTemporal {
                kind: CommunityDmlTemporalKind::LocalDatetime,
                iso8601: "2026-07-27T12:34:56".to_owned(),
            }),
            CommunityDmlValue::Binary(vec![0, 1, 255]),
        ];
        let request =
            community_dml_request(dml_test_request(CommunityDmlStatement::SingleInsert {
                columns,
                row: CommunityDmlRow { values },
            }))
            .expect("valid typed DML must encode");

        assert_eq!(request.database_type, "H2");
        let target = request.target.expect("target must be present");
        assert_eq!(target.database_name.as_deref(), Some("inventory"));
        assert_eq!(target.schema_name.as_deref(), Some("APP"));
        assert_eq!(target.table_name, "items");
        let wire::build_community_dml_request::Statement::SingleInsert(insert) =
            request.statement.expect("statement must be present")
        else {
            panic!("single insert must retain its wire variant");
        };
        let values = insert.row.expect("row must be present").values;
        assert!(matches!(
            values[0].value,
            Some(wire::community_dml_value::Value::NullValue(_))
        ));
        assert!(matches!(
            values[1].value,
            Some(wire::community_dml_value::Value::StringValue(ref value)) if value.is_empty()
        ));
        assert!(matches!(
            values[2].value,
            Some(wire::community_dml_value::Value::DecimalValue(ref value)) if value == "12.50"
        ));
        assert!(matches!(
            values[3].value,
            Some(wire::community_dml_value::Value::BooleanValue(true))
        ));
        assert!(matches!(
            values[4].value,
            Some(wire::community_dml_value::Value::TemporalValue(ref value))
                if value.kind == wire::CommunityDmlTemporalKind::LocalDatetime as i32
                    && value.iso8601 == "2026-07-27T12:34:56"
        ));
        assert!(matches!(
            values[5].value,
            Some(wire::community_dml_value::Value::BinaryValue(ref value))
                if value == &[0, 1, 255]
        ));
    }

    #[test]
    fn dml_request_rejects_duplicate_columns_unsafe_identifiers_and_row_widths() {
        let duplicate =
            community_dml_request(dml_test_request(CommunityDmlStatement::SingleInsert {
                columns: vec![
                    dml_test_column("id", "BIGINT"),
                    dml_test_column("id", "BIGINT"),
                ],
                row: CommunityDmlRow {
                    values: vec![
                        CommunityDmlValue::Decimal("1".to_owned()),
                        CommunityDmlValue::Decimal("2".to_owned()),
                    ],
                },
            }))
            .expect_err("duplicate insert columns must fail");
        assert!(duplicate.to_string().contains("duplicate column names"));

        let mut unsafe_target = dml_test_request(CommunityDmlStatement::SingleInsert {
            columns: vec![dml_test_column("id", "BIGINT")],
            row: CommunityDmlRow {
                values: vec![CommunityDmlValue::Decimal("1".to_owned())],
            },
        });
        unsafe_target.target.table_name = "APP.items".to_owned();
        let unsafe_identifier =
            community_dml_request(unsafe_target).expect_err("qualified raw table names must fail");
        assert!(
            unsafe_identifier
                .to_string()
                .contains("unsafe identifier syntax")
        );

        let wrong_width =
            community_dml_request(dml_test_request(CommunityDmlStatement::MultiInsert {
                columns: vec![
                    dml_test_column("id", "BIGINT"),
                    dml_test_column("label", "VARCHAR"),
                ],
                rows: vec![CommunityDmlRow {
                    values: vec![CommunityDmlValue::Decimal("1".to_owned())],
                }],
            }))
            .expect_err("batch row width mismatch must fail");
        assert!(wrong_width.to_string().contains("row width"));
    }

    #[test]
    fn dml_update_requires_nonempty_nonnull_equality_predicates() {
        let assignment = CommunityDmlAssignment {
            column: dml_test_column("label", "VARCHAR"),
            value: CommunityDmlValue::String("next".to_owned()),
        };
        let empty = community_dml_request(dml_test_request(CommunityDmlStatement::Update {
            assignments: vec![assignment.clone()],
            predicates: Vec::new(),
        }))
        .expect_err("an update without predicates must fail");
        assert!(empty.to_string().contains("update predicates count"));

        let null = community_dml_request(dml_test_request(CommunityDmlStatement::Update {
            assignments: vec![assignment],
            predicates: vec![CommunityDmlAssignment {
                column: dml_test_column("id", "BIGINT"),
                value: CommunityDmlValue::Null,
            }],
        }))
        .expect_err("a null equality predicate must fail");
        assert!(null.to_string().contains("cannot compare NULL"));
    }

    #[test]
    fn dml_request_enforces_the_encoded_eight_megabyte_budget() {
        let columns = (0..33)
            .map(|index| dml_test_column(&format!("value_{index}"), "VARCHAR"))
            .collect::<Vec<_>>();
        let values = (0..columns.len())
            .map(|_| CommunityDmlValue::String("x".repeat(MAX_DML_VALUE_BYTES)))
            .collect();
        let error = community_dml_request(dml_test_request(CommunityDmlStatement::SingleInsert {
            columns,
            row: CommunityDmlRow { values },
        }))
        .expect_err("encoded DML above eight MiB must fail");
        assert!(error.to_string().contains("8388608 encoded bytes"));
    }

    #[test]
    fn sql_formatter_complexity_rejects_token_dense_input_at_the_shared_limit() {
        let exact = "a,".repeat(MAX_SQL_FORMATTER_COMPLEXITY_UNITS / 2);
        validate_sql_formatter_complexity(&exact).expect("the exact complexity limit must pass");

        let error = validate_sql_formatter_complexity(&(exact + "a"))
            .expect_err("one unit above the complexity limit must fail");
        assert!(error.to_string().contains("16384 units"));
        validate_sql_formatter_complexity(&"a".repeat(1_048_576))
            .expect("a long single token must retain the independent one MiB byte limit");
    }

    #[test]
    fn formatted_sql_wire_mapping_preserves_sql() {
        assert_eq!(
            CommunityFormattedSql::from(wire::CommunityFormattedSql {
                sql: "SELECT\n  1;".to_owned(),
            }),
            CommunityFormattedSql {
                sql: "SELECT\n  1;".to_owned(),
            }
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn sql_completion_wire_mapping_preserves_every_field() {
        let completion = wire::CommunitySqlCompletion {
            status: "SUCCESS".to_owned(),
            replace_start_utf16: 2,
            replace_end_utf16: 4,
            candidates: vec![wire::CommunitySqlCompletionCandidate {
                id: Some("column:id".to_owned()),
                label: "id".to_owned(),
                r#type: "COLUMN".to_owned(),
                insert_text: Some("c.id".to_owned()),
                insert_type: "PLAIN_TEXT".to_owned(),
                replace_start_utf16: Some(2),
                replace_end_utf16: Some(4),
                detail: Some("APP.CUSTOMER.id".to_owned()),
                description: Some("customer identifier".to_owned()),
                data_type: Some("BIGINT".to_owned()),
                object_type: Some("TABLE".to_owned()),
                comment: Some("primary key".to_owned()),
                datasource_name: Some("local".to_owned()),
                database_name: Some("inventory".to_owned()),
                schema_name: Some("APP".to_owned()),
                table_name: Some("CUSTOMER".to_owned()),
                table_alias: Some("c".to_owned()),
                column_name: Some("id".to_owned()),
                object_name: Some("CUSTOMER".to_owned()),
                parameter_mode: Some("IN".to_owned()),
                sort_rank: Some(1),
                sort_text: Some("0001".to_owned()),
                snippet_slots: vec!["INSERT_COLUMN_LIST".to_owned()],
            }],
            editor_hints: vec![wire::CommunitySqlCompletionEditorHint {
                r#type: "INSERT_VALUE".to_owned(),
                statement_range: Some(wire::CommunitySqlCompletionRange {
                    start_line_number: 1,
                    start_column: 1,
                    end_line_number: 1,
                    end_column: 5,
                }),
                row_range: None,
                value_range: None,
                items: vec![wire::CommunitySqlCompletionEditorHintItem {
                    row_index: 0,
                    column_index: 1,
                    field_name: Some("id".to_owned()),
                    field_type: Some("BIGINT".to_owned()),
                    label: Some("identifier".to_owned()),
                    range: None,
                    active: true,
                }],
            }],
            reason_code: Some("ok".to_owned()),
        };

        assert_eq!(
            CommunitySqlCompletion::from(completion),
            CommunitySqlCompletion {
                status: "SUCCESS".to_owned(),
                replace_start_utf16: 2,
                replace_end_utf16: 4,
                candidates: vec![CommunitySqlCompletionCandidate {
                    id: Some("column:id".to_owned()),
                    label: "id".to_owned(),
                    candidate_type: "COLUMN".to_owned(),
                    insert_text: Some("c.id".to_owned()),
                    insert_type: "PLAIN_TEXT".to_owned(),
                    replace_start_utf16: Some(2),
                    replace_end_utf16: Some(4),
                    detail: Some("APP.CUSTOMER.id".to_owned()),
                    description: Some("customer identifier".to_owned()),
                    data_type: Some("BIGINT".to_owned()),
                    object_type: Some("TABLE".to_owned()),
                    comment: Some("primary key".to_owned()),
                    datasource_name: Some("local".to_owned()),
                    database_name: Some("inventory".to_owned()),
                    schema_name: Some("APP".to_owned()),
                    table_name: Some("CUSTOMER".to_owned()),
                    table_alias: Some("c".to_owned()),
                    column_name: Some("id".to_owned()),
                    object_name: Some("CUSTOMER".to_owned()),
                    parameter_mode: Some("IN".to_owned()),
                    sort_rank: Some(1),
                    sort_text: Some("0001".to_owned()),
                    snippet_slots: vec!["INSERT_COLUMN_LIST".to_owned()],
                }],
                editor_hints: vec![CommunitySqlCompletionEditorHint {
                    hint_type: "INSERT_VALUE".to_owned(),
                    statement_range: Some(CommunitySqlCompletionRange {
                        start_line_number: 1,
                        start_column: 1,
                        end_line_number: 1,
                        end_column: 5,
                    }),
                    row_range: None,
                    value_range: None,
                    items: vec![CommunitySqlCompletionEditorHintItem {
                        row_index: 0,
                        column_index: 1,
                        field_name: Some("id".to_owned()),
                        field_type: Some("BIGINT".to_owned()),
                        label: Some("identifier".to_owned()),
                        range: None,
                        active: true,
                    }],
                }],
                reason_code: Some("ok".to_owned()),
            }
        );
    }

    #[test]
    fn sql_completion_validates_utf16_offsets_and_editor_ranges() {
        let layout = SqlUtf16Layout::new("a\u{1f600}\n中")
            .expect("bounded SQL must have a representable UTF-16 layout");
        assert_eq!(layout.length, 5);
        assert_eq!(layout.line_lengths, vec![3, 1]);
        let mut completion = wire::CommunitySqlCompletion {
            status: "SUCCESS".to_owned(),
            replace_start_utf16: 1,
            replace_end_utf16: 3,
            candidates: vec![wire::CommunitySqlCompletionCandidate {
                replace_start_utf16: Some(1),
                replace_end_utf16: Some(3),
                ..Default::default()
            }],
            editor_hints: vec![wire::CommunitySqlCompletionEditorHint {
                statement_range: Some(wire::CommunitySqlCompletionRange {
                    start_line_number: 1,
                    start_column: 2,
                    end_line_number: 2,
                    end_column: 2,
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        validate_completion_for_sql(&completion, &layout)
            .expect("UTF-16 offsets and one-based editor range must pass");

        completion.replace_end_utf16 = 6;
        assert!(
            validate_completion_for_sql(&completion, &layout)
                .expect_err("offset beyond UTF-16 SQL length must fail")
                .contains("UTF-16 SQL length")
        );
        completion.replace_end_utf16 = 3;
        completion.editor_hints[0]
            .statement_range
            .as_mut()
            .expect("test range must exist")
            .end_column = 3;
        assert!(
            validate_completion_for_sql(&completion, &layout)
                .expect_err("column beyond the UTF-16 line boundary must fail")
                .contains("line boundary")
        );
    }

    #[test]
    fn sql_completion_normalizes_options_and_allocates_nonzero_monotonic_scopes() {
        assert_eq!(normalize_keyword_case(" upper ").unwrap(), "UPPER");
        assert_eq!(normalize_keyword_case("Lower").unwrap(), "LOWER");
        assert!(normalize_keyword_case("title").is_err());
        validate_completion_prefix_length(MAX_SQL_COMPLETION_PREFIX_LENGTH)
            .expect("exact prefix limit must pass");
        assert!(validate_completion_prefix_length(MAX_SQL_COMPLETION_PREFIX_LENGTH + 1).is_err());

        let first = next_sql_completion_datasource_scope().expect("scope must allocate");
        let second = next_sql_completion_datasource_scope().expect("next scope must allocate");
        assert_ne!(first, 0);
        assert_eq!(second, first + 1);
    }

    #[test]
    fn sql_completion_scope_never_exceeds_a_positive_java_long() {
        let maximum = i64::MAX.unsigned_abs();
        assert_eq!(next_java_long_scope(maximum), Some(maximum + 1));
        assert_eq!(next_java_long_scope(maximum + 1), None);
        assert_eq!(next_java_long_scope(u64::MAX), None);
    }

    #[test]
    fn sql_validation_wire_mapping_preserves_every_field() {
        let validation = wire::CommunitySqlValidation {
            valid: false,
            statements: vec![wire::CommunityParsedStatement {
                sql: "SELECT FROM".to_owned(),
                r#type: "Unknown".to_owned(),
                statement_type: "UNKNOWN".to_owned(),
            }],
            diagnostics: vec![wire::CommunitySqlDiagnostic {
                start_line: 1,
                start_column: 8,
                end_line: 1,
                end_column: 12,
                token_text: "FROM".to_owned(),
                message: "unexpected FROM".to_owned(),
            }],
        };

        assert_eq!(
            CommunitySqlValidation::from(validation),
            CommunitySqlValidation {
                valid: false,
                statements: vec![super::CommunityParsedStatement {
                    sql: "SELECT FROM".to_owned(),
                    statement_type: "UNKNOWN".to_owned(),
                    kind: "Unknown".to_owned(),
                }],
                diagnostics: vec![CommunitySqlDiagnostic {
                    start_line: 1,
                    start_column: 8,
                    end_line: 1,
                    end_column: 12,
                    token_text: "FROM".to_owned(),
                    message: "unexpected FROM".to_owned(),
                }],
            }
        );
    }

    #[test]
    fn relation_metadata_wire_mapping_preserves_every_field() {
        let foreign_key = wire::CommunityForeignKey {
            primary_table_database: "inventory".to_owned(),
            primary_table_schema: "APP".to_owned(),
            primary_table_name: "parent".to_owned(),
            primary_column_name: "id".to_owned(),
            foreign_table_database: "inventory".to_owned(),
            foreign_table_schema: "APP".to_owned(),
            foreign_table_name: "child".to_owned(),
            foreign_column_name: "parent_id".to_owned(),
            key_sequence: 2,
            update_rule: 3,
            delete_rule: 4,
            foreign_key_name: "fk_child_parent".to_owned(),
            primary_key_name: "pk_parent".to_owned(),
            deferrability: 5,
        };
        assert_eq!(
            CommunityForeignKey::from(foreign_key),
            CommunityForeignKey {
                primary_table_database: "inventory".to_owned(),
                primary_table_schema: "APP".to_owned(),
                primary_table_name: "parent".to_owned(),
                primary_column_name: "id".to_owned(),
                foreign_table_database: "inventory".to_owned(),
                foreign_table_schema: "APP".to_owned(),
                foreign_table_name: "child".to_owned(),
                foreign_column_name: "parent_id".to_owned(),
                key_sequence: 2,
                update_rule: 3,
                delete_rule: 4,
                foreign_key_name: "fk_child_parent".to_owned(),
                primary_key_name: "pk_parent".to_owned(),
                deferrability: 5,
            }
        );

        let primary_key = wire::CommunityPrimaryKey {
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "parent".to_owned(),
            column_name: "id".to_owned(),
            name: "pk_parent".to_owned(),
        };
        assert_eq!(
            CommunityPrimaryKey::from(primary_key),
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
    fn programmability_metadata_wire_mapping_preserves_every_field() {
        assert_eq!(
            CommunityFunction::from(wire::CommunityFunction {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "total_stock".to_owned(),
                remarks: "stock total".to_owned(),
                function_type: Some(1),
                specific_name: "total_stock_1".to_owned(),
                body: "RETURN 1".to_owned(),
                template: "total_stock(?)".to_owned(),
            }),
            CommunityFunction {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "total_stock".to_owned(),
                remarks: "stock total".to_owned(),
                function_type: Some(1),
                specific_name: "total_stock_1".to_owned(),
                body: "RETURN 1".to_owned(),
                template: "total_stock(?)".to_owned(),
            }
        );

        assert_eq!(
            CommunityFunctionParameter::from(wire::CommunityFunctionParameter {
                function_database: "inventory".to_owned(),
                function_schema: "APP".to_owned(),
                function_name: "total_stock".to_owned(),
                column_name: "warehouse".to_owned(),
                column_type: Some(2),
                data_type: Some(12),
                type_name: "VARCHAR".to_owned(),
                precision: Some(255),
                length: Some(255),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "warehouse name".to_owned(),
                char_octet_length: Some(255),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "total_stock_1".to_owned(),
            }),
            CommunityFunctionParameter {
                function_database: "inventory".to_owned(),
                function_schema: "APP".to_owned(),
                function_name: "total_stock".to_owned(),
                column_name: "warehouse".to_owned(),
                column_type: Some(2),
                data_type: Some(12),
                type_name: "VARCHAR".to_owned(),
                precision: Some(255),
                length: Some(255),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "warehouse name".to_owned(),
                char_octet_length: Some(255),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "total_stock_1".to_owned(),
            }
        );

        assert_eq!(
            CommunityProcedure::from(wire::CommunityProcedure {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "refresh_stock".to_owned(),
                remarks: "refresh totals".to_owned(),
                procedure_type: Some(2),
                specific_name: "refresh_stock_1".to_owned(),
                body: "BEGIN SELECT 1; END".to_owned(),
            }),
            CommunityProcedure {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "refresh_stock".to_owned(),
                remarks: "refresh totals".to_owned(),
                procedure_type: Some(2),
                specific_name: "refresh_stock_1".to_owned(),
                body: "BEGIN SELECT 1; END".to_owned(),
            }
        );

        assert_eq!(
            CommunityProcedureParameter::from(wire::CommunityProcedureParameter {
                procedure_database: "inventory".to_owned(),
                procedure_schema: "APP".to_owned(),
                procedure_name: "refresh_stock".to_owned(),
                column_name: "warehouse".to_owned(),
                column_type: Some(1),
                data_type: Some(12),
                type_name: "VARCHAR".to_owned(),
                precision: Some(255),
                length: Some(255),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "warehouse name".to_owned(),
                column_default: "'all'".to_owned(),
                sql_data_type: Some(12),
                sql_datetime_sub: Some(0),
                char_octet_length: Some(255),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "refresh_stock_1".to_owned(),
            }),
            CommunityProcedureParameter {
                procedure_database: "inventory".to_owned(),
                procedure_schema: "APP".to_owned(),
                procedure_name: "refresh_stock".to_owned(),
                column_name: "warehouse".to_owned(),
                column_type: Some(1),
                data_type: Some(12),
                type_name: "VARCHAR".to_owned(),
                precision: Some(255),
                length: Some(255),
                scale: Some(0),
                radix: Some(10),
                nullable: Some(1),
                remarks: "warehouse name".to_owned(),
                column_default: "'all'".to_owned(),
                sql_data_type: Some(12),
                sql_datetime_sub: Some(0),
                char_octet_length: Some(255),
                ordinal_position: Some(1),
                is_nullable: "YES".to_owned(),
                specific_name: "refresh_stock_1".to_owned(),
            }
        );

        assert_eq!(
            CommunityTrigger::from(wire::CommunityTrigger {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "audit_stock".to_owned(),
                event_manipulation: "UPDATE".to_owned(),
                body: "INSERT INTO audit_log VALUES (1)".to_owned(),
            }),
            CommunityTrigger {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                name: "audit_stock".to_owned(),
                event_manipulation: "UPDATE".to_owned(),
                body: "INSERT INTO audit_log VALUES (1)".to_owned(),
            }
        );
    }

    #[test]
    fn programmability_identifiers_require_nonblank_bounded_utf8() {
        let blank = validate_non_blank_utf8(" \t", MAX_SCALAR_BYTES, "function name")
            .expect_err("blank routine identifiers must fail");
        assert!(blank.to_string().contains("function name is required"));

        let oversized = "x".repeat(MAX_SCALAR_BYTES + 1);
        let error = validate_non_blank_utf8(&oversized, MAX_SCALAR_BYTES, "procedure name")
            .expect_err("oversized routine identifiers must fail");
        assert!(
            error
                .to_string()
                .contains(&format!("cannot exceed {MAX_SCALAR_BYTES} UTF-8 bytes"))
        );
    }

    fn temp_file(name: &str, extension: &str) -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("test directory must exist");
        let path = directory.path().join(format!("{name}.{extension}"));
        fs::write(&path, b"fixture").expect("fixture JAR must write");
        (directory, path)
    }

    fn locked_directory() -> (TempDir, String) {
        let directory = tempfile::tempdir().expect("test directory must exist");
        let artifacts = [
            ("first.jar", b"first".as_slice()),
            ("second.jar", b"second"),
        ];
        let mut lock = format!(
            "format_version\t1\nsource_commit\t{COMMIT}\nartifact_count\t{}\n",
            artifacts.len()
        );
        for (file_name, contents) in artifacts {
            fs::write(directory.path().join(file_name), contents)
                .expect("locked fixture must write");
            let digest = Sha256::digest(contents);
            let mut digest_hex = String::with_capacity(64);
            for byte in digest {
                write!(&mut digest_hex, "{byte:02x}").expect("digest formatting cannot fail");
            }
            writeln!(
                &mut lock,
                "artifact\t{file_name}\t{digest_hex}\t{}",
                contents.len()
            )
            .expect("lock formatting cannot fail");
        }
        (directory, lock)
    }

    #[test]
    fn classpath_is_canonical_and_deterministic() {
        let (_second_directory, second) = temp_file("second", "jar");
        let (_first_directory, first) = temp_file("first", "jar");
        let classpath = CommunityClasspath::from_paths(COMMIT, [second.clone(), first.clone()])
            .expect("valid classpath must load");

        let artifacts = classpath.artifacts().collect::<Vec<_>>();
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts[0] < artifacts[1]);
        assert!(artifacts.iter().all(|path| path.is_absolute()));
    }

    #[test]
    fn locked_classpath_requires_the_exact_artifact_set() {
        let (directory, lock) = locked_directory();
        let classpath = CommunityClasspath::from_locked_directory(directory.path(), &lock)
            .expect("matching lock must load");
        assert_eq!(classpath.source_commit(), COMMIT);
        assert_eq!(classpath.artifacts().len(), 2);

        fs::write(directory.path().join("extra.jar"), b"extra").expect("extra fixture must write");
        assert!(CommunityClasspath::from_locked_directory(directory.path(), &lock).is_err());
    }

    #[test]
    fn locked_classpath_rejects_digest_and_length_drift() {
        let (directory, lock) = locked_directory();
        fs::write(directory.path().join("first.jar"), b"changed").expect("fixture must change");
        assert!(CommunityClasspath::from_locked_directory(directory.path(), &lock).is_err());
    }

    #[test]
    fn locked_classpath_rejects_unsafe_or_malformed_rows() {
        let (directory, lock) = locked_directory();
        for invalid in [
            lock.replace("first.jar", "../first.jar"),
            lock.replace("artifact_count\t2", "artifact_count\t3"),
            lock.replace("format_version\t1", "format_version\t2"),
        ] {
            assert!(CommunityClasspath::from_locked_directory(directory.path(), &invalid).is_err());
        }
    }

    #[test]
    fn classpath_rejects_non_full_or_uppercase_commits() {
        let (_directory, jar) = temp_file("commit", "jar");
        assert!(CommunityClasspath::from_paths("abc", [jar.clone()]).is_err());
        assert!(
            CommunityClasspath::from_paths("0123456789ABCDEF0123456789ABCDEF01234567", [jar],)
                .is_err()
        );
    }

    #[test]
    fn classpath_rejects_duplicate_artifacts() {
        let (_directory, jar) = temp_file("duplicate", "jar");
        assert!(CommunityClasspath::from_paths(COMMIT, [jar.clone(), jar]).is_err());
    }

    #[test]
    fn classpath_rejects_empty_and_non_jar_inputs() {
        assert!(CommunityClasspath::from_paths(COMMIT, Vec::<PathBuf>::new()).is_err());
        let (_directory, text) = temp_file("not-a-jar", "txt");
        assert!(CommunityClasspath::from_paths(COMMIT, [text]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn classpath_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let (directory, jar) = temp_file("target", "jar");
        let link = directory.path().join("linked.jar");
        symlink(jar, &link).expect("fixture symbolic link must exist");
        assert!(CommunityClasspath::from_paths(COMMIT, [link]).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn snapshot_rejects_an_artifact_replaced_by_a_file_symlink() {
        use std::os::windows::fs::symlink_file;

        let directory = tempfile::tempdir().expect("test directory must exist");
        let jar = directory.path().join("validated.jar");
        let replacement = directory.path().join("replacement.jar");
        fs::write(&jar, b"validated").expect("fixture JAR must write");
        fs::write(&replacement, b"replacement").expect("replacement JAR must write");
        let classpath = CommunityClasspath::from_paths(COMMIT, [jar.clone()])
            .expect("initial artifact must validate");

        fs::remove_file(&jar).expect("validated artifact must be replaceable");
        symlink_file(&replacement, &jar).expect("replacement file symlink must be created");
        let generation = tempfile::tempdir().expect("generation directory must exist");

        assert!(classpath.snapshot_into(generation.path()).is_err());
    }

    #[test]
    fn snapshot_rejects_an_artifact_changed_after_validation() {
        let (_directory, jar) = temp_file("mutable", "jar");
        let classpath = CommunityClasspath::from_paths(COMMIT, [jar.clone()])
            .expect("initial artifact must validate");
        fs::write(&jar, b"changed fixture").expect("fixture must change");
        let generation = tempfile::tempdir().expect("generation directory must exist");

        assert!(classpath.snapshot_into(generation.path()).is_err());
    }
}
