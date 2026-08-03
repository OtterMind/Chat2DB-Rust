use chat2db_contract::{ApiError, DatasourceConnection};
use chat2db_java_bridge::{ConnectionProperty, EngineClient, Session, SessionConfig};
use chat2db_storage::Storage;

use crate::{AppError, AppErrorKind};

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

    use super::{ResolvedDatasourceConnection, SessionReadOnly, session_config};

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
