use std::collections::HashSet;

use chat2db_contract::{
    DatasourceConnection, DatasourceConnectionProperty, DatasourceEditProjection,
    DatasourceSecretChange, PortableDatasourceProperty, SshAuthentication, SshAuthenticationType,
    SshTunnelConfig, SshTunnelEditProjection, UpdateDatasourceRequest,
};
use chat2db_storage::StorageError;
use url::Url;

use crate::{AppError, Application, storage_call};

struct ProjectedConnection {
    jdbc_url: String,
    username: Option<String>,
    properties: Vec<PortableDatasourceProperty>,
    read_only: bool,
    ssh: Option<SshTunnelEditProjection>,
}

impl Application {
    /// Returns the connection fields required by an edit form without exposing credentials.
    ///
    /// # Errors
    ///
    /// Returns datasource, vault, persisted-descriptor, or URL validation failures.
    pub async fn get_datasource_edit_projection(
        &self,
        id: &str,
    ) -> Result<DatasourceEditProjection, AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let (record, connection) = storage_call(move || {
            let (record, secret) = storage.get_datasource_with_secret(&id)?;
            let connection = secret
                .as_ref()
                .map(|secret| decode_connection(secret.expose_secret()))
                .transpose()?;
            Ok((record, connection))
        })
        .await?;

        let has_secret = connection.is_some();
        let projected = connection.map_or_else(
            || {
                Ok(ProjectedConnection {
                    jdbc_url: String::new(),
                    username: None,
                    properties: Vec::new(),
                    read_only: false,
                    ssh: None,
                })
            },
            |connection| project_connection(&connection),
        )?;
        Ok(DatasourceEditProjection {
            id: record.id,
            name: record.name,
            driver_id: record.driver_id,
            jdbc_url: projected.jdbc_url,
            username: projected.username,
            properties: projected.properties,
            read_only: projected.read_only,
            ssh: projected.ssh,
            has_secret,
            revision: record.revision.to_string(),
        })
    }

    /// Applies an edit-form replacement while retaining omitted or blank sensitive values.
    ///
    /// The ordinary `update_datasource` method retains strict full-replacement semantics. This
    /// compatibility entry point is intentionally separate because the retained Community form
    /// never receives stored passwords and submits an empty password when it was not changed.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, revision-conflict, vault, or storage failures.
    pub async fn update_datasource_preserving_secrets(
        &self,
        id: &str,
        request: UpdateDatasourceRequest,
    ) -> Result<chat2db_contract::Datasource, AppError> {
        let UpdateDatasourceRequest {
            expected_revision,
            name,
            driver_id,
            secret_change,
        } = request;
        let DatasourceSecretChange::Replace {
            connection: incoming,
        } = secret_change
        else {
            return self
                .update_datasource(
                    id,
                    UpdateDatasourceRequest {
                        expected_revision,
                        name,
                        driver_id,
                        secret_change,
                    },
                )
                .await;
        };

        let expected = expected_revision.parse::<u64>().map_err(|_| {
            AppError::invalid(
                "invalid_numeric_value",
                "expectedRevision must be an unsigned decimal integer",
            )
        })?;
        let storage = self.require_storage()?;
        let datasource_id = id.to_owned();
        let old_connection = storage_call(move || {
            let (record, secret) = storage.get_datasource_with_secret(&datasource_id)?;
            if record.revision != expected {
                return Err(StorageError::RevisionConflict {
                    id: datasource_id,
                    expected,
                    actual: Some(record.revision),
                });
            }
            secret
                .as_ref()
                .map(|secret| decode_connection(secret.expose_secret()))
                .transpose()
        })
        .await?;
        let connection = match old_connection {
            Some(old) => merge_preserved_secrets(old, incoming)?,
            None => incoming,
        };

        self.update_datasource(
            id,
            UpdateDatasourceRequest {
                expected_revision,
                name,
                driver_id,
                secret_change: DatasourceSecretChange::Replace { connection },
            },
        )
        .await
    }
}

fn decode_connection(bytes: &[u8]) -> Result<DatasourceConnection, StorageError> {
    serde_json::from_slice(bytes).map_err(|_| {
        StorageError::InvalidDatasource("stored datasource connection descriptor is invalid")
    })
}

fn project_connection(connection: &DatasourceConnection) -> Result<ProjectedConnection, AppError> {
    let url_username = jdbc_url_username(&connection.jdbc_url)?;
    let username = connection
        .properties
        .iter()
        .find(|property| is_username_key(&property.key))
        .map(|property| property.value.clone())
        .filter(|value| !value.is_empty())
        .or(url_username);
    let properties = connection
        .properties
        .iter()
        .filter(|property| {
            !property.sensitive
                && !is_sensitive_key(&property.key)
                && !is_username_key(&property.key)
        })
        .map(|property| PortableDatasourceProperty {
            key: property.key.clone(),
            value: property.value.clone(),
        })
        .collect();
    Ok(ProjectedConnection {
        jdbc_url: sanitize_jdbc_url(&connection.jdbc_url)?,
        username,
        properties,
        read_only: connection.read_only,
        ssh: connection.ssh.as_ref().map(project_ssh),
    })
}

pub(crate) fn project_ssh(config: &SshTunnelConfig) -> SshTunnelEditProjection {
    let (authentication_type, key_file) = match &config.authentication {
        SshAuthentication::Password { .. } => (SshAuthenticationType::Password, None),
        SshAuthentication::PrivateKey { key_file, .. } => {
            (SshAuthenticationType::PrivateKey, Some(key_file.clone()))
        }
    };
    SshTunnelEditProjection {
        host_name: config.host_name.clone(),
        port: config.port,
        user_name: config.user_name.clone(),
        local_port: config.local_port,
        authentication_type,
        key_file,
        host_key_verification: config.host_key_verification,
    }
}

fn merge_preserved_secrets(
    mut old: DatasourceConnection,
    mut incoming: DatasourceConnection,
) -> Result<DatasourceConnection, AppError> {
    move_url_userinfo_to_properties(&mut old)?;
    move_url_userinfo_to_properties(&mut incoming)?;
    incoming.jdbc_url = merge_sensitive_query(&old.jdbc_url, &incoming.jdbc_url)?;
    incoming.ssh = merge_preserved_ssh(old.ssh.take(), incoming.ssh.take())?;

    for property in &mut incoming.properties {
        if is_sensitive_key(&property.key) {
            property.sensitive = true;
        }
    }
    for old_property in old
        .properties
        .into_iter()
        .filter(|property| property.sensitive || is_sensitive_key(&property.key))
    {
        match incoming
            .properties
            .iter_mut()
            .find(|property| property.key.eq_ignore_ascii_case(&old_property.key))
        {
            Some(property) if property.value.trim().is_empty() => {
                property.value = old_property.value;
                property.sensitive = true;
            }
            Some(_) => {}
            None => incoming.properties.push(DatasourceConnectionProperty {
                sensitive: true,
                ..old_property
            }),
        }
    }
    Ok(incoming)
}

fn merge_preserved_ssh(
    old: Option<SshTunnelConfig>,
    incoming: Option<SshTunnelConfig>,
) -> Result<Option<SshTunnelConfig>, AppError> {
    let Some(mut incoming) = incoming else {
        return Ok(None);
    };
    match &mut incoming.authentication {
        SshAuthentication::Password { password } if password.is_empty() => {
            let Some(SshTunnelConfig {
                authentication: SshAuthentication::Password { password: old },
                ..
            }) = old
            else {
                return Err(AppError::invalid(
                    "missing_ssh_password",
                    "SSH password is required when password authentication is selected",
                ));
            };
            *password = old;
        }
        SshAuthentication::PrivateKey {
            key_file,
            passphrase,
        } => {
            if let Some(SshTunnelConfig {
                authentication:
                    SshAuthentication::PrivateKey {
                        key_file: old_key_file,
                        passphrase: old_passphrase,
                    },
                ..
            }) = old
            {
                if key_file.trim().is_empty() {
                    *key_file = old_key_file;
                }
                if passphrase.as_deref().is_none_or(str::is_empty) {
                    *passphrase = old_passphrase;
                }
            }
            if key_file.trim().is_empty() {
                return Err(AppError::invalid(
                    "missing_ssh_private_key",
                    "SSH private-key path is required when private-key authentication is selected",
                ));
            }
        }
        SshAuthentication::Password { .. } => {}
    }
    Ok(Some(incoming))
}

fn move_url_userinfo_to_properties(connection: &mut DatasourceConnection) -> Result<(), AppError> {
    let (prefix, mut parsed) = parse_jdbc_url(&connection.jdbc_url, "invalid_datasource_url")?;
    let username = parsed.username().to_owned();
    let password = parsed.password().map(str::to_owned);
    parsed.set_username("").map_err(|()| AppError::internal())?;
    parsed
        .set_password(None)
        .map_err(|()| AppError::internal())?;
    connection.jdbc_url = format!("{prefix}{parsed}");
    if !username.is_empty()
        && !connection
            .properties
            .iter()
            .any(|property| is_username_key(&property.key))
    {
        connection.properties.push(DatasourceConnectionProperty {
            key: "user".to_owned(),
            value: username,
            sensitive: false,
        });
    }
    if let Some(password) = password
        && !connection
            .properties
            .iter()
            .any(|property| property.key.eq_ignore_ascii_case("password"))
    {
        connection.properties.push(DatasourceConnectionProperty {
            key: "password".to_owned(),
            value: password,
            sensitive: true,
        });
    }
    Ok(())
}

fn merge_sensitive_query(old_url: &str, incoming_url: &str) -> Result<String, AppError> {
    let (_, old) = parse_jdbc_url(old_url, "invalid_datasource_url")?;
    let (prefix, mut incoming) = parse_jdbc_url(incoming_url, "invalid_datasource_url")?;
    let mut incoming_pairs = incoming
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut incoming_keys = incoming_pairs
        .iter()
        .map(|(key, _)| key.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    for (key, value) in old.query_pairs() {
        let normalized = key.to_ascii_lowercase();
        if is_sensitive_key(&key) && incoming_keys.insert(normalized) {
            incoming_pairs.push((key.into_owned(), value.into_owned()));
        }
    }
    {
        let mut query = incoming.query_pairs_mut();
        query.clear();
        query.extend_pairs(incoming_pairs);
    }
    Ok(format!("{prefix}{incoming}"))
}

pub(crate) fn sanitize_jdbc_url(jdbc_url: &str) -> Result<String, AppError> {
    let (prefix, mut parsed) = parse_jdbc_url(jdbc_url, "unsafe_datasource_projection")?;
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

fn jdbc_url_username(jdbc_url: &str) -> Result<Option<String>, AppError> {
    let (_, parsed) = parse_jdbc_url(jdbc_url, "unsafe_datasource_projection")?;
    Ok((!parsed.username().is_empty()).then(|| parsed.username().to_owned()))
}

fn parse_jdbc_url(jdbc_url: &str, code: &'static str) -> Result<(&'static str, Url), AppError> {
    let jdbc_url = jdbc_url.trim();
    let (prefix, raw_url) = jdbc_url
        .strip_prefix("jdbc:")
        .map_or(("", jdbc_url), |url| ("jdbc:", url));
    let parsed = Url::parse(raw_url).map_err(|_| {
        AppError::invalid(code, "the datasource JDBC URL cannot be processed safely")
    })?;
    Ok((prefix, parsed))
}

pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    key.contains("password")
        || key.contains("passwd")
        || key == "pwd"
        || key.contains("secret")
        || key.contains("token")
        || key.contains("credential")
        || key.contains("privatekey")
        || key.contains("private_key")
        || key.contains("passphrase")
        || key.contains("apikey")
        || key.contains("api_key")
        || key.contains("api-key")
        || key.contains("accesskey")
        || key.contains("access_key")
}

fn is_username_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "user" | "username" | "user_name"
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use chat2db_contract::{
        CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
        DatasourceSecretChange, SshAuthentication, SshAuthenticationType, SshHostKeyVerification,
        SshTunnelConfig, UpdateDatasourceRequest,
    };
    use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
    use tempfile::TempDir;

    use crate::Application;

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

    fn property(key: &str, value: &str, sensitive: bool) -> DatasourceConnectionProperty {
        DatasourceConnectionProperty {
            key: key.to_owned(),
            value: value.to_owned(),
            sensitive,
        }
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn edit_projection_redacts_credentials_and_compat_update_keeps_them() {
        let (_directory, application) = application();
        let created = application
            .create_datasource(CreateDatasourceRequest {
                name: "Original".to_owned(),
                driver_id: "mysql".to_owned(),
                connection: Some(DatasourceConnection {
                    jdbc_url: "jdbc:mysql://url-user:url-password@localhost:3306/old?token=url-token&useSSL=false".to_owned(),
                    properties: vec![
                        property("user", "root", false),
                        property("password", "stored-password", true),
                        property("connectionTimeZone", "UTC", false),
                        property("apiToken", "property-token", false),
                    ],
                    read_only: false,
                    ssh: Some(SshTunnelConfig {
                        host_name: "ssh-old.internal".to_owned(),
                        port: 22,
                        user_name: "ssh-user".to_owned(),
                        authentication: SshAuthentication::Password {
                            password: "stored-ssh-password".to_owned(),
                        },
                        host_key_verification: SshHostKeyVerification::KnownHosts,
                        local_port: None,
                    }),
                }),
            })
            .await
            .expect("datasource creates");

        let projection = application
            .get_datasource_edit_projection(&created.id)
            .await
            .expect("projection loads");
        assert_eq!(projection.username.as_deref(), Some("root"));
        assert_eq!(
            projection.jdbc_url,
            "jdbc:mysql://localhost:3306/old?useSSL=false"
        );
        assert_eq!(projection.properties.len(), 1);
        let ssh = projection.ssh.as_ref().expect("SSH projection exists");
        assert_eq!(ssh.host_name, "ssh-old.internal");
        assert_eq!(ssh.authentication_type, SshAuthenticationType::Password);
        let serialized = serde_json::to_string(&projection).expect("projection serializes");
        for forbidden in [
            "url-user",
            "url-password",
            "url-token",
            "stored-password",
            "property-token",
            "stored-ssh-password",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "projection leaked {forbidden}"
            );
        }

        let updated = application
            .update_datasource_preserving_secrets(
                &created.id,
                UpdateDatasourceRequest {
                    expected_revision: projection.revision,
                    name: "Updated".to_owned(),
                    driver_id: "mysql".to_owned(),
                    secret_change: DatasourceSecretChange::Replace {
                        connection: DatasourceConnection {
                            jdbc_url: "jdbc:mysql://localhost:3306/new?useSSL=true".to_owned(),
                            properties: vec![
                                property("user", "new-user", false),
                                property("connectionTimeZone", "Asia/Shanghai", false),
                            ],
                            read_only: true,
                            ssh: Some(SshTunnelConfig {
                                host_name: "ssh-new.internal".to_owned(),
                                port: 2222,
                                user_name: "new-ssh-user".to_owned(),
                                authentication: SshAuthentication::Password {
                                    password: String::new(),
                                },
                                host_key_verification: SshHostKeyVerification::KnownHosts,
                                local_port: Some(33060),
                            }),
                        },
                    },
                },
            )
            .await
            .expect("compatibility update succeeds");
        assert_eq!(updated.revision, "2");

        let storage = application.storage().expect("storage configured");
        let (_, secret) = storage
            .get_datasource_with_secret(&created.id)
            .expect("stored secret loads");
        let connection: DatasourceConnection =
            serde_json::from_slice(secret.expect("stored descriptor exists").expose_secret())
                .expect("stored descriptor decodes");
        assert!(connection.jdbc_url.contains("/new"));
        assert!(connection.jdbc_url.contains("token=url-token"));
        assert!(
            connection
                .properties
                .iter()
                .any(|property| { property.key == "user" && property.value == "new-user" })
        );
        assert!(
            connection.properties.iter().any(|property| {
                property.key == "password" && property.value == "stored-password"
            })
        );
        assert!(
            connection.properties.iter().any(|property| {
                property.key == "apiToken" && property.value == "property-token"
            })
        );
        let ssh = connection.ssh.expect("SSH config remains installed");
        assert_eq!(ssh.host_name, "ssh-new.internal");
        assert_eq!(ssh.port, 2222);
        assert!(matches!(
            ssh.authentication,
            SshAuthentication::Password { password } if password == "stored-ssh-password"
        ));
        let stale = application
            .update_datasource_preserving_secrets(
                &created.id,
                UpdateDatasourceRequest {
                    expected_revision: "1".to_owned(),
                    name: "Stale".to_owned(),
                    driver_id: "mysql".to_owned(),
                    secret_change: DatasourceSecretChange::Keep,
                },
            )
            .await
            .expect_err("SSH updates must not bypass datasource revision CAS");
        assert_eq!(stale.api_error().code, "revision_conflict");
    }

    #[tokio::test]
    async fn private_key_projection_and_blank_passphrase_update_are_secret_safe() {
        let (_directory, application) = application();
        let created = application
            .create_datasource(CreateDatasourceRequest {
                name: "Private key".to_owned(),
                driver_id: "mysql".to_owned(),
                connection: Some(DatasourceConnection {
                    jdbc_url: "jdbc:mysql://db.internal:3306/app".to_owned(),
                    properties: Vec::new(),
                    read_only: false,
                    ssh: Some(SshTunnelConfig {
                        host_name: "bastion.internal".to_owned(),
                        port: 22,
                        user_name: "developer".to_owned(),
                        authentication: SshAuthentication::PrivateKey {
                            key_file: "/keys/id_ed25519".to_owned(),
                            passphrase: Some("stored-passphrase".to_owned()),
                        },
                        host_key_verification: SshHostKeyVerification::KnownHosts,
                        local_port: None,
                    }),
                }),
            })
            .await
            .expect("datasource creates");
        let projection = application
            .get_datasource_edit_projection(&created.id)
            .await
            .expect("projection loads");
        let ssh = projection.ssh.as_ref().expect("SSH projection exists");
        assert_eq!(ssh.authentication_type, SshAuthenticationType::PrivateKey);
        assert_eq!(ssh.key_file.as_deref(), Some("/keys/id_ed25519"));
        assert!(
            !serde_json::to_string(&projection)
                .expect("projection serializes")
                .contains("stored-passphrase")
        );

        application
            .update_datasource_preserving_secrets(
                &created.id,
                UpdateDatasourceRequest {
                    expected_revision: projection.revision,
                    name: "Private key updated".to_owned(),
                    driver_id: "mysql".to_owned(),
                    secret_change: DatasourceSecretChange::Replace {
                        connection: DatasourceConnection {
                            jdbc_url: "jdbc:mysql://db.internal:3306/app".to_owned(),
                            properties: Vec::new(),
                            read_only: false,
                            ssh: Some(SshTunnelConfig {
                                host_name: "bastion.internal".to_owned(),
                                port: 22,
                                user_name: "developer".to_owned(),
                                authentication: SshAuthentication::PrivateKey {
                                    key_file: "/keys/id_ed25519".to_owned(),
                                    passphrase: None,
                                },
                                host_key_verification: SshHostKeyVerification::KnownHosts,
                                local_port: None,
                            }),
                        },
                    },
                },
            )
            .await
            .expect("blank passphrase retains the stored value");
        let storage = application.storage().expect("storage configured");
        let (_, secret) = storage
            .get_datasource_with_secret(&created.id)
            .expect("stored secret loads");
        let connection: DatasourceConnection =
            serde_json::from_slice(secret.expect("descriptor exists").expose_secret())
                .expect("descriptor decodes");
        assert!(matches!(
            connection.ssh.expect("SSH remains").authentication,
            SshAuthentication::PrivateKey { passphrase: Some(value), .. }
                if value == "stored-passphrase"
        ));
    }
}
