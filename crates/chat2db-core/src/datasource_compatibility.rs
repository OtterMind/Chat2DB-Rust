use std::{collections::HashSet, sync::Arc};

use chat2db_contract::{
    CloneDatasourceRequest, CommunityDatasourceExport, CommunityDatasourceImportResult,
    ConsoleConnectResult, CreateDatasourceRequest, DatasourceCloseResult, DatasourceConnectResult,
    DatasourceConnection, DatasourceConnectionProperty, DatasourceSessionMode,
    ExportCommunityDatasourcesRequest, JdbcDriver, ListCommunityDatabasesRequest,
    NativeDriverAction, NativeDriverCompatibility, PortableCommunityDatasource,
    PortableDatasourceConnection, PortableDatasourceProperty, SshAuthentication,
    SshAuthenticationType, SshTunnelConfig,
};
use chat2db_storage::{CreateDatasource, SecretValue, StorageError};
use url::Url;

use crate::{
    AppError, Application, convert,
    datasource_edit::project_ssh,
    datasource_session::resolve_datasource_connection,
    native_driver::{NativeDriver, NativeDriverRegistry},
    native_driver_types::NativeDriverDescriptor,
    now_millis, storage_call,
};

const COMMUNITY_DATASOURCE_DOCUMENT_VERSION: u32 = 1;
const MAX_TRANSFER_DATASOURCES: usize = 1_000;

impl Application {
    /// Clones datasource metadata and installs a separately referenced copy of its vault secret.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, vault, availability, or storage failures.
    pub async fn clone_datasource(
        &self,
        request: CloneDatasourceRequest,
    ) -> Result<chat2db_contract::Datasource, AppError> {
        if request.id.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_datasource_clone",
                "datasource id cannot be empty",
            ));
        }
        let storage = self.require_storage()?;
        let source_id = request.id;
        let requested_name = request.name;
        let record = storage_call(move || {
            let (source, secret) = storage.get_datasource_with_secret(&source_id)?;
            let name = requested_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| copy_name(&source.name));
            let copied_secret = secret
                .as_ref()
                .map(|value| SecretValue::new(value.expose_secret().to_vec()));
            storage.create_datasource(
                CreateDatasource {
                    name,
                    driver_id: source.driver_id,
                },
                copied_secret,
            )
        })
        .await?;
        Ok(convert::datasource(record))
    }

    /// Exports selected datasource definitions without passwords or sensitive properties.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, vault, availability, or storage failures.
    pub async fn export_community_datasources(
        &self,
        request: ExportCommunityDatasourcesRequest,
    ) -> Result<CommunityDatasourceExport, AppError> {
        if request.datasource_ids.len() > MAX_TRANSFER_DATASOURCES {
            return Err(AppError::invalid(
                "datasource_export_limit_exceeded",
                "at most 1000 datasources can be exported at once",
            ));
        }
        let storage = self.require_storage()?;
        let ids = if request.datasource_ids.is_empty() {
            let storage = storage.clone();
            storage_call(move || {
                storage.list_datasources().map(|records| {
                    records
                        .into_iter()
                        .map(|record| record.id)
                        .collect::<Vec<_>>()
                })
            })
            .await?
        } else {
            unique_non_empty_ids(request.datasource_ids)?
        };

        let mut datasources = Vec::with_capacity(ids.len());
        for id in ids {
            let storage = storage.clone();
            let (record, connection) = storage_call(move || {
                let (record, secret) = storage.get_datasource_with_secret(&id)?;
                let connection = secret
                    .as_ref()
                    .map(|secret| {
                        serde_json::from_slice::<DatasourceConnection>(secret.expose_secret())
                            .map_err(|_| {
                                StorageError::InvalidDatasource(
                                    "stored datasource connection descriptor is invalid",
                                )
                            })
                    })
                    .transpose()?;
                Ok((record, connection))
            })
            .await?;
            let connection = connection
                .map(|connection| portable_connection(&connection))
                .transpose()?;
            datasources.push(PortableCommunityDatasource {
                source_id: Some(record.id),
                name: record.name,
                driver_id: record.driver_id,
                connection,
            });
        }
        Ok(CommunityDatasourceExport {
            schema_version: COMMUNITY_DATASOURCE_DOCUMENT_VERSION,
            exported_at_ms: now_millis()?.to_string(),
            datasources,
        })
    }

    /// Imports a secret-safe Community document as new datasource records only.
    ///
    /// Imported source ids are intentionally ignored. Existing datasource metadata and vault
    /// references are never updated by this operation.
    ///
    /// # Errors
    ///
    /// Returns validation, driver, vault, availability, or storage failures.
    pub async fn import_community_datasources(
        &self,
        document: CommunityDatasourceExport,
    ) -> Result<CommunityDatasourceImportResult, AppError> {
        if document.schema_version != COMMUNITY_DATASOURCE_DOCUMENT_VERSION {
            return Err(AppError::invalid(
                "unsupported_datasource_import_version",
                "the datasource import document version is not supported",
            ));
        }
        if document.datasources.len() > MAX_TRANSFER_DATASOURCES {
            return Err(AppError::invalid(
                "datasource_import_limit_exceeded",
                "at most 1000 datasources can be imported at once",
            ));
        }

        let mut prepared = Vec::with_capacity(document.datasources.len());
        for datasource in document.datasources {
            self.require_managed_driver(&datasource.driver_id)?;
            let connection = datasource.connection.map(imported_connection).transpose()?;
            prepared.push(CreateDatasourceRequest {
                name: datasource.name,
                driver_id: datasource.driver_id,
                connection,
            });
        }

        let mut created = Vec::with_capacity(prepared.len());
        for request in prepared {
            created.push(self.create_datasource(request).await?);
        }
        let count = u32::try_from(created.len()).map_err(|_| AppError::internal())?;
        Ok(CommunityDatasourceImportResult { count, created })
    }

    /// Opens a real ephemeral metadata connection and returns its database list.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, driver, engine, or database failures.
    pub async fn connect_datasource_compatibility(
        &self,
        datasource_id: &str,
        database_type: &str,
    ) -> Result<DatasourceConnectResult, AppError> {
        let database_type = if database_type.trim().is_empty() {
            let datasource = self.get_datasource(datasource_id).await?;
            self.native_database_type_for_datasource_driver_id(&datasource.driver_id)
                .unwrap_or(datasource.driver_id)
        } else {
            database_type.trim().to_owned()
        };
        let databases = self
            .list_community_databases(ListCommunityDatabasesRequest {
                datasource_id: datasource_id.to_owned(),
                database_type,
            })
            .await?;
        Ok(DatasourceConnectResult {
            datasource_id: datasource_id.to_owned(),
            session_mode: DatasourceSessionMode::Ephemeral,
            databases: databases.items,
        })
    }

    /// Verifies a Console datasource using a real open/ping/close cycle.
    ///
    /// # Errors
    ///
    /// Returns datasource, storage, driver, engine, or database failures.
    pub async fn connect_console_compatibility(
        &self,
        datasource_id: &str,
    ) -> Result<ConsoleConnectResult, AppError> {
        let storage = self.require_storage()?;
        let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
        self.test_datasource_connection(&resolved.driver_id, resolved.connection)
            .await?;
        Ok(ConsoleConnectResult {
            datasource_id: datasource_id.to_owned(),
            session_mode: DatasourceSessionMode::Ephemeral,
            verified: true,
        })
    }

    /// Acknowledges close after verifying the datasource exists.
    ///
    /// Connections are operation-scoped, so successful operations have already disconnected and
    /// there is no retained pool generation to drain.
    ///
    /// # Errors
    ///
    /// Returns datasource-not-found, availability, or storage failures.
    pub async fn close_datasource_compatibility(
        &self,
        datasource_id: &str,
    ) -> Result<DatasourceCloseResult, AppError> {
        self.get_datasource(datasource_id).await?;
        Ok(DatasourceCloseResult {
            datasource_id: datasource_id.to_owned(),
            session_mode: DatasourceSessionMode::Ephemeral,
            closed_connections: 0,
        })
    }

    /// Returns an explicit no-artifact result for a registered native Rust driver.
    ///
    /// # Errors
    ///
    /// Returns invalid-request for database types without a registered native driver.
    pub fn native_driver_compatibility(
        &self,
        database_type: &str,
        action: NativeDriverAction,
    ) -> Result<NativeDriverCompatibility, AppError> {
        let driver = self
            .native_driver_for_database_type(database_type)
            .ok_or_else(|| {
                AppError::invalid(
                    "native_driver_not_available",
                    "the requested database type is not implemented by a native Rust driver",
                )
            })?;
        let descriptor = driver.descriptor();
        Ok(NativeDriverCompatibility {
            database_type: descriptor.database_types[0].to_owned(),
            driver_id: descriptor.id.to_owned(),
            action,
            implementation: descriptor.implementation.to_owned(),
            artifact_required: false,
            changed: false,
        })
    }
}

/// Converts native identity metadata into the historical JDBC-shaped HTTP contract.
pub(crate) fn jdbc_driver_from_descriptor(descriptor: &NativeDriverDescriptor) -> JdbcDriver {
    let display_name = match descriptor.id.to_ascii_lowercase().as_str() {
        "mysql" => "MySQL".to_owned(),
        _ => descriptor
            .database_types
            .first()
            .copied()
            .unwrap_or(descriptor.id)
            .to_owned(),
    };
    JdbcDriver {
        pack_id: format!("native:{}", descriptor.implementation),
        name: format!("{display_name} (native Rust)"),
        version: "native".to_owned(),
        driver_id: descriptor.id.to_owned(),
        driver_class: format!("rust:{}", descriptor.implementation),
        artifact_count: 0,
        artifact_bytes: "0".to_owned(),
    }
}

/// Resolves the native implementation for a persisted datasource driver ID.
pub(crate) fn native_driver_for_datasource_driver_id(
    registry: &NativeDriverRegistry,
    datasource_driver_id: &str,
    managed_drivers: &[JdbcDriver],
) -> Option<Arc<dyn NativeDriver>> {
    if let Some(driver) = registry.driver_for_datasource_driver_id(datasource_driver_id) {
        return Some(driver);
    }

    let managed_descriptor = managed_drivers.iter().find(|driver| {
        driver
            .driver_id
            .eq_ignore_ascii_case(datasource_driver_id.trim())
    })?;
    let mut matches = registry
        .descriptors()
        .filter(|descriptor| jdbc_driver_matches_descriptor(managed_descriptor, descriptor));
    let driver_id = matches.next()?.id;
    if matches.next().is_some() {
        return None;
    }
    registry.driver_for_datasource_driver_id(driver_id)
}

fn jdbc_driver_matches_descriptor(
    jdbc_driver: &JdbcDriver,
    descriptor: &NativeDriverDescriptor,
) -> bool {
    let compatibility_values = [
        jdbc_driver.pack_id.as_str(),
        jdbc_driver.name.as_str(),
        jdbc_driver.driver_id.as_str(),
        jdbc_driver.driver_class.as_str(),
    ];
    descriptor
        .compatibility_aliases
        .iter()
        .copied()
        .filter_map(|alias| {
            let alias = alias.trim().to_ascii_lowercase();
            (!alias.is_empty()).then_some(alias)
        })
        .any(|alias| {
            compatibility_values
                .iter()
                .any(|value| value.to_ascii_lowercase().contains(&alias))
        })
}

fn copy_name(name: &str) -> String {
    let candidate = format!("{name} Copy");
    if candidate.len() <= 512 {
        candidate
    } else {
        name.to_owned()
    }
}

fn unique_non_empty_ids(ids: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut seen = HashSet::with_capacity(ids.len());
    let mut unique = Vec::with_capacity(ids.len());
    for id in ids {
        if id.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_datasource_export",
                "datasource ids cannot be empty",
            ));
        }
        if seen.insert(id.clone()) {
            unique.push(id);
        }
    }
    Ok(unique)
}

fn portable_connection(
    connection: &DatasourceConnection,
) -> Result<PortableDatasourceConnection, AppError> {
    Ok(PortableDatasourceConnection {
        jdbc_url: sanitize_jdbc_url(&connection.jdbc_url)?,
        properties: connection
            .properties
            .iter()
            .filter(|property| !property.sensitive && !is_sensitive_key(&property.key))
            .map(|property| PortableDatasourceProperty {
                key: property.key.clone(),
                value: property.value.clone(),
            })
            .collect(),
        read_only: connection.read_only,
        ssh: connection.ssh.as_ref().map(project_ssh),
    })
}

fn imported_connection(
    connection: PortableDatasourceConnection,
) -> Result<DatasourceConnection, AppError> {
    if connection.jdbc_url.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_datasource_import",
            "portable JDBC URL cannot be empty",
        ));
    }
    if connection
        .properties
        .iter()
        .any(|property| is_sensitive_key(&property.key))
    {
        return Err(AppError::invalid(
            "unsafe_datasource_import",
            "portable datasource properties cannot contain credentials",
        ));
    }
    Ok(DatasourceConnection {
        jdbc_url: sanitize_jdbc_url(&connection.jdbc_url)?,
        properties: connection
            .properties
            .into_iter()
            .map(|property| DatasourceConnectionProperty {
                key: property.key,
                value: property.value,
                sensitive: false,
            })
            .collect(),
        read_only: connection.read_only,
        ssh: connection.ssh.map(imported_ssh),
    })
}

fn imported_ssh(ssh: chat2db_contract::SshTunnelEditProjection) -> SshTunnelConfig {
    let authentication = match ssh.authentication_type {
        SshAuthenticationType::Password => SshAuthentication::Password {
            password: String::new(),
        },
        SshAuthenticationType::PrivateKey => SshAuthentication::PrivateKey {
            key_file: ssh.key_file.unwrap_or_default(),
            passphrase: None,
        },
    };
    SshTunnelConfig {
        host_name: ssh.host_name,
        port: ssh.port,
        user_name: ssh.user_name,
        authentication,
        host_key_verification: ssh.host_key_verification,
        local_port: ssh.local_port,
    }
}

fn sanitize_jdbc_url(jdbc_url: &str) -> Result<String, AppError> {
    let jdbc_url = jdbc_url.trim();
    let (prefix, raw_url) = jdbc_url
        .strip_prefix("jdbc:")
        .map_or(("", jdbc_url), |url| ("jdbc:", url));
    let mut parsed = Url::parse(raw_url).map_err(|_| {
        AppError::invalid(
            "unsafe_datasource_export",
            "the datasource URL cannot be exported safely",
        )
    })?;
    parsed.set_username("").map_err(|()| AppError::internal())?;
    parsed
        .set_password(None)
        .map_err(|()| AppError::internal())?;
    parsed.set_fragment(None);
    let retained_query = parsed
        .query_pairs()
        .filter(|(key, _)| !is_sensitive_key(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    {
        let mut query = parsed.query_pairs_mut();
        query.clear();
        query.extend_pairs(retained_query);
    }
    Ok(format!("{prefix}{parsed}"))
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    key.contains("password")
        || key.contains("passwd")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("credential")
        || key.contains("privatekey")
        || key.contains("passphrase")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use chat2db_contract::{
        CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
        ExportCommunityDatasourcesRequest, JdbcDriver, NativeDriverAction, SshAuthentication,
        SshAuthenticationType, SshHostKeyVerification, SshTunnelConfig,
    };
    use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
    use tempfile::TempDir;

    use super::{
        CloneDatasourceRequest, jdbc_driver_from_descriptor, native_driver_for_datasource_driver_id,
    };
    use crate::{Application, native_driver::NativeDriverRegistry};

    #[derive(Debug, Default)]
    struct MemoryVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretVault for MemoryVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            reference: &SecretRef,
            value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            self.values.lock().expect("vault lock").insert(
                reference.as_str().to_owned(),
                value.expose_secret().to_vec(),
            );
            Ok(())
        }

        fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(self
                .values
                .lock()
                .expect("vault lock")
                .get(reference.as_str())
                .cloned()
                .map(SecretValue::new))
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
            self.values
                .lock()
                .expect("vault lock")
                .remove(reference.as_str());
            Ok(())
        }
    }

    fn application() -> (TempDir, Application) {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect("storage opens");
        (directory, Application::with_storage(storage))
    }

    fn connection() -> DatasourceConnection {
        DatasourceConnection {
            jdbc_url:
                "jdbc:mysql://url-user:url-password@localhost:3306/demo?token=hidden&useSSL=false"
                    .to_owned(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "root".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "sentinel-password".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: false,
            ssh: Some(SshTunnelConfig {
                host_name: "bastion.internal".to_owned(),
                port: 22,
                user_name: "ssh-user".to_owned(),
                authentication: SshAuthentication::Password {
                    password: "sentinel-ssh-password".to_owned(),
                },
                host_key_verification: SshHostKeyVerification::KnownHosts,
                local_port: None,
            }),
        }
    }

    #[tokio::test]
    async fn export_import_never_overwrites_or_serializes_credentials() {
        let (_directory, application) = application();
        let existing = application
            .create_datasource(CreateDatasourceRequest {
                name: "Existing".to_owned(),
                driver_id: "mysql".to_owned(),
                connection: Some(connection()),
            })
            .await
            .expect("datasource creates");
        let mut document = application
            .export_community_datasources(ExportCommunityDatasourcesRequest {
                datasource_ids: vec![existing.id.clone()],
            })
            .await
            .expect("datasource exports");
        let json = serde_json::to_string(&document).expect("document serializes");
        for forbidden in [
            "sentinel-password",
            "sentinel-ssh-password",
            "url-password",
            "hidden",
        ] {
            assert!(!json.contains(forbidden), "export leaked {forbidden}");
        }
        let exported_ssh = document.datasources[0]
            .connection
            .as_ref()
            .and_then(|connection| connection.ssh.as_ref())
            .expect("SSH metadata exports");
        assert_eq!(exported_ssh.host_name, "bastion.internal");
        assert_eq!(
            exported_ssh.authentication_type,
            SshAuthenticationType::Password
        );

        document.datasources[0].source_id = Some(existing.id.clone());
        let imported = application
            .import_community_datasources(document)
            .await
            .expect("datasource imports");
        assert_eq!(imported.count, 1);
        assert_ne!(imported.created[0].id, existing.id);

        let storage = application.storage().expect("storage configured");
        let (_, secret) = storage
            .get_datasource_with_secret(&existing.id)
            .expect("existing secret resolves");
        let secret = secret.expect("existing secret remains");
        let existing_connection: DatasourceConnection =
            serde_json::from_slice(secret.expose_secret()).expect("connection decodes");
        assert!(existing_connection.properties.iter().any(|property| {
            property.key == "password" && property.value == "sentinel-password"
        }));
        assert!(matches!(
            existing_connection
                .ssh
                .expect("existing SSH config remains")
                .authentication,
            SshAuthentication::Password { password } if password == "sentinel-ssh-password"
        ));
        let (_, imported_secret) = storage
            .get_datasource_with_secret(&imported.created[0].id)
            .expect("imported secret resolves");
        let imported_connection: DatasourceConnection = serde_json::from_slice(
            imported_secret
                .expect("imported descriptor exists")
                .expose_secret(),
        )
        .expect("imported descriptor decodes");
        assert!(matches!(
            imported_connection
                .ssh
                .expect("SSH metadata imports")
                .authentication,
            SshAuthentication::Password { password } if password.is_empty()
        ));
    }

    #[tokio::test]
    async fn clone_uses_a_distinct_vault_reference_and_close_is_stateless() {
        let (_directory, application) = application();
        let original = application
            .create_datasource(CreateDatasourceRequest {
                name: "Source".to_owned(),
                driver_id: "mysql".to_owned(),
                connection: Some(connection()),
            })
            .await
            .expect("source creates");
        let duplicate = application
            .clone_datasource(CloneDatasourceRequest {
                id: original.id.clone(),
                name: None,
            })
            .await
            .expect("datasource clones");
        let storage = application.storage().expect("storage configured");
        let original_record = storage
            .get_datasource(&original.id)
            .expect("source reads")
            .expect("source exists");
        let duplicate_record = storage
            .get_datasource(&duplicate.id)
            .expect("clone reads")
            .expect("clone exists");
        assert_ne!(original_record.secret_ref, duplicate_record.secret_ref);
        let (_, duplicate_secret) = storage
            .get_datasource_with_secret(&duplicate.id)
            .expect("clone secret resolves");
        let duplicate_connection: DatasourceConnection = serde_json::from_slice(
            duplicate_secret
                .expect("clone descriptor exists")
                .expose_secret(),
        )
        .expect("clone descriptor decodes");
        assert!(matches!(
            duplicate_connection
                .ssh
                .expect("clone retains SSH")
                .authentication,
            SshAuthentication::Password { password } if password == "sentinel-ssh-password"
        ));
        let closed = application
            .close_datasource_compatibility(&original.id)
            .await
            .expect("close succeeds");
        assert_eq!(closed.closed_connections, 0);
    }

    #[test]
    fn native_mysql_inventory_and_mutations_never_require_a_jar() {
        let application = Application::new();
        let drivers = application.list_drivers();
        assert!(drivers.items.iter().any(|driver| {
            driver.driver_id == "mysql"
                && driver.driver_class == "rust:mysql_async"
                && driver.artifact_count == 0
        }));
        let compatibility = application
            .native_driver_compatibility("MYSQL", NativeDriverAction::Download)
            .expect("native compatibility resolves");
        assert!(!compatibility.artifact_required);
        assert!(!compatibility.changed);
    }

    #[test]
    fn native_descriptor_preserves_the_existing_mysql_jdbc_wire_shape() {
        let registry = NativeDriverRegistry::built_in();
        let descriptor = registry
            .descriptors()
            .next()
            .expect("built-in MySQL descriptor exists");

        assert_eq!(
            jdbc_driver_from_descriptor(descriptor),
            JdbcDriver {
                pack_id: "native:mysql_async".to_owned(),
                name: "MySQL (native Rust)".to_owned(),
                version: "native".to_owned(),
                driver_id: "mysql".to_owned(),
                driver_class: "rust:mysql_async".to_owned(),
                artifact_count: 0,
                artifact_bytes: "0".to_owned(),
            }
        );
    }

    #[test]
    fn managed_jdbc_driver_id_resolves_through_the_datasource_compatibility_boundary() {
        let registry = NativeDriverRegistry::built_in();
        let managed_drivers = vec![JdbcDriver {
            pack_id: "mysql-connector-j".to_owned(),
            name: "MySQL JDBC".to_owned(),
            version: "9".to_owned(),
            driver_id: "managed-mysql".to_owned(),
            driver_class: "com.mysql.cj.jdbc.Driver".to_owned(),
            artifact_count: 1,
            artifact_bytes: "1".to_owned(),
        }];

        let driver =
            native_driver_for_datasource_driver_id(&registry, "managed-mysql", &managed_drivers)
                .expect("managed MySQL descriptor resolves to the native implementation");
        assert_eq!(driver.descriptor().id, "mysql");
    }
}
