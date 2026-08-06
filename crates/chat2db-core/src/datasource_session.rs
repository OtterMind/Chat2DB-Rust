use chat2db_contract::{ApiError, DatasourceConnection};
use chat2db_java_bridge::{
    ConnectionProperty, EngineClient, JdbcColumn, JdbcParameter, JdbcRow, QueryCompleted,
    QueryEvent, QueryOptions, QueryRequest, Session, SessionConfig,
};
use chat2db_storage::Storage;

use crate::{
    AppError, AppErrorKind, Application,
    datasource_compatibility::{
        jdbc_driver_matches_descriptor, managed_jdbc_driver_for_descriptor,
    },
    driver_not_installed,
    engine_manager::EngineLease,
    native_driver_types::NativeDriverDescriptor,
};

const JDBC_QUERY_BATCH_ROWS: u32 = 128;
const JDBC_QUERY_BATCH_BYTES: u32 = 64 * 1024;

pub(crate) struct ResolvedDatasourceConnection {
    pub(crate) datasource_id: String,
    pub(crate) datasource_revision: u64,
    pub(crate) driver_id: String,
    pub(crate) datasource_name: String,
    pub(crate) connection: DatasourceConnection,
}

#[derive(Clone, Copy)]
pub(crate) enum SessionReadOnly {
    Configured,
    Forced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JdbcQueryLimits {
    pub(crate) max_rows: u64,
    pub(crate) max_result_bytes: u64,
}

#[derive(Debug, PartialEq)]
pub(crate) struct JdbcQueryResult {
    pub(crate) columns: Vec<JdbcColumn>,
    pub(crate) rows: Vec<JdbcRow>,
    pub(crate) completed: QueryCompleted,
}

pub(crate) async fn resolve_datasource_connection(
    storage: &Storage,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = storage.clone();
    let datasource_id = datasource_id.to_owned();
    let (datasource, secret) =
        crate::storage_call(move || storage.get_datasource_with_secret(&datasource_id)).await?;
    let secret = secret.ok_or_else(|| {
        AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "datasource_connection_missing",
                "The datasource has no installed connection descriptor",
            ),
        )
    })?;
    let connection = serde_json::from_slice(secret.expose_secret()).map_err(|_| {
        AppError::new(
            AppErrorKind::Internal,
            ApiError::new(
                "datasource_connection_invalid",
                "The stored datasource connection descriptor is invalid",
            ),
        )
    })?;
    Ok(ResolvedDatasourceConnection {
        datasource_id: datasource.id,
        datasource_revision: datasource.revision,
        driver_id: datasource.driver_id,
        datasource_name: datasource.name,
        connection,
    })
}

pub(crate) async fn open_datasource_session(
    engine: &EngineClient,
    resolved: ResolvedDatasourceConnection,
    read_only: SessionReadOnly,
) -> Result<Session, AppError> {
    let driver = engine.driver_client().map_err(AppError::from)?;
    driver
        .open_session(session_config(resolved, read_only))
        .await
        .map_err(AppError::from)
}

pub(crate) async fn open_mapped_datasource_session(
    application: &Application,
    engine: &EngineClient,
    mut resolved: ResolvedDatasourceConnection,
    read_only: SessionReadOnly,
) -> Result<Session, AppError> {
    resolved.driver_id = resolve_jdbc_driver_id(application, &resolved.driver_id, None)?;
    open_datasource_session(engine, resolved, read_only).await
}

/// Opens and closes a managed JDBC connection while retaining the Java generation lease.
pub(crate) async fn jdbc_test_connection(
    application: &Application,
    driver_id: &str,
    connection: DatasourceConnection,
) -> Result<(), AppError> {
    let resolved = ResolvedDatasourceConnection {
        datasource_id: "connection-test".to_owned(),
        datasource_revision: 0,
        driver_id: driver_id.to_owned(),
        datasource_name: "Connection test".to_owned(),
        connection,
    };
    ManagedJdbcSession::open(application, resolved, SessionReadOnly::Configured, None)
        .await?
        .finish(Ok(()))
        .await
}

/// Executes one bounded, forced-read-only JDBC query for a native SPI driver.
///
/// The native driver owns database-specific SQL and result mapping. This helper owns
/// datasource secret resolution, managed driver selection, Java lifecycle, flow control,
/// and session cleanup without loading any Community plugin.
pub(crate) async fn jdbc_query(
    application: &Application,
    descriptor: &'static NativeDriverDescriptor,
    datasource_id: &str,
    sql: String,
    parameters: Vec<JdbcParameter>,
    limits: JdbcQueryLimits,
) -> Result<JdbcQueryResult, AppError> {
    if limits.max_rows == 0 || limits.max_result_bytes == 0 {
        return Err(AppError::invalid(
            "invalid_jdbc_query_limits",
            "JDBC helper queries require non-zero row and byte limits",
        ));
    }
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    let session = ManagedJdbcSession::open(
        application,
        resolved,
        SessionReadOnly::Forced,
        Some(descriptor),
    )
    .await?;
    session.query(sql, parameters, limits).await
}

/// Normal paths consume this owner through `finish`, which awaits `Session::close`
/// before releasing the engine lease. `Drop` is only a cancellation/panic guard.
struct ManagedJdbcSession {
    session: Option<Session>,
    engine: Option<EngineLease>,
}

impl ManagedJdbcSession {
    async fn open(
        application: &Application,
        mut resolved: ResolvedDatasourceConnection,
        read_only: SessionReadOnly,
        expected_driver: Option<&NativeDriverDescriptor>,
    ) -> Result<Self, AppError> {
        resolved.driver_id =
            resolve_jdbc_driver_id(application, &resolved.driver_id, expected_driver)?;
        let engine = application.require_engine().await?;
        let driver = engine.driver_client().map_err(AppError::from)?;
        let session = driver
            .open_session(session_config(resolved, read_only))
            .await
            .map_err(AppError::from)?;
        Ok(Self {
            session: Some(session),
            engine: Some(engine),
        })
    }

    async fn query(
        self,
        sql: String,
        parameters: Vec<JdbcParameter>,
        limits: JdbcQueryLimits,
    ) -> Result<JdbcQueryResult, AppError> {
        let task = tokio::spawn(async move { self.run_query(sql, parameters, limits).await });
        match task.await {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(
                    cancelled = error.is_cancelled(),
                    panicked = error.is_panic(),
                    "managed JDBC query task ended without a product result"
                );
                Err(AppError::internal())
            }
        }
    }

    async fn run_query(
        self,
        sql: String,
        parameters: Vec<JdbcParameter>,
        limits: JdbcQueryLimits,
    ) -> Result<JdbcQueryResult, AppError> {
        let session = self.session.as_ref().expect("open JDBC session is present");
        let result = match session
            .execute_query(QueryRequest {
                sql,
                parameters,
                transaction_id: None,
                options: QueryOptions {
                    max_rows: limits.max_rows,
                    target_batch_rows: JDBC_QUERY_BATCH_ROWS,
                    target_batch_bytes: JDBC_QUERY_BATCH_BYTES,
                    initial_batch_credits: 0,
                    max_result_bytes: limits.max_result_bytes,
                },
            })
            .await
        {
            Ok(mut stream) => {
                let result = consume_bounded_query(&mut stream).await;
                let result = match result {
                    Ok(result) => Ok(result),
                    Err(primary) => {
                        if let Err(cleanup_error) =
                            crate::query::settle_query_stream(&mut stream).await
                        {
                            tracing::warn!(
                                cleanup_error = %cleanup_error,
                                "JDBC query stream cleanup also failed after the primary operation failure"
                            );
                        }
                        Err(primary)
                    }
                };
                drop(stream);
                result
            }
            Err(error) => Err(error.into()),
        };
        self.finish(result).await
    }

    async fn finish<T>(mut self, result: Result<T, AppError>) -> Result<T, AppError> {
        let session = self.session.take();
        let engine = self.engine.take();
        let cleanup = tokio::spawn(async move {
            let close_result = match session {
                Some(session) => session.close().await.map_err(AppError::from),
                None => Ok(()),
            };
            drop(engine);
            close_result
        });
        let close_result = match cleanup.await {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(
                    cancelled = error.is_cancelled(),
                    panicked = error.is_panic(),
                    "managed JDBC session cleanup task ended without a product result"
                );
                Err(AppError::internal())
            }
        };
        match (result, close_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(close_error)) => Err(close_error),
            (Err(primary), Ok(())) => Err(primary),
            (Err(primary), Err(close_error)) => {
                tracing::warn!(
                    close_error = %close_error,
                    "JDBC session cleanup also failed after the primary operation failure"
                );
                Err(primary)
            }
        }
    }
}

impl Drop for ManagedJdbcSession {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        let engine = self.engine.take();
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!(
                "JDBC session owner was dropped outside a Tokio runtime; asynchronous cleanup could not run"
            );
            return;
        };
        runtime.spawn(async move {
            if let Err(error) = session.close().await {
                tracing::warn!(
                    close_error = %error,
                    "best-effort JDBC session cleanup failed"
                );
            }
            drop(engine);
        });
    }
}

async fn consume_bounded_query(
    stream: &mut chat2db_java_bridge::QueryStream,
) -> Result<JdbcQueryResult, AppError> {
    let mut columns = None;
    let mut rows = Vec::new();
    loop {
        let event = stream.next_event().await.map_err(AppError::from)?;
        match event {
            Some(QueryEvent::Started(started)) if columns.is_none() => {
                columns = Some(started.columns);
                crate::query::grant_next_credit(stream).await?;
            }
            Some(QueryEvent::Started(_)) => {
                return Err(invalid_jdbc_query_stream(
                    "JDBC query emitted more than one schema event",
                ));
            }
            Some(QueryEvent::Batch(batch)) => {
                if columns.is_none() {
                    return Err(invalid_jdbc_query_stream(
                        "JDBC query emitted rows before its schema",
                    ));
                }
                rows.extend(batch.rows);
                crate::query::grant_next_credit(stream).await?;
            }
            Some(QueryEvent::Completed(completed)) => {
                let columns = columns.ok_or_else(|| {
                    invalid_jdbc_query_stream("JDBC query completed without a schema")
                })?;
                return Ok(JdbcQueryResult {
                    columns,
                    rows,
                    completed,
                });
            }
            None => {
                return Err(invalid_jdbc_query_stream(
                    "JDBC query ended without a completion event",
                ));
            }
        }
    }
}

fn resolve_jdbc_driver_id(
    application: &Application,
    requested_driver_id: &str,
    expected_driver: Option<&NativeDriverDescriptor>,
) -> Result<String, AppError> {
    let requested_driver_id = requested_driver_id.trim();
    let exact_managed = application
        .inner
        .drivers
        .iter()
        .find(|driver| driver.driver_id.eq_ignore_ascii_case(requested_driver_id));
    let selected_native = application.native_driver_for_datasource_driver_id(requested_driver_id);

    if let Some(expected) = expected_driver {
        let matches_expected = selected_native
            .as_ref()
            .is_some_and(|driver| driver.descriptor().id.eq_ignore_ascii_case(expected.id))
            || exact_managed.is_some_and(|driver| jdbc_driver_matches_descriptor(driver, expected));
        if !matches_expected {
            return Err(AppError::invalid(
                "jdbc_driver_mismatch",
                "The datasource is not compatible with the selected database driver",
            ));
        }
    }

    if let Some(driver) = exact_managed {
        return Ok(driver.driver_id.clone());
    }

    let descriptor =
        expected_driver.or_else(|| selected_native.as_ref().map(|driver| driver.descriptor()));
    if let Some(descriptor) = descriptor {
        if let Some(driver) =
            managed_jdbc_driver_for_descriptor(&application.inner.drivers, descriptor)
        {
            return Ok(driver.driver_id.clone());
        }
        return match &application.inner.managed_driver_ids {
            Some(_) => Err(driver_not_installed()),
            None => Ok(requested_driver_id.to_owned()),
        };
    }

    match &application.inner.managed_driver_ids {
        Some(_) => Err(driver_not_installed()),
        None => Ok(requested_driver_id.to_owned()),
    }
}

fn invalid_jdbc_query_stream(message: &'static str) -> AppError {
    AppError::new(
        AppErrorKind::Internal,
        ApiError::new("invalid_jdbc_query_stream", message),
    )
}

fn session_config(
    resolved: ResolvedDatasourceConnection,
    read_only: SessionReadOnly,
) -> SessionConfig {
    let ResolvedDatasourceConnection {
        driver_id,
        datasource_name: _,
        connection,
        ..
    } = resolved;
    let read_only = match read_only {
        SessionReadOnly::Configured => connection.read_only,
        SessionReadOnly::Forced => true,
    };
    SessionConfig {
        driver_id,
        jdbc_url: connection.jdbc_url,
        properties: connection
            .properties
            .into_iter()
            .map(|property| ConnectionProperty {
                key: property.key,
                value: property.value,
                sensitive: property.sensitive,
            })
            .collect(),
        read_only,
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{DatasourceConnection, DatasourceConnectionProperty};

    use super::{
        ResolvedDatasourceConnection, SessionReadOnly, resolve_jdbc_driver_id, session_config,
    };
    use crate::Application;

    #[test]
    fn configured_session_preserves_the_datasource_read_only_setting() {
        let config = session_config(resolved(false), SessionReadOnly::Configured);

        assert!(!config.read_only);
        assert_eq!(config.driver_id, "driver-1");
        assert_eq!(config.jdbc_url, "jdbc:h2:mem:test");
        assert_eq!(config.properties.len(), 1);
        assert_eq!(config.properties[0].key, "user");
        assert_eq!(config.properties[0].value, "sa");
        assert!(!config.properties[0].sensitive);

        assert!(session_config(resolved(true), SessionReadOnly::Configured).read_only);
    }

    #[test]
    fn forced_session_is_read_only_even_when_the_datasource_is_writable() {
        assert!(session_config(resolved(false), SessionReadOnly::Forced).read_only);
    }

    #[test]
    fn unmanaged_engine_host_preserves_an_external_native_driver_id() {
        let application = Application::new();

        assert_eq!(
            resolve_jdbc_driver_id(&application, "mysql", None)
                .expect("an unmanaged host cannot inspect the external driver's inventory"),
            "mysql"
        );
    }

    fn resolved(read_only: bool) -> ResolvedDatasourceConnection {
        ResolvedDatasourceConnection {
            datasource_id: "datasource-1".to_owned(),
            datasource_revision: 1,
            driver_id: "driver-1".to_owned(),
            datasource_name: "Local H2".to_owned(),
            connection: DatasourceConnection {
                jdbc_url: "jdbc:h2:mem:test".to_owned(),
                properties: vec![DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "sa".to_owned(),
                    sensitive: false,
                }],
                read_only,
                ssh: None,
            },
        }
    }
}
