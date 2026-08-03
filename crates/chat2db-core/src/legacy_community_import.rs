use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use chat2db_contract::{
    CommunityDatasourceExport, DatasourceConnection, DatasourceConnectionProperty,
    PortableCommunityDatasource, PortableDatasourceConnection, PortableDatasourceProperty,
};
use chat2db_java_bridge::{JdbcRow, JdbcValue, QueryEvent, QueryOptions, QueryRequest};
use directories::BaseDirs;
use tempfile::{Builder as TempDirBuilder, TempDir};
use url::Url;

use crate::{
    AppError, Application,
    datasource_edit::sanitize_jdbc_url,
    datasource_session::{ResolvedDatasourceConnection, SessionReadOnly, open_datasource_session},
    now_millis,
};

const H2_DRIVER_CLASS: &str = "org.h2.Driver";
const H2_MIGRATION_DRIVER_PACK_ID: &str = "h2-legacy-migration";
const MAX_LEGACY_DATABASE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_LEGACY_DATASOURCES: usize = 1_000;
const MAX_LEGACY_QUERY_BYTES: u64 = 16 * 1024 * 1024;

/// Summary of one Desktop-only migration from the pre-Community `Chat2DB` H2 store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegacyCommunityImportOutcome {
    pub database_found: bool,
    pub imported: u32,
    pub skipped_unsupported: u32,
    pub password_fields_omitted: u32,
    pub other_sensitive_fields_omitted: u32,
}

struct LegacyDatabaseSnapshot {
    _directory: TempDir,
    jdbc_base_path: PathBuf,
}

struct LegacyRows {
    labels: HashMap<String, usize>,
    rows: Vec<JdbcRow>,
}

impl Application {
    /// Imports native `MySQL` datasource definitions from the old `~/.chat2db` H2 database.
    ///
    /// The old database is copied to a private snapshot before Java starts. Passwords and other
    /// sensitive legacy fields are intentionally omitted and must be supplied again by the user.
    ///
    /// # Errors
    ///
    /// Returns discovery, snapshot, H2 driver, engine, query, validation, or storage failures.
    pub async fn import_legacy_community_datasources(
        &self,
    ) -> Result<LegacyCommunityImportOutcome, AppError> {
        let Some(base_dirs) = BaseDirs::new() else {
            return Err(AppError::unavailable(
                "legacy_community_home_unavailable",
                "The legacy Chat2DB home directory could not be resolved",
            ));
        };
        let database_file = require_legacy_database(base_dirs.home_dir())?;
        self.import_legacy_community_datasources_from_file(&database_file)
            .await
    }

    #[doc(hidden)]
    pub async fn import_legacy_community_datasources_from_file(
        &self,
        database_file: &Path,
    ) -> Result<LegacyCommunityImportOutcome, AppError> {
        let snapshot = tokio::task::spawn_blocking({
            let database_file = database_file.to_path_buf();
            move || snapshot_legacy_database(&database_file)
        })
        .await
        .map_err(|_| AppError::internal())??;
        let h2_driver = self
            .list_drivers()
            .items
            .into_iter()
            .find(|driver| driver.pack_id == H2_MIGRATION_DRIVER_PACK_ID)
            .ok_or_else(|| {
                AppError::unavailable(
                    "legacy_community_h2_driver_missing",
                    "The bundled H2 migration driver is not installed",
                )
            })?;
        if h2_driver.driver_class != H2_DRIVER_CLASS {
            return Err(AppError::unavailable(
                "legacy_community_h2_driver_invalid",
                "The bundled H2 migration driver has an unexpected driver class",
            ));
        }
        let jdbc_url = snapshot_jdbc_url(&snapshot.jdbc_base_path)?;
        let engine = self.require_engine().await?;
        let session = open_datasource_session(
            &engine,
            ResolvedDatasourceConnection {
                datasource_id: "legacy-community-import".to_owned(),
                datasource_revision: 0,
                driver_id: h2_driver.driver_id,
                datasource_name: "Legacy Chat2DB migration snapshot".to_owned(),
                connection: DatasourceConnection {
                    jdbc_url,
                    properties: vec![DatasourceConnectionProperty {
                        key: "user".to_owned(),
                        value: "sa".to_owned(),
                        sensitive: false,
                    }],
                    read_only: true,
                    ssh: None,
                },
            },
            SessionReadOnly::Forced,
        )
        .await?;
        let queried = query_legacy_datasources(&session).await;
        let closed = session.close().await.map_err(AppError::from);
        let rows = match (queried, closed) {
            (Ok(rows), Ok(())) => rows,
            (Err(error), _) | (Ok(_), Err(error)) => return Err(error),
        };
        drop(snapshot);

        let (mut outcome, datasources) = convert_legacy_rows(rows)?;
        let imported = self
            .import_community_datasources(CommunityDatasourceExport {
                schema_version: 1,
                exported_at_ms: now_millis()?.to_string(),
                datasources,
            })
            .await?;
        if imported.count == 0 {
            return Err(AppError::unavailable(
                "legacy_community_mysql_not_imported",
                "No legacy MySQL datasource was imported",
            ));
        }
        outcome.imported = imported.count;
        Ok(outcome)
    }
}

fn convert_legacy_rows(
    rows: LegacyRows,
) -> Result<
    (
        LegacyCommunityImportOutcome,
        Vec<PortableCommunityDatasource>,
    ),
    AppError,
> {
    let mut outcome = LegacyCommunityImportOutcome {
        database_found: true,
        ..LegacyCommunityImportOutcome::default()
    };
    let mut datasources = Vec::new();
    for row in rows.rows {
        if !text_field(&rows.labels, &row, "type")?
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("mysql"))
        {
            outcome.skipped_unsupported = outcome.skipped_unsupported.saturating_add(1);
            continue;
        }
        if nonempty_field(&rows.labels, &row, "password")? {
            outcome.password_fields_omitted = outcome.password_fields_omitted.saturating_add(1);
        }
        if ["ssh", "ssl", "driver_config", "extend_info"]
            .into_iter()
            .any(|field| nonempty_field(&rows.labels, &row, field).unwrap_or(true))
        {
            outcome.other_sensitive_fields_omitted =
                outcome.other_sensitive_fields_omitted.saturating_add(1);
        }
        datasources.push(portable_mysql_datasource(&rows.labels, &row)?);
    }
    if datasources.is_empty() {
        return Err(AppError::not_found(
            "legacy_community_mysql_not_found",
            "The legacy Chat2DB database contains no compatible MySQL datasources",
        ));
    }
    Ok((outcome, datasources))
}

fn require_legacy_database(home: &Path) -> Result<PathBuf, AppError> {
    find_legacy_database(home)?.ok_or_else(|| {
        AppError::not_found(
            "legacy_community_database_not_found",
            "The legacy Chat2DB datasource database does not exist",
        )
    })
}

fn find_legacy_database(home: &Path) -> Result<Option<PathBuf>, AppError> {
    let candidate = home.join(".chat2db").join("db").join("chat2db.mv.db");
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(AppError::invalid(
                "unsafe_legacy_community_database",
                "The legacy Chat2DB database path is not a regular file",
            ))
        }
        Ok(_) => Ok(Some(candidate)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(snapshot_error(&error)),
    }
}

fn snapshot_legacy_database(source_path: &Path) -> Result<LegacyDatabaseSnapshot, AppError> {
    let mut source =
        open_regular_file_no_follow(source_path).map_err(|error| snapshot_error(&error))?;
    let source_length = source
        .metadata()
        .map_err(|error| snapshot_error(&error))?
        .len();
    if source_length > MAX_LEGACY_DATABASE_BYTES {
        return Err(AppError::invalid(
            "legacy_community_database_too_large",
            "The legacy Chat2DB database exceeds the migration size limit",
        ));
    }
    let directory = TempDirBuilder::new()
        .prefix("chat2db-legacy-import-")
        .tempdir()
        .map_err(|error| snapshot_error(&error))?;
    let source_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            AppError::invalid(
                "invalid_legacy_community_database",
                "The legacy Chat2DB database filename is invalid",
            )
        })?;
    if !source_name.ends_with(".mv.db") {
        return Err(AppError::invalid(
            "legacy_community_database_format_unsupported",
            "Only the Community H2 MVStore database format can be migrated",
        ));
    }
    let snapshot_path = directory.path().join("chat2db.mv.db");
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&snapshot_path)
        .map_err(|error| snapshot_error(&error))?;
    let copied = io::copy(
        &mut std::io::Read::by_ref(&mut source).take(MAX_LEGACY_DATABASE_BYTES.saturating_add(1)),
        &mut output,
    )
    .map_err(|error| snapshot_error(&error))?;
    if copied != source_length || copied > MAX_LEGACY_DATABASE_BYTES {
        return Err(AppError::unavailable(
            "legacy_community_snapshot_changed",
            "The legacy Chat2DB database changed while its migration snapshot was created",
        ));
    }
    output.flush().map_err(|error| snapshot_error(&error))?;
    output.sync_all().map_err(|error| snapshot_error(&error))?;
    Ok(LegacyDatabaseSnapshot {
        jdbc_base_path: directory.path().join("chat2db"),
        _directory: directory,
    })
}

fn open_regular_file_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        let flags = i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits())
            .map_err(|_| io::Error::other("legacy database open flags are not representable"))?;
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
            "legacy database path is not a regular file",
        ));
    }
    Ok(file)
}

fn snapshot_jdbc_url(base_path: &Path) -> Result<String, AppError> {
    let path = base_path.to_str().ok_or_else(|| {
        AppError::invalid(
            "invalid_legacy_community_database",
            "The migration snapshot path is not valid UTF-8",
        )
    })?;
    if path.contains([';', '\0']) {
        return Err(AppError::invalid(
            "invalid_legacy_community_database",
            "The migration snapshot path cannot be represented as an H2 URL",
        ));
    }
    Ok(format!(
        "jdbc:h2:file:{};ACCESS_MODE_DATA=r;IFEXISTS=TRUE;MODE=MYSQL;FILE_LOCK=NO",
        path.replace('\\', "/")
    ))
}

async fn query_legacy_datasources(
    session: &chat2db_java_bridge::Session,
) -> Result<LegacyRows, AppError> {
    let mut stream = session
        .execute_query(QueryRequest {
            sql: "SELECT * FROM DATA_SOURCE".to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
            options: QueryOptions {
                max_rows: u64::try_from(MAX_LEGACY_DATASOURCES + 1)
                    .map_err(|_| AppError::internal())?,
                target_batch_rows: 250,
                target_batch_bytes: 256 * 1024,
                initial_batch_credits: 8,
                max_result_bytes: MAX_LEGACY_QUERY_BYTES,
            },
        })
        .await
        .map_err(|error| import_query_error(&error))?;
    let mut labels = None;
    let mut rows = Vec::new();
    let mut completed = false;
    while let Some(event) = stream
        .next_event()
        .await
        .map_err(|error| import_query_error(&error))?
    {
        match event {
            QueryEvent::Started(started) => {
                if labels.is_some() {
                    return Err(AppError::internal());
                }
                labels = Some(
                    started
                        .columns
                        .into_iter()
                        .enumerate()
                        .map(|(index, column)| (column.label.to_ascii_lowercase(), index))
                        .collect(),
                );
            }
            QueryEvent::Batch(batch) => rows.extend(batch.rows),
            QueryEvent::Completed(result) => {
                if result.truncated_by_max_rows
                    || result.truncated_by_max_result_bytes
                    || rows.len() > MAX_LEGACY_DATASOURCES
                {
                    return Err(AppError::invalid(
                        "legacy_community_datasource_limit_exceeded",
                        "The legacy Chat2DB database contains too many datasource records",
                    ));
                }
                completed = true;
                break;
            }
        }
    }
    if !completed {
        return Err(AppError::unavailable(
            "legacy_community_import_failed",
            "The legacy Chat2DB datasource query ended unexpectedly",
        ));
    }
    Ok(LegacyRows {
        labels: labels.ok_or_else(AppError::internal)?,
        rows,
    })
}

fn portable_mysql_datasource(
    labels: &HashMap<String, usize>,
    row: &JdbcRow,
) -> Result<PortableCommunityDatasource, AppError> {
    let source_id = text_field(labels, row, "id")?;
    let host = text_field(labels, row, "host")?.unwrap_or_default();
    let name = text_field(labels, row, "alias")?
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            if host.trim().is_empty() {
                "Imported MySQL".to_owned()
            } else {
                format!("@{}", host.trim())
            }
        });
    let jdbc_url = legacy_mysql_url(labels, row, &host)?;
    let properties = text_field(labels, row, "user_name")?
        .filter(|user| !user.trim().is_empty())
        .map(|user| {
            vec![PortableDatasourceProperty {
                key: "user".to_owned(),
                value: user,
            }]
        })
        .unwrap_or_default();
    Ok(PortableCommunityDatasource {
        source_id,
        name,
        driver_id: "mysql".to_owned(),
        connection: Some(PortableDatasourceConnection {
            jdbc_url,
            properties,
            read_only: false,
            ssh: None,
        }),
    })
}

fn legacy_mysql_url(
    labels: &HashMap<String, usize>,
    row: &JdbcRow,
    host: &str,
) -> Result<String, AppError> {
    let configured = text_field(labels, row, "url")?
        .filter(|value| !value.trim().is_empty())
        .or(text_field(labels, row, "jdbc")?.filter(|value| !value.trim().is_empty()));
    if let Some(configured) = configured {
        let configured = configured.trim();
        if configured.to_ascii_lowercase().starts_with("jdbc:mysql://") {
            return sanitize_jdbc_url(configured);
        }
        if configured.to_ascii_lowercase().starts_with("mysql://") {
            return sanitize_jdbc_url(&format!("jdbc:{configured}"));
        }
        return Err(AppError::invalid(
            "invalid_legacy_mysql_url",
            "A legacy MySQL datasource contains a non-MySQL URL",
        ));
    }
    if host.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_legacy_mysql_url",
            "A legacy MySQL datasource has neither a URL nor a host",
        ));
    }
    let port = text_field(labels, row, "port")?
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| {
            AppError::invalid(
                "invalid_legacy_mysql_url",
                "A legacy MySQL datasource has an invalid port",
            )
        })?
        .unwrap_or(3306);
    let database = text_field(labels, row, "service_name")?.unwrap_or_default();
    let mut url = Url::parse("mysql://localhost").map_err(|_| AppError::internal())?;
    url.set_host(Some(host.trim())).map_err(|_| {
        AppError::invalid(
            "invalid_legacy_mysql_url",
            "A legacy MySQL datasource has an invalid host",
        )
    })?;
    url.set_port(Some(port))
        .map_err(|()| AppError::internal())?;
    if !database.trim().is_empty() {
        url.set_path(&format!("/{}", database.trim().trim_start_matches('/')));
    }
    Ok(format!("jdbc:{url}"))
}

fn nonempty_field(
    labels: &HashMap<String, usize>,
    row: &JdbcRow,
    field: &str,
) -> Result<bool, AppError> {
    Ok(text_field(labels, row, field)?.is_some_and(|value| !value.trim().is_empty()))
}

fn text_field(
    labels: &HashMap<String, usize>,
    row: &JdbcRow,
    field: &str,
) -> Result<Option<String>, AppError> {
    let Some(index) = labels.get(field).copied() else {
        return Ok(None);
    };
    let value = row.values.get(index).ok_or_else(AppError::internal)?;
    match value {
        JdbcValue::Null => Ok(None),
        JdbcValue::Boolean(value) => Ok(Some(value.to_string())),
        JdbcValue::SignedInteger(value) => Ok(Some(value.to_string())),
        JdbcValue::UnsignedInteger(value) => Ok(Some(value.to_string())),
        JdbcValue::Float32(value) => Ok(Some(value.to_string())),
        JdbcValue::Float64(value) => Ok(Some(value.to_string())),
        JdbcValue::Decimal(value)
        | JdbcValue::Text(value)
        | JdbcValue::Date(value)
        | JdbcValue::Time(value)
        | JdbcValue::Timestamp(value)
        | JdbcValue::TimestampWithTimeZone(value)
        | JdbcValue::Json(value)
        | JdbcValue::Uuid(value) => Ok(Some(value.clone())),
        JdbcValue::Opaque { display_value, .. } => Ok(Some(display_value.clone())),
        JdbcValue::Binary(_) => Err(AppError::invalid(
            "invalid_legacy_community_datasource",
            format!("Legacy datasource field {field} is not text"),
        )),
    }
}

fn snapshot_error(error: &io::Error) -> AppError {
    tracing::warn!(%error, "legacy Chat2DB database snapshot failed");
    AppError::unavailable(
        "legacy_community_snapshot_failed",
        "The legacy Chat2DB database could not be copied for migration",
    )
}

fn import_query_error(error: &chat2db_java_bridge::BridgeError) -> AppError {
    tracing::warn!(%error, "legacy Chat2DB datasource query failed");
    AppError::unavailable(
        "legacy_community_import_failed",
        "The legacy Chat2DB datasource database could not be read",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chat2db_java_bridge::{JdbcRow, JdbcValue};
    use tempfile::TempDir;

    use super::{
        LegacyRows, convert_legacy_rows, find_legacy_database, portable_mysql_datasource,
        require_legacy_database,
    };

    #[cfg(any(unix, windows))]
    #[test]
    fn discovery_is_scoped_to_an_isolated_home_and_rejects_symlinks() {
        let home = TempDir::new().expect("temporary home");
        assert!(
            find_legacy_database(home.path())
                .expect("missing legacy database is valid")
                .is_none()
        );
        let database_dir = home.path().join(".chat2db/db");
        std::fs::create_dir_all(&database_dir).expect("legacy database directory creates");
        let database = database_dir.join("chat2db.mv.db");
        std::fs::write(&database, b"snapshot").expect("legacy database fixture writes");
        assert_eq!(
            find_legacy_database(home.path()).expect("legacy database discovers"),
            Some(database.clone())
        );

        std::fs::remove_file(&database).expect("regular fixture removes");
        let target = home.path().join("outside.mv.db");
        std::fs::write(&target, b"snapshot").expect("symlink target writes");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &database).expect("file symlink creates");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &database).expect("file symlink creates");
        let error = find_legacy_database(home.path()).expect_err("symlink must be rejected");
        assert_eq!(error.api_error().code, "unsafe_legacy_community_database");
    }

    #[test]
    fn missing_legacy_database_is_an_explicit_error() {
        let home = TempDir::new().expect("temporary home");
        let error = require_legacy_database(home.path()).expect_err("missing database must fail");
        assert_eq!(
            error.api_error().code,
            "legacy_community_database_not_found"
        );
    }

    #[test]
    fn legacy_database_without_mysql_is_an_explicit_error() {
        let labels = HashMap::from([("type".to_owned(), 0), ("alias".to_owned(), 1)]);
        let rows = LegacyRows {
            labels,
            rows: vec![JdbcRow {
                values: vec![
                    JdbcValue::Text("POSTGRESQL".to_owned()),
                    JdbcValue::Text("Legacy PostgreSQL".to_owned()),
                ],
            }],
        };
        let error = convert_legacy_rows(rows).expect_err("non-MySQL database must fail");
        assert_eq!(error.api_error().code, "legacy_community_mysql_not_found");
    }

    #[test]
    fn legacy_mysql_conversion_omits_passwords_and_keeps_username() {
        let fields = ["id", "alias", "type", "url", "user_name", "password"];
        let labels = fields
            .into_iter()
            .enumerate()
            .map(|(index, field)| (field.to_owned(), index))
            .collect::<HashMap<_, _>>();
        let row = JdbcRow {
            values: vec![
                JdbcValue::SignedInteger(7),
                JdbcValue::Text("Local MySQL".to_owned()),
                JdbcValue::Text("MYSQL".to_owned()),
                JdbcValue::Text(
                    "jdbc:mysql://embedded:must-not-migrate@127.0.0.1:3306/demo?useSSL=true&password=must-not-migrate"
                        .to_owned(),
                ),
                JdbcValue::Text("developer".to_owned()),
                JdbcValue::Text("must-not-migrate".to_owned()),
            ],
        };
        let converted = portable_mysql_datasource(&labels, &row).expect("row converts");
        let connection = converted.connection.as_ref().expect("connection exists");
        assert_eq!(connection.properties.len(), 1);
        assert_eq!(connection.properties[0].key, "user");
        assert_eq!(connection.properties[0].value, "developer");
        assert_eq!(
            connection.jdbc_url,
            "jdbc:mysql://127.0.0.1:3306/demo?useSSL=true"
        );
        let json = serde_json::to_string(&converted).expect("datasource serializes");
        assert!(!json.contains("must-not-migrate"));
    }
}
