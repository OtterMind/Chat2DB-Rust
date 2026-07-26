use std::{
    collections::HashSet,
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
/// Builds dialect SQL through a Community `ISqlBuilder` implementation.
pub const COMMUNITY_SQL_BUILDER_CAPABILITY: &str = "community.sql-builder.v1";
/// Parses SQL through a Community `ISqlSyntaxPlugin` implementation.
pub const COMMUNITY_SQL_PARSER_CAPABILITY: &str = "community.sql-parser.v1";

pub(super) const COMMUNITY_CLASSPATH_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
pub(super) const COMMUNITY_SOURCE_COMMIT_ENV: &str = "CHAT2DB_COMMUNITY_SOURCE_COMMIT";

const MAX_CLASSPATH_ARTIFACTS: usize = wire::CommunityCountLimit::MaxClasspathArtifacts as usize;
const MAX_CLASSPATH_BYTES: u64 = wire::CommunityByteLimit::MaxClasspathBytes as u64;
const MAX_DATABASE_TYPE_BYTES: usize = wire::CommunityByteLimit::MaxDatabaseTypeBytes as usize;
const MAX_SOURCE_COMMIT_BYTES: usize = wire::CommunityByteLimit::MaxSourceCommitBytes as usize;
const MAX_COMMENT_BYTES: usize = wire::CommunityByteLimit::MaxCommentBytes as usize;
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

/// Validated, immutable Community compatibility classpath for one engine process.
#[derive(Clone, Debug)]
pub struct CommunityClasspath {
    source_commit: String,
    artifacts: Vec<CommunityArtifact>,
}

impl CommunityClasspath {
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
    /// Returns an error when the engine is not ready.
    pub fn community_client(&self) -> Result<CommunityClient, BridgeError> {
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

impl From<wire::CommunitySqlAnalysis> for CommunitySqlAnalysis {
    fn from(analysis: wire::CommunitySqlAnalysis) -> Self {
        Self {
            is_select: analysis.is_select,
            statements: analysis.statements.into_iter().map(Into::into).collect(),
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
    use std::{fs, path::PathBuf};

    use tempfile::TempDir;

    use super::CommunityClasspath;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn temp_file(name: &str, extension: &str) -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("test directory must exist");
        let path = directory.path().join(format!("{name}.{extension}"));
        fs::write(&path, b"fixture").expect("fixture JAR must write");
        (directory, path)
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
