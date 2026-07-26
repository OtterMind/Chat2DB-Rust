use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use chat2db_engine_protocol::wire;
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

pub(super) const COMMUNITY_CLASSPATH_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
pub(super) const COMMUNITY_SOURCE_COMMIT_ENV: &str = "CHAT2DB_COMMUNITY_SOURCE_COMMIT";

const MAX_CLASSPATH_ARTIFACTS: usize = wire::CommunityCountLimit::MaxClasspathArtifacts as usize;
const MAX_CLASSPATH_BYTES: u64 = wire::CommunityByteLimit::MaxClasspathBytes as u64;
const MAX_DATABASE_TYPE_BYTES: usize = wire::CommunityByteLimit::MaxDatabaseTypeBytes as usize;
const MAX_SOURCE_COMMIT_BYTES: usize = wire::CommunityByteLimit::MaxSourceCommitBytes as usize;
const MAX_COMMENT_BYTES: usize = wire::CommunityByteLimit::MaxCommentBytes as usize;
const MAX_SQL_FORMATTER_COMPLEXITY_UNITS: usize =
    wire::CommunitySqlFormatterLimit::MaxComplexityUnits as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_PROTOCOL_ID_BYTES: usize = wire::JdbcProtocolLimit::MaxDriverIdBytes as usize;
const MAX_PATH_BYTES: usize = wire::JdbcProtocolLimit::MaxPathBytes as usize;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityPluginServices {
    pub metadata_available: bool,
    pub sql_builder_available: bool,
    pub sql_parser_available: bool,
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
        CommunityClasspath, CommunityForeignKey, CommunityFormattedSql, CommunityFunction,
        CommunityFunctionParameter, CommunityPrimaryKey, CommunityProcedure,
        CommunityProcedureParameter, CommunitySqlDiagnostic, CommunitySqlValidation,
        CommunityTrigger, MAX_SCALAR_BYTES, MAX_SQL_FORMATTER_COMPLEXITY_UNITS,
        validate_non_blank_utf8, validate_sql_formatter_complexity,
    };

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

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
