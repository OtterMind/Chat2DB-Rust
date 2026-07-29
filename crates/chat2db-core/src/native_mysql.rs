use chat2db_contract::{
    ApiError, CommunityDatabase, CommunityDatabaseList, CommunitySchemaList, CommunityTable,
    CommunityTableList, DatasourceConnection,
};
use mysql_async::{Conn, Error as MysqlError, Opts, OptsBuilder, SslOpts, prelude::Queryable};
use std::time::Duration;
use url::Url;

use crate::{
    AppError, AppErrorKind, Application,
    datasource_session::{ResolvedDatasourceConnection, resolve_datasource_connection},
};

const MYSQL_SCHEME: &str = "mysql://";
const JDBC_MYSQL_SCHEME: &str = "jdbc:mysql://";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
type TableRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(crate) fn is_mysql_database_type(database_type: &str) -> bool {
    database_type.trim().eq_ignore_ascii_case("mysql")
}

pub(crate) async fn test_connection(connection: &DatasourceConnection) -> Result<(), AppError> {
    let mut conn = open_connection(connection).await?;
    let result = conn.ping().await.map_err(mysql_connection_error);
    let close = conn.disconnect().await.map_err(mysql_connection_error);
    result.and(close)
}

pub(crate) async fn list_databases(
    application: &Application,
    datasource_id: &str,
) -> Result<CommunityDatabaseList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_connection(&resolved.connection).await?;
    let result = conn
        .query::<(String, String, String), _>(
            "SELECT SCHEMA_NAME, DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
             FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
        )
        .await
        .map(|rows| CommunityDatabaseList {
            items: rows
                .into_iter()
                .map(|(name, charset, collation)| CommunityDatabase {
                    system: is_system_database(&name),
                    name,
                    charset,
                    collation,
                    ..CommunityDatabase::default()
                })
                .collect(),
        })
        .map_err(mysql_query_error);
    finish_connection(conn, result).await
}

pub(crate) async fn list_schemas(
    application: &Application,
    datasource_id: &str,
) -> Result<CommunitySchemaList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let conn = open_connection(&resolved.connection).await?;
    finish_connection(conn, Ok(CommunitySchemaList::default())).await
}

pub(crate) async fn list_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name_pattern: &str,
) -> Result<CommunityTableList, AppError> {
    if database_name.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_mysql_metadata_request",
            "databaseName cannot be empty",
        ));
    }
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_connection(&resolved.connection).await?;
    let query = "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, COALESCE(TABLE_COMMENT, ''), \
                 COALESCE(ENGINE, ''), COALESCE(TABLE_COLLATION, ''), \
                 CAST(AUTO_INCREMENT AS CHAR), CAST(TABLE_ROWS AS CHAR), \
                 CAST(DATA_LENGTH AS CHAR), \
                 DATE_FORMAT(CREATE_TIME, '%Y-%m-%dT%H:%i:%s'), \
                 DATE_FORMAT(UPDATE_TIME, '%Y-%m-%dT%H:%i:%s') \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? AND (? = '' OR TABLE_NAME LIKE ?) \
                 ORDER BY TABLE_NAME";
    let pattern = table_name_pattern.trim().to_owned();
    let result = conn
        .exec::<TableRow, _, _>(query, (database_name.to_owned(), pattern.clone(), pattern))
        .await
        .map(|rows| CommunityTableList {
            items: rows
                .into_iter()
                .map(
                    |(
                        database_name,
                        name,
                        table_type,
                        comment,
                        engine,
                        collation,
                        increment_value,
                        rows,
                        data_length,
                        create_time,
                        update_time,
                    )| CommunityTable {
                        database_name,
                        name,
                        table_type: normalize_table_type(&table_type).to_owned(),
                        comment,
                        database_type: "MYSQL".to_owned(),
                        engine,
                        charset: collation
                            .split_once('_')
                            .map_or_else(String::new, |(charset, _)| charset.to_owned()),
                        collation,
                        increment_value,
                        rows,
                        data_length,
                        create_time: create_time.unwrap_or_default(),
                        update_time: update_time.unwrap_or_default(),
                        ..CommunityTable::default()
                    },
                )
                .collect(),
        })
        .map_err(mysql_query_error);
    finish_connection(conn, result).await
}

async fn resolve_native_connection(
    application: &Application,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    if !application.is_native_mysql_driver(&resolved.driver_id) {
        return Err(AppError::invalid(
            "mysql_driver_mismatch",
            "The datasource is not configured with a MySQL driver",
        ));
    }
    Ok(resolved)
}

async fn open_connection(connection: &DatasourceConnection) -> Result<Conn, AppError> {
    let opts = connection_opts(connection)?;
    tokio::time::timeout(CONNECT_TIMEOUT, Conn::new(opts))
        .await
        .map_err(|_| {
            AppError::unavailable(
                "mysql_connection_timeout",
                "The MySQL connection attempt timed out",
            )
        })?
        .map_err(mysql_connection_error)
}

async fn finish_connection<T>(conn: Conn, result: Result<T, AppError>) -> Result<T, AppError> {
    let close = conn.disconnect().await.map_err(mysql_connection_error);
    match result {
        Ok(value) => close.map(|()| value),
        Err(error) => {
            if let Err(close_error) = close {
                tracing::warn!(error = %close_error, "native MySQL connection cleanup failed");
            }
            Err(error)
        }
    }
}

fn connection_opts(connection: &DatasourceConnection) -> Result<Opts, AppError> {
    let url = normalize_mysql_url(&connection.jdbc_url)?;
    let mut parsed = Url::parse(&url).map_err(|_| invalid_connection_url())?;
    if parsed.scheme() != "mysql" || parsed.host_str().is_none() {
        return Err(invalid_connection_url());
    }
    let query_properties = parsed
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    parsed.set_query(None);
    parsed.set_fragment(None);
    let base = Opts::from_url(parsed.as_str()).map_err(|_| invalid_connection_url())?;
    let mut builder = OptsBuilder::from_opts(base).prefer_socket(Some(false));

    let mut ssl = None;
    for (key, value) in query_properties
        .iter()
        .map(|(key, value)| (key, value))
        .chain(
            connection
                .properties
                .iter()
                .map(|property| (&property.key, &property.value)),
        )
    {
        match key.trim().to_ascii_lowercase().as_str() {
            "user" | "username" => builder = builder.user(Some(value.to_owned())),
            "password" => builder = builder.pass(Some(value.to_owned())),
            "database" | "databasename" => builder = builder.db_name(Some(value.to_owned())),
            "usessl" | "requiressl" => {
                ssl = parse_bool(value).then(SslOpts::default);
            }
            "sslmode" => {
                ssl = match value.trim().to_ascii_lowercase().as_str() {
                    "disable" | "disabled" | "false" | "preferred" => None,
                    "require" | "required" | "true" => Some(
                        SslOpts::default()
                            .with_danger_accept_invalid_certs(true)
                            .with_danger_skip_domain_validation(true),
                    ),
                    "verify_ca" => {
                        Some(SslOpts::default().with_danger_skip_domain_validation(true))
                    }
                    "verify_identity" => Some(SslOpts::default()),
                    _ => return Err(invalid_connection_property("sslMode")),
                };
            }
            "verifyservercertificate" if !parse_bool(value) && ssl.is_some() => {
                ssl = ssl.map(|options| {
                    options
                        .with_danger_accept_invalid_certs(true)
                        .with_danger_skip_domain_validation(true)
                });
            }
            _ => {}
        }
    }
    Ok(builder.ssl_opts(ssl).into())
}

fn normalize_mysql_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value
        .get(..JDBC_MYSQL_SCHEME.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(JDBC_MYSQL_SCHEME))
    {
        return Ok(format!(
            "{MYSQL_SCHEME}{}",
            &value[JDBC_MYSQL_SCHEME.len()..]
        ));
    }
    if value
        .get(..MYSQL_SCHEME.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(MYSQL_SCHEME))
    {
        return Ok(format!("{MYSQL_SCHEME}{}", &value[MYSQL_SCHEME.len()..]));
    }
    Err(invalid_connection_url())
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "required"
    )
}

fn normalize_table_type(value: &str) -> &str {
    if value.eq_ignore_ascii_case("VIEW") {
        "VIEW"
    } else {
        "TABLE"
    }
}

fn is_system_database(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "information_schema" | "mysql" | "performance_schema" | "sys"
    )
}

fn invalid_connection_url() -> AppError {
    AppError::invalid(
        "invalid_mysql_connection",
        "A valid jdbc:mysql:// or mysql:// connection URL is required",
    )
}

fn invalid_connection_property(property: &str) -> AppError {
    AppError::invalid(
        "invalid_mysql_connection",
        format!("The MySQL connection property {property} is invalid"),
    )
}

fn mysql_connection_error(error: MysqlError) -> AppError {
    match error {
        MysqlError::Server(server) => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("mysql_connection_rejected", server.message),
        ),
        _ => AppError::unavailable(
            "mysql_connection_failed",
            "The MySQL server could not be reached",
        ),
    }
}

fn mysql_query_error(error: MysqlError) -> AppError {
    match error {
        MysqlError::Server(server) => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("mysql_query_failed", server.message),
        ),
        _ => AppError::unavailable(
            "mysql_connection_failed",
            "The MySQL connection ended before the operation completed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{DatasourceConnection, DatasourceConnectionProperty};

    use super::{connection_opts, is_mysql_database_type, normalize_table_type};

    #[test]
    fn jdbc_url_and_properties_build_native_options_without_exposing_jdbc() {
        let opts = connection_opts(&DatasourceConnection {
            jdbc_url: "jdbc:mysql://db.example:3307/app?useSSL=false&serverTimezone=UTC".to_owned(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "chat2db".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "secret".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: false,
        })
        .expect("JDBC URL should convert");

        assert_eq!(opts.ip_or_hostname(), "db.example");
        assert_eq!(opts.tcp_port(), 3307);
        assert_eq!(opts.db_name(), Some("app"));
        assert_eq!(opts.user(), Some("chat2db"));
        assert_eq!(opts.pass(), Some("secret"));
        assert!(opts.ssl_opts().is_none());
        assert!(!opts.prefer_socket());
    }

    #[test]
    fn mysql_detection_and_table_types_are_closed() {
        assert!(is_mysql_database_type(" mysql "));
        assert!(!is_mysql_database_type("mariadb"));
        assert_eq!(normalize_table_type("VIEW"), "VIEW");
        assert_eq!(normalize_table_type("BASE TABLE"), "TABLE");
    }

    #[test]
    fn explicit_properties_override_url_values_and_ssl_modes_are_mapped() {
        let opts = connection_opts(&DatasourceConnection {
            jdbc_url: "mysql://url-user:url-pass@localhost/url_db?sslMode=VERIFY_IDENTITY"
                .to_owned(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "property-user".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "property-pass".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: false,
        })
        .expect("native URL should convert");

        assert_eq!(opts.user(), Some("property-user"));
        assert_eq!(opts.pass(), Some("property-pass"));
        assert_eq!(opts.db_name(), Some("url_db"));
        assert!(opts.ssl_opts().is_some());
    }

    #[test]
    fn invalid_and_non_mysql_urls_are_rejected_without_panicking() {
        for jdbc_url in [
            "",
            "jdbc:postgresql://localhost/app",
            "数mysql://localhost/app",
        ] {
            let error = connection_opts(&DatasourceConnection {
                jdbc_url: jdbc_url.to_owned(),
                properties: Vec::new(),
                read_only: false,
            })
            .expect_err("non-MySQL URLs must fail");
            assert_eq!(error.api_error().code, "invalid_mysql_connection");
        }
    }
}
