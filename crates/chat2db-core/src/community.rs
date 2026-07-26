use std::future::Future;

use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, CommunityBuiltSql, CommunityDriverConfig,
    CommunityParsedStatement, CommunityPlugin, CommunityPluginBehavior, CommunityPluginCatalog,
    CommunityPluginServices, CommunitySchema, CommunitySchemaList, CommunitySqlAnalysis,
    ListCommunitySchemasRequest, ParseCommunitySqlRequest,
};
use chat2db_java_bridge::{
    BridgeError, CommunityClasspath, CommunityDriverConfig as BridgeCommunityDriverConfig,
    CommunityParsedStatement as BridgeCommunityParsedStatement,
    CommunityPlugin as BridgeCommunityPlugin,
    CommunityPluginCatalog as BridgeCommunityPluginCatalog,
    CommunitySchema as BridgeCommunitySchema, CommunitySqlAnalysis as BridgeCommunitySqlAnalysis,
};

const FIXED_COMMUNITY_CLASSPATH_LOCK: &str =
    include_str!("../../../third_party/community-h2-classpath.lock");

/// Loads the product's fixed Community 5.3.0 classpath only when every
/// filename, byte length, and SHA-256 digest matches the embedded lock.
///
/// # Errors
///
/// Returns an error when the directory or embedded lock is invalid or when
/// the artifact set differs from the fixed distribution inventory.
pub fn load_fixed_community_classpath(
    directory: impl AsRef<std::path::Path>,
) -> Result<CommunityClasspath, BridgeError> {
    CommunityClasspath::from_locked_directory(directory, FIXED_COMMUNITY_CLASSPATH_LOCK)
}

use crate::{
    AppError, Application,
    datasource_session::{SessionReadOnly, open_datasource_session, resolve_datasource_connection},
};

impl Application {
    /// Lists the plugins discovered from the fixed Community classpath.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, protocol, or Community
    /// discovery error.
    pub async fn list_community_plugins(&self) -> Result<CommunityPluginCatalog, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .list_plugins()
            .await
            .map(community_plugin_catalog)
            .map_err(AppError::from)
    }

    /// Lists schemas through Community metadata using a forced read-only session.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, engine, metadata, or session-cleanup errors.
    pub async fn list_community_schemas(
        &self,
        request: ListCommunitySchemasRequest,
    ) -> Result<CommunitySchemaList, AppError> {
        let storage = self.require_storage()?;
        let engine = self.require_community_engine()?;
        let ListCommunitySchemasRequest {
            datasource_id,
            database_type,
            database_name,
        } = request;
        let client = engine.community_client().map_err(AppError::from)?;
        run_cancellation_safe(async move {
            let resolved = resolve_datasource_connection(&storage, &datasource_id).await?;
            let session =
                open_datasource_session(&engine, resolved, SessionReadOnly::Forced).await?;
            let outcome = client
                .list_schemas(&session, database_type, database_name, None)
                .await
                .map(|schemas| CommunitySchemaList {
                    items: schemas.into_iter().map(community_schema).collect(),
                })
                .map_err(AppError::from);
            let cleanup = session.close().await.map_err(AppError::from);
            preserve_primary_result("close_community_schema_session", outcome, cleanup)
        })
        .await
    }

    /// Builds dialect-specific `CREATE SCHEMA` SQL through Community.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, validation, protocol, or
    /// Community SQL-builder error.
    pub async fn build_community_create_schema(
        &self,
        request: BuildCommunityCreateSchemaRequest,
    ) -> Result<CommunityBuiltSql, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .build_create_schema(request.database_type, bridge_schema(request.schema))
            .await
            .map(|sql| CommunityBuiltSql { sql })
            .map_err(AppError::from)
    }

    /// Parses SQL through the retained Community dialect parser.
    ///
    /// # Errors
    ///
    /// Returns an engine availability, capability, validation, protocol, or
    /// Community parser error.
    pub async fn parse_community_sql(
        &self,
        request: ParseCommunitySqlRequest,
    ) -> Result<CommunitySqlAnalysis, AppError> {
        let engine = self.require_community_engine()?;
        let client = engine.community_client().map_err(AppError::from)?;
        client
            .parse_sql(request.database_type, request.sql)
            .await
            .map(community_sql_analysis)
            .map_err(AppError::from)
    }

    fn require_community_engine(&self) -> Result<chat2db_java_bridge::EngineClient, AppError> {
        let engine = self.require_engine()?;
        if !engine.community_compatibility_configured() {
            return Err(AppError::unavailable(
                "community_compatibility_disabled",
                "The fixed Community compatibility classpath is not configured",
            ));
        }
        Ok(engine)
    }
}

async fn run_cancellation_safe<T, F>(operation: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: Future<Output = Result<T, AppError>> + Send + 'static,
{
    match tokio::spawn(operation).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(
                cancelled = error.is_cancelled(),
                panicked = error.is_panic(),
                "Community metadata task ended without a product result"
            );
            Err(AppError::internal())
        }
    }
}

fn community_plugin_catalog(catalog: BridgeCommunityPluginCatalog) -> CommunityPluginCatalog {
    CommunityPluginCatalog {
        source_commit: catalog.source_commit,
        plugins: catalog.plugins.into_iter().map(community_plugin).collect(),
    }
}

fn community_plugin(plugin: BridgeCommunityPlugin) -> CommunityPlugin {
    CommunityPlugin {
        database_type: plugin.database_type,
        name: plugin.name,
        behavior: CommunityPluginBehavior {
            supports_database: plugin.behavior.supports_database,
            supports_schema: plugin.behavior.supports_schema,
            preserves_script_batch_execution: plugin.behavior.preserves_script_batch_execution,
        },
        drivers: plugin
            .drivers
            .into_iter()
            .map(community_driver_config)
            .collect(),
        services: CommunityPluginServices {
            metadata_available: plugin.services.metadata_available,
            sql_builder_available: plugin.services.sql_builder_available,
            sql_parser_available: plugin.services.sql_parser_available,
        },
    }
}

fn community_driver_config(driver: BridgeCommunityDriverConfig) -> CommunityDriverConfig {
    CommunityDriverConfig {
        url: driver.url,
        jdbc_driver: driver.jdbc_driver,
        jdbc_driver_class: driver.jdbc_driver_class,
        download_urls: driver.download_urls,
        custom: driver.custom,
        default_driver: driver.default_driver,
    }
}

fn community_schema(schema: BridgeCommunitySchema) -> CommunitySchema {
    CommunitySchema {
        database_name: schema.database_name,
        name: schema.name,
        comment: schema.comment,
        owner: schema.owner,
        system: schema.system,
    }
}

fn bridge_schema(schema: CommunitySchema) -> BridgeCommunitySchema {
    BridgeCommunitySchema {
        database_name: schema.database_name,
        name: schema.name,
        comment: schema.comment,
        owner: schema.owner,
        system: schema.system,
    }
}

fn community_sql_analysis(analysis: BridgeCommunitySqlAnalysis) -> CommunitySqlAnalysis {
    CommunitySqlAnalysis {
        is_select: analysis.is_select,
        statements: analysis
            .statements
            .into_iter()
            .map(community_parsed_statement)
            .collect(),
    }
}

fn community_parsed_statement(
    statement: BridgeCommunityParsedStatement,
) -> CommunityParsedStatement {
    CommunityParsedStatement {
        sql: statement.sql,
        statement_type: statement.statement_type,
        kind: statement.kind,
    }
}

fn preserve_primary_result<T>(
    cleanup_phase: &'static str,
    outcome: Result<T, AppError>,
    cleanup: Result<(), AppError>,
) -> Result<T, AppError> {
    match cleanup {
        Ok(()) => outcome,
        Err(cleanup_error) => match outcome {
            Ok(_) => Err(cleanup_error),
            Err(primary_error) => {
                tracing::warn!(
                    cleanup_phase,
                    cleanup_error = %cleanup_error,
                    "Community session cleanup failed after the primary outcome was determined"
                );
                Err(primary_error)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{
        CommunityDriverConfig, CommunityParsedStatement, CommunityPlugin, CommunityPluginBehavior,
        CommunityPluginCatalog, CommunityPluginServices, CommunitySchema, CommunitySqlAnalysis,
        ListCommunitySchemasRequest,
    };
    use chat2db_java_bridge::{
        CommunityDriverConfig as BridgeCommunityDriverConfig,
        CommunityParsedStatement as BridgeCommunityParsedStatement,
        CommunityPlugin as BridgeCommunityPlugin,
        CommunityPluginBehavior as BridgeCommunityPluginBehavior,
        CommunityPluginCatalog as BridgeCommunityPluginCatalog,
        CommunityPluginServices as BridgeCommunityPluginServices,
        CommunitySchema as BridgeCommunitySchema,
        CommunitySqlAnalysis as BridgeCommunitySqlAnalysis,
    };
    use tokio::{sync::oneshot, time};

    use super::{
        Application, bridge_schema, community_plugin_catalog, community_schema,
        community_sql_analysis, preserve_primary_result, run_cancellation_safe,
    };
    use crate::{AppError, AppErrorKind};

    #[test]
    fn bridge_plugin_catalog_mapping_preserves_every_field() {
        let bridge = BridgeCommunityPluginCatalog {
            source_commit: "f63cbf4".to_owned(),
            plugins: vec![BridgeCommunityPlugin {
                database_type: "H2".to_owned(),
                name: "H2 Database".to_owned(),
                behavior: BridgeCommunityPluginBehavior {
                    supports_database: true,
                    supports_schema: true,
                    preserves_script_batch_execution: false,
                },
                drivers: vec![BridgeCommunityDriverConfig {
                    url: "jdbc:h2:mem:test".to_owned(),
                    jdbc_driver: "h2.jar".to_owned(),
                    jdbc_driver_class: "org.h2.Driver".to_owned(),
                    download_urls: vec!["https://example.invalid/h2.jar".to_owned()],
                    custom: false,
                    default_driver: true,
                }],
                services: BridgeCommunityPluginServices {
                    metadata_available: true,
                    sql_builder_available: true,
                    sql_parser_available: true,
                },
            }],
        };
        let expected = CommunityPluginCatalog {
            source_commit: "f63cbf4".to_owned(),
            plugins: vec![CommunityPlugin {
                database_type: "H2".to_owned(),
                name: "H2 Database".to_owned(),
                behavior: CommunityPluginBehavior {
                    supports_database: true,
                    supports_schema: true,
                    preserves_script_batch_execution: false,
                },
                drivers: vec![CommunityDriverConfig {
                    url: "jdbc:h2:mem:test".to_owned(),
                    jdbc_driver: "h2.jar".to_owned(),
                    jdbc_driver_class: "org.h2.Driver".to_owned(),
                    download_urls: vec!["https://example.invalid/h2.jar".to_owned()],
                    custom: false,
                    default_driver: true,
                }],
                services: CommunityPluginServices {
                    metadata_available: true,
                    sql_builder_available: true,
                    sql_parser_available: true,
                },
            }],
        };

        assert_eq!(community_plugin_catalog(bridge), expected);
    }

    #[test]
    fn schema_mapping_preserves_every_field_in_both_directions() {
        let contract = CommunitySchema {
            database_name: "inventory".to_owned(),
            name: "reporting".to_owned(),
            comment: "Reporting objects".to_owned(),
            owner: "app".to_owned(),
            system: false,
        };
        let bridge = BridgeCommunitySchema {
            database_name: "inventory".to_owned(),
            name: "reporting".to_owned(),
            comment: "Reporting objects".to_owned(),
            owner: "app".to_owned(),
            system: false,
        };

        assert_eq!(bridge_schema(contract.clone()), bridge);
        assert_eq!(community_schema(bridge), contract);
    }

    #[test]
    fn sql_analysis_mapping_preserves_every_field() {
        let analysis = BridgeCommunitySqlAnalysis {
            is_select: true,
            statements: vec![BridgeCommunityParsedStatement {
                sql: "select 1".to_owned(),
                statement_type: "SELECT".to_owned(),
                kind: "Select".to_owned(),
            }],
        };

        assert_eq!(
            community_sql_analysis(analysis),
            CommunitySqlAnalysis {
                is_select: true,
                statements: vec![CommunityParsedStatement {
                    sql: "select 1".to_owned(),
                    statement_type: "SELECT".to_owned(),
                    kind: "Select".to_owned(),
                }],
            }
        );
    }

    #[test]
    fn cleanup_success_preserves_the_primary_outcome() {
        assert_eq!(
            preserve_primary_result("test_cleanup", Ok(7), Ok(()))
                .expect("successful work remains successful"),
            7
        );
        let primary = AppError::invalid("primary_failure", "primary");
        assert_eq!(
            preserve_primary_result::<()>("test_cleanup", Err(primary.clone()), Ok(()))
                .expect_err("primary failure remains visible"),
            primary
        );
    }

    #[test]
    fn cleanup_failure_does_not_replace_a_primary_failure() {
        let primary = AppError::invalid("primary_failure", "primary");
        let cleanup = AppError::invalid("cleanup_failure", "cleanup");

        assert_eq!(
            preserve_primary_result::<()>("test_cleanup", Err(primary.clone()), Err(cleanup))
                .expect_err("primary failure remains visible"),
            primary
        );
    }

    #[test]
    fn cleanup_failure_fails_otherwise_successful_work() {
        let cleanup = AppError::invalid("cleanup_failure", "cleanup");

        assert_eq!(
            preserve_primary_result("test_cleanup", Ok(7), Err(cleanup.clone()))
                .expect_err("cleanup failure must be visible"),
            cleanup
        );
    }

    #[tokio::test]
    async fn cancellation_safe_work_finishes_after_its_waiter_is_aborted() {
        let (started_sender, started_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = oneshot::channel();
        let (finished_sender, finished_receiver) = oneshot::channel();
        let waiter = tokio::spawn(async move {
            run_cancellation_safe(async move {
                started_sender.send(()).expect("waiter must observe start");
                release_receiver.await.expect("test must release work");
                finished_sender
                    .send(())
                    .expect("test must observe detached completion");
                Ok::<(), AppError>(())
            })
            .await
        });

        started_receiver.await.expect("work must start");
        waiter.abort();
        assert!(
            waiter
                .await
                .expect_err("waiter must be aborted")
                .is_cancelled()
        );
        release_sender.send(()).expect("work must still be alive");
        time::timeout(std::time::Duration::from_secs(1), finished_receiver)
            .await
            .expect("detached work must finish promptly")
            .expect("detached work must report completion");
    }

    #[tokio::test]
    async fn community_services_report_unconfigured_dependencies_safely() {
        let application = Application::new();

        let engine_error = application
            .list_community_plugins()
            .await
            .expect_err("plugin discovery requires the engine");
        assert_eq!(engine_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(engine_error.api_error().code, "database_engine_unavailable");

        let storage_error = application
            .list_community_schemas(ListCommunitySchemasRequest {
                datasource_id: "datasource-1".to_owned(),
                database_type: "H2".to_owned(),
                database_name: "inventory".to_owned(),
            })
            .await
            .expect_err("schema metadata requires storage");
        assert_eq!(storage_error.kind(), AppErrorKind::Unavailable);
        assert_eq!(storage_error.api_error().code, "storage_unavailable");
    }
}
