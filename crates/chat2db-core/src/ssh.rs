use std::{
    collections::HashMap,
    future::Future,
    net::Ipv4Addr,
    sync::{Arc, OnceLock, Weak},
    time::Duration,
};

use chat2db_contract::{
    SshAuthentication, SshConnectionTestResult, SshDatasourcePreConnectRequest,
    SshDatasourcePreConnectResult, SshHostKeyVerification, SshTunnelConfig,
};
use russh::{
    Disconnect,
    client::{self, Config, Handle},
    keys::{self, key::PrivateKeyWithHashAlg, ssh_key},
};
use sha2::{Digest, Sha256};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::{Mutex, oneshot},
    task::{JoinHandle, JoinSet},
};
use url::Url;

use crate::{AppError, Application};

const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const SSH_AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const SSH_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const SSH_CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const SSH_FORWARD_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const SSH_SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(600);
const SSH_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const MAX_SSH_FORWARD_CONNECTIONS: usize = 16;
const MAX_SSH_HOST_BYTES: usize = 255;
const MAX_SSH_USER_BYTES: usize = 255;
const MAX_SSH_SECRET_BYTES: usize = 64 * 1024;
const MAX_SSH_KEY_PATH_BYTES: usize = 4 * 1024;
const MYSQL_DEFAULT_PORT: u16 = 3_306;

#[derive(Clone, Copy)]
pub(crate) enum SshTunnelIdentity<'a> {
    Datasource {
        datasource_id: &'a str,
        revision: u64,
    },
    Ephemeral,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct SshTunnelKey([u8; 32]);

impl SshTunnelKey {
    fn new(
        identity: SshTunnelIdentity<'_>,
        config: &SshTunnelConfig,
        target_host: &str,
        target_port: u16,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"chat2db-ssh-tunnel-v1");
        match identity {
            SshTunnelIdentity::Datasource {
                datasource_id,
                revision,
            } => {
                digest.update([1]);
                hash_component(&mut digest, datasource_id.as_bytes());
                digest.update(revision.to_be_bytes());
            }
            SshTunnelIdentity::Ephemeral => digest.update([0]),
        }
        hash_component(&mut digest, config.host_name.as_bytes());
        digest.update(config.port.to_be_bytes());
        hash_component(&mut digest, config.user_name.as_bytes());
        digest.update([match config.host_key_verification {
            SshHostKeyVerification::KnownHosts => 0,
        }]);
        match config.local_port {
            Some(port) => {
                digest.update([1]);
                digest.update(port.to_be_bytes());
            }
            None => digest.update([0]),
        }
        match &config.authentication {
            SshAuthentication::Password { password } => {
                digest.update([0]);
                hash_component(&mut digest, password.as_bytes());
            }
            SshAuthentication::PrivateKey {
                key_file,
                passphrase,
            } => {
                digest.update([1]);
                hash_component(&mut digest, key_file.as_bytes());
                match passphrase {
                    Some(passphrase) => {
                        digest.update([1]);
                        hash_component(&mut digest, passphrase.as_bytes());
                    }
                    None => digest.update([0]),
                }
            }
        }
        hash_component(&mut digest, target_host.as_bytes());
        digest.update(target_port.to_be_bytes());
        Self(digest.finalize().into())
    }
}

fn hash_component(digest: &mut Sha256, value: &[u8]) {
    digest.update(value.len().to_be_bytes());
    digest.update(value);
}

#[derive(Default)]
struct SshTunnelRegistry {
    entries: Mutex<HashMap<SshTunnelKey, Weak<SshTunnelEntry>>>,
}

struct SshTunnelEntry {
    current: Mutex<Weak<SshTunnelInner>>,
}

impl SshTunnelRegistry {
    async fn acquire_with<F, Fut>(
        &self,
        key: SshTunnelKey,
        opener: F,
    ) -> Result<SshTunnel, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<SshTunnelInner, AppError>>,
    {
        let entry = {
            let mut entries = self.entries.lock().await;
            entries.retain(|_, entry| entry.strong_count() > 0);
            if let Some(entry) = entries.get(&key).and_then(Weak::upgrade) {
                entry
            } else {
                let entry = Arc::new(SshTunnelEntry {
                    current: Mutex::new(Weak::new()),
                });
                entries.insert(key, Arc::downgrade(&entry));
                entry
            }
        };

        let mut current = entry.current.lock().await;
        if let Some(inner) = current.upgrade() {
            if inner.is_active() {
                drop(current);
                return Ok(SshTunnel { inner, entry });
            }
            drop(inner);
            *current = Weak::new();
        }
        let inner = Arc::new(opener().await?);
        *current = Arc::downgrade(&inner);
        drop(current);
        Ok(SshTunnel { inner, entry })
    }
}

fn ssh_tunnel_registry() -> &'static SshTunnelRegistry {
    static REGISTRY: OnceLock<SshTunnelRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SshTunnelRegistry::default)
}

impl Application {
    /// Tests SSH transport, host-key verification, and authentication without opening a tunnel.
    ///
    /// # Errors
    ///
    /// Returns validation or a secret-safe SSH availability failure.
    pub async fn test_ssh_connection(
        &self,
        config: SshTunnelConfig,
    ) -> Result<SshConnectionTestResult, AppError> {
        let verification = config.host_key_verification;
        let mut session = connect_authenticated(&config).await?;
        disconnect(&mut session).await?;
        Ok(SshConnectionTestResult {
            verified: true,
            host_key_verification: verification,
        })
    }

    /// Tests an unsaved datasource directly or through an ephemeral SSH local forward.
    ///
    /// Only native `MySQL` uses this tunnel path. The listener binds to loopback, the database URL
    /// is rewritten in memory, and the SSH session is closed before the method returns.
    ///
    /// # Errors
    ///
    /// Returns validation, driver, SSH, or database failures without exposing credentials.
    pub async fn test_datasource_connection_with_ssh(
        &self,
        request: SshDatasourcePreConnectRequest,
    ) -> Result<SshDatasourcePreConnectResult, AppError> {
        let Some(ssh) = request.ssh else {
            self.test_datasource_connection(&request.driver_id, request.connection)
                .await?;
            return Ok(SshDatasourcePreConnectResult {
                verified: true,
                local_port: None,
            });
        };
        self.require_managed_driver(&request.driver_id)?;
        let driver = self
            .native_driver_for_driver_id(&request.driver_id)
            .ok_or_else(|| {
                AppError::invalid(
                    "ssh_driver_not_supported",
                    "SSH forwarding requires a native Rust driver",
                )
            })?;
        let mut forwarded = request.connection;
        forwarded.ssh = Some(ssh);
        let local_port = driver
            .connection()
            .test_connection_with_local_port(&forwarded)
            .await?
            .ok_or_else(AppError::internal)?;
        Ok(SshDatasourcePreConnectResult {
            verified: true,
            local_port: Some(local_port),
        })
    }
}

struct HostKeyHandler {
    host: String,
    port: u16,
}

impl client::Handler for HostKeyHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        keys::check_known_hosts(&self.host, self.port, server_public_key).map_err(Into::into)
    }
}

async fn connect_authenticated(
    config: &SshTunnelConfig,
) -> Result<Handle<HostKeyHandler>, AppError> {
    validate_ssh_config(config)?;
    let client_config = Arc::new(Config {
        nodelay: true,
        inactivity_timeout: Some(SSH_SESSION_IDLE_TIMEOUT),
        keepalive_interval: Some(SSH_KEEPALIVE_INTERVAL),
        keepalive_max: 3,
        ..Config::default()
    });
    let handler = HostKeyHandler {
        host: config.host_name.clone(),
        port: config.port,
    };
    let mut session = tokio::time::timeout(
        SSH_CONNECT_TIMEOUT,
        client::connect(
            client_config,
            (config.host_name.as_str(), config.port),
            handler,
        ),
    )
    .await
    .map_err(|_| ssh_unavailable())?
    .map_err(|_| ssh_unavailable())?;

    let authenticated = match &config.authentication {
        SshAuthentication::Password { password } => tokio::time::timeout(
            SSH_AUTH_TIMEOUT,
            session.authenticate_password(config.user_name.clone(), password.clone()),
        )
        .await
        .map_err(|_| ssh_unavailable())?
        .map_err(|_| ssh_unavailable())?
        .success(),
        SshAuthentication::PrivateKey {
            key_file,
            passphrase,
        } => {
            let key_file = key_file.clone();
            let passphrase = passphrase.clone();
            let private_key = tokio::task::spawn_blocking(move || {
                keys::load_secret_key(key_file, passphrase.as_deref())
            })
            .await
            .map_err(|_| AppError::internal())?
            .map_err(|_| {
                AppError::invalid(
                    "ssh_private_key_invalid",
                    "The selected SSH private key could not be loaded",
                )
            })?;
            let hash = tokio::time::timeout(SSH_AUTH_TIMEOUT, session.best_supported_rsa_hash())
                .await
                .map_err(|_| ssh_unavailable())?
                .map_err(|_| ssh_unavailable())?
                .flatten();
            tokio::time::timeout(
                SSH_AUTH_TIMEOUT,
                session.authenticate_publickey(
                    config.user_name.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(private_key), hash),
                ),
            )
            .await
            .map_err(|_| ssh_unavailable())?
            .map_err(|_| ssh_unavailable())?
            .success()
        }
    };
    if !authenticated {
        let _ = disconnect(&mut session).await;
        return Err(AppError::unavailable(
            "ssh_authentication_failed",
            "SSH authentication was rejected",
        ));
    }
    Ok(session)
}

pub(crate) struct SshTunnel {
    inner: Arc<SshTunnelInner>,
    entry: Arc<SshTunnelEntry>,
}

struct SshTunnelInner {
    local_port: u16,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), AppError>>>,
}

impl SshTunnel {
    pub(crate) async fn open(
        identity: SshTunnelIdentity<'_>,
        config: &SshTunnelConfig,
        target_host: String,
        target_port: u16,
    ) -> Result<Self, AppError> {
        let key = SshTunnelKey::new(identity, config, &target_host, target_port);
        ssh_tunnel_registry()
            .acquire_with(key, || async move {
                SshTunnelInner::open(config, target_host, target_port).await
            })
            .await
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.inner.local_port
    }

    pub(crate) async fn close(self) -> Result<(), AppError> {
        let Self { inner, entry } = self;
        let mut current = entry.current.lock().await;
        let own_tunnel = Arc::downgrade(&inner);
        match Arc::try_unwrap(inner) {
            Ok(inner) => {
                if current.ptr_eq(&own_tunnel) {
                    *current = Weak::new();
                }
                let result = inner.close().await;
                drop(current);
                result
            }
            Err(inner) => {
                drop(inner);
                drop(current);
                Ok(())
            }
        }
    }
}

impl SshTunnelInner {
    fn is_active(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    async fn open(
        config: &SshTunnelConfig,
        target_host: String,
        target_port: u16,
    ) -> Result<Self, AppError> {
        validate_ssh_config(config)?;
        let requested_port = config.local_port.unwrap_or(0);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, requested_port))
            .await
            .map_err(|_| {
                AppError::unavailable(
                    "ssh_tunnel_bind_failed",
                    "The SSH loopback tunnel port is unavailable",
                )
            })?;
        let local_port = listener.local_addr().map_err(|_| ssh_unavailable())?.port();
        let session = connect_authenticated(config).await?;
        let (shutdown, receiver) = oneshot::channel();
        let task = tokio::spawn(run_tunnel(
            session,
            listener,
            target_host,
            target_port,
            receiver,
        ));
        Ok(Self {
            local_port,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    async fn close(mut self) -> Result<(), AppError> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await.map_err(|_| AppError::internal())?
    }
}

impl Drop for SshTunnelInner {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_tunnel(
    mut session: Handle<HostKeyHandler>,
    listener: TcpListener,
    target_host: String,
    target_port: u16,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), AppError> {
    let mut transfers = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept(), if transfers.len() < MAX_SSH_FORWARD_CONNECTIONS => {
                let (socket, origin) = accepted.map_err(|_| ssh_unavailable())?;
                match tokio::time::timeout(
                    SSH_CHANNEL_OPEN_TIMEOUT,
                    session.channel_open_direct_tcpip(
                        target_host.clone(),
                        u32::from(target_port),
                        origin.ip().to_string(),
                        u32::from(origin.port()),
                    ),
                ).await
                {
                    Ok(Ok(channel)) => {
                        transfers.spawn(forward_connection(socket, channel.into_stream()));
                    }
                    Ok(Err(_)) | Err(_) => {
                        tracing::warn!("SSH server rejected a direct-tcpip channel");
                    }
                }
            }
            Some(_) = transfers.join_next(), if !transfers.is_empty() => {}
        }
    }
    disconnect(&mut session).await?;
    transfers.abort_all();
    while transfers.join_next().await.is_some() {}
    Ok(())
}

async fn forward_connection<S>(mut socket: TcpStream, mut channel: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match tokio::time::timeout(
        SSH_FORWARD_IDLE_TIMEOUT,
        copy_bidirectional(&mut socket, &mut channel),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(_)) => tracing::debug!("SSH forwarded connection closed with an I/O error"),
        Err(_) => tracing::debug!("SSH forwarded connection exceeded its idle lifetime"),
    }
}

async fn disconnect(session: &mut Handle<HostKeyHandler>) -> Result<(), AppError> {
    tokio::time::timeout(
        SSH_DISCONNECT_TIMEOUT,
        session.disconnect(Disconnect::ByApplication, "", "English"),
    )
    .await
    .map_err(|_| ssh_unavailable())?
    .map_err(|_| ssh_unavailable())
}

fn validate_ssh_config(config: &SshTunnelConfig) -> Result<(), AppError> {
    if config.host_name.trim().is_empty() || config.host_name.len() > MAX_SSH_HOST_BYTES {
        return Err(AppError::invalid(
            "invalid_ssh_config",
            "SSH hostname must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    if config.port == 0 {
        return Err(AppError::invalid(
            "invalid_ssh_config",
            "SSH port must be greater than zero",
        ));
    }
    if config.user_name.trim().is_empty() || config.user_name.len() > MAX_SSH_USER_BYTES {
        return Err(AppError::invalid(
            "invalid_ssh_config",
            "SSH username must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    if config.host_key_verification != SshHostKeyVerification::KnownHosts {
        return Err(AppError::invalid(
            "invalid_ssh_host_key_policy",
            "SSH host keys must be verified through the user's OpenSSH known_hosts file",
        ));
    }
    match &config.authentication {
        SshAuthentication::Password { password }
            if password.is_empty() || password.len() > MAX_SSH_SECRET_BYTES =>
        {
            Err(AppError::invalid(
                "invalid_ssh_config",
                "SSH password must be non-empty and at most 65536 UTF-8 bytes",
            ))
        }
        SshAuthentication::PrivateKey {
            key_file,
            passphrase,
        } if key_file.trim().is_empty()
            || key_file.len() > MAX_SSH_KEY_PATH_BYTES
            || passphrase
                .as_ref()
                .is_some_and(|value| value.len() > MAX_SSH_SECRET_BYTES) =>
        {
            Err(AppError::invalid(
                "invalid_ssh_config",
                "SSH private-key settings exceed their allowed size",
            ))
        }
        _ => Ok(()),
    }
}

pub(crate) fn mysql_target(jdbc_url: &str) -> Result<(String, u16), AppError> {
    let parsed = parse_mysql_url(jdbc_url)?;
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(invalid_mysql_ssh_url)?
        .to_owned();
    Ok((host, parsed.port().unwrap_or(MYSQL_DEFAULT_PORT)))
}

pub(crate) fn rewrite_mysql_target(jdbc_url: &str, local_port: u16) -> Result<String, AppError> {
    let has_jdbc_prefix = jdbc_url.trim().starts_with("jdbc:");
    let mut parsed = parse_mysql_url(jdbc_url)?;
    parsed
        .set_host(Some("127.0.0.1"))
        .map_err(|_| invalid_mysql_ssh_url())?;
    parsed
        .set_port(Some(local_port))
        .map_err(|()| invalid_mysql_ssh_url())?;
    let prefix = if has_jdbc_prefix { "jdbc:" } else { "" };
    Ok(format!("{prefix}{parsed}"))
}

fn parse_mysql_url(jdbc_url: &str) -> Result<Url, AppError> {
    let raw = jdbc_url
        .trim()
        .strip_prefix("jdbc:")
        .unwrap_or(jdbc_url.trim());
    let parsed = Url::parse(raw).map_err(|_| invalid_mysql_ssh_url())?;
    if parsed.scheme() != "mysql" {
        return Err(invalid_mysql_ssh_url());
    }
    Ok(parsed)
}

fn invalid_mysql_ssh_url() -> AppError {
    AppError::invalid(
        "invalid_mysql_ssh_url",
        "The MySQL URL does not contain a valid tunnel target",
    )
}

fn ssh_unavailable() -> AppError {
    AppError::unavailable(
        "ssh_connection_failed",
        "The SSH connection or tunnel could not be established",
    )
}

#[cfg(test)]
mod tests {
    use std::{
        net::Ipv4Addr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use chat2db_contract::{SshAuthentication, SshHostKeyVerification, SshTunnelConfig};
    use tokio::{net::TcpListener, sync::oneshot, task::JoinSet};

    use super::{
        AppError, SshTunnelIdentity, SshTunnelInner, SshTunnelKey, SshTunnelRegistry, mysql_target,
        rewrite_mysql_target, validate_ssh_config,
    };

    #[test]
    fn mysql_target_rewrite_preserves_database_and_query() {
        let url = "jdbc:mysql://db.internal:3307/example?useSSL=true";
        assert_eq!(
            mysql_target(url).expect("target parses"),
            ("db.internal".to_owned(), 3307)
        );
        let rewritten = rewrite_mysql_target(url, 41_223).expect("URL rewrites");
        assert_eq!(
            rewritten,
            "jdbc:mysql://127.0.0.1:41223/example?useSSL=true"
        );
    }

    #[test]
    fn ssh_config_requires_endpoint_user_and_authentication_material() {
        let mut config = SshTunnelConfig {
            host_name: "ssh.internal".to_owned(),
            port: 22,
            user_name: "developer".to_owned(),
            authentication: SshAuthentication::Password {
                password: "secret".to_owned(),
            },
            host_key_verification: SshHostKeyVerification::KnownHosts,
            local_port: None,
        };
        assert!(validate_ssh_config(&config).is_ok());
        config.port = 0;
        assert!(validate_ssh_config(&config).is_err());
        config.port = 22;
        config.authentication = SshAuthentication::Password {
            password: String::new(),
        };
        assert!(validate_ssh_config(&config).is_err());
    }

    #[test]
    fn tunnel_key_is_scoped_by_datasource_revision_config_and_target() {
        let config = ssh_config("first-secret", Some(43_210));
        let base = SshTunnelKey::new(
            SshTunnelIdentity::Datasource {
                datasource_id: "datasource-1",
                revision: 7,
            },
            &config,
            "mysql.internal",
            3_306,
        );
        let same = SshTunnelKey::new(
            SshTunnelIdentity::Datasource {
                datasource_id: "datasource-1",
                revision: 7,
            },
            &config,
            "mysql.internal",
            3_306,
        );
        assert!(base == same);

        let changed_secret = ssh_config("second-secret", Some(43_210));
        for changed in [
            SshTunnelKey::new(
                SshTunnelIdentity::Datasource {
                    datasource_id: "datasource-2",
                    revision: 7,
                },
                &config,
                "mysql.internal",
                3_306,
            ),
            SshTunnelKey::new(
                SshTunnelIdentity::Datasource {
                    datasource_id: "datasource-1",
                    revision: 8,
                },
                &config,
                "mysql.internal",
                3_306,
            ),
            SshTunnelKey::new(
                SshTunnelIdentity::Datasource {
                    datasource_id: "datasource-1",
                    revision: 7,
                },
                &changed_secret,
                "mysql.internal",
                3_306,
            ),
            SshTunnelKey::new(
                SshTunnelIdentity::Datasource {
                    datasource_id: "datasource-1",
                    revision: 7,
                },
                &config,
                "other.internal",
                3_306,
            ),
        ] {
            assert!(base != changed);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_leases_share_one_fixed_listener_until_the_last_close() {
        let port = unused_loopback_port().await;
        let registry = Arc::new(SshTunnelRegistry::default());
        let opens = Arc::new(AtomicUsize::new(0));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let mut tasks = JoinSet::new();

        for _ in 0..16 {
            let registry = Arc::clone(&registry);
            let opens = Arc::clone(&opens);
            let shutdowns = Arc::clone(&shutdowns);
            tasks.spawn(async move {
                registry
                    .acquire_with(SshTunnelKey([7; 32]), || async move {
                        opens.fetch_add(1, Ordering::SeqCst);
                        fake_tunnel_inner(port, shutdowns).await
                    })
                    .await
            });
        }

        let mut leases = Vec::new();
        while let Some(result) = tasks.join_next().await {
            leases.push(result.expect("lease task joins").expect("lease opens"));
        }
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        assert!(leases.iter().all(|lease| lease.local_port() == port));
        assert!(
            TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                .await
                .is_err()
        );

        while leases.len() > 1 {
            leases
                .pop()
                .expect("lease exists")
                .close()
                .await
                .expect("lease closes");
            assert_eq!(shutdowns.load(Ordering::SeqCst), 0);
            assert!(
                TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_err()
            );
        }
        leases
            .pop()
            .expect("last lease exists")
            .close()
            .await
            .expect("last lease closes");
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
        let rebound = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .expect("fixed port is released after the final lease");
        drop(rebound);
    }

    #[tokio::test]
    async fn changed_scope_never_reuses_an_old_fixed_port_tunnel() {
        let port = unused_loopback_port().await;
        let registry = SshTunnelRegistry::default();
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let old = registry
            .acquire_with(SshTunnelKey([1; 32]), || {
                fake_tunnel_inner(port, Arc::clone(&shutdowns))
            })
            .await
            .expect("old tunnel opens");

        let replacement = registry
            .acquire_with(SshTunnelKey([2; 32]), || {
                fake_tunnel_inner(port, Arc::clone(&shutdowns))
            })
            .await;
        assert!(
            replacement.is_err(),
            "a changed scope must not reuse the old listener"
        );
        assert_eq!(shutdowns.load(Ordering::SeqCst), 0);

        old.close().await.expect("old tunnel closes");
        let replacement = registry
            .acquire_with(SshTunnelKey([2; 32]), || {
                fake_tunnel_inner(port, Arc::clone(&shutdowns))
            })
            .await
            .expect("replacement uses the configured port after old leases close");
        assert_eq!(replacement.local_port(), port);
        replacement.close().await.expect("replacement closes");
        assert_eq!(shutdowns.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_open_leaves_no_registry_entry_or_listener() {
        let port = unused_loopback_port().await;
        let registry = SshTunnelRegistry::default();
        let attempts = Arc::new(AtomicUsize::new(0));
        let shutdowns = Arc::new(AtomicUsize::new(0));

        let result = registry
            .acquire_with(SshTunnelKey([9; 32]), || {
                let attempts = Arc::clone(&attempts);
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                        .await
                        .map_err(|_| AppError::internal())?;
                    drop(listener);
                    Err(AppError::unavailable(
                        "test_tunnel_open_failed",
                        "test tunnel failed",
                    ))
                }
            })
            .await;
        assert!(result.is_err());

        let lease = registry
            .acquire_with(SshTunnelKey([9; 32]), || {
                attempts.fetch_add(1, Ordering::SeqCst);
                fake_tunnel_inner(port, Arc::clone(&shutdowns))
            })
            .await
            .expect("a failed open does not poison the key or retain its listener");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        lease.close().await.expect("retry lease closes");
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
    }

    fn ssh_config(password: &str, local_port: Option<u16>) -> SshTunnelConfig {
        SshTunnelConfig {
            host_name: "ssh.internal".to_owned(),
            port: 22,
            user_name: "developer".to_owned(),
            authentication: SshAuthentication::Password {
                password: password.to_owned(),
            },
            host_key_verification: SshHostKeyVerification::KnownHosts,
            local_port,
        }
    }

    async fn unused_loopback_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("an ephemeral port is available");
        listener
            .local_addr()
            .expect("listener has an address")
            .port()
    }

    async fn fake_tunnel_inner(
        port: u16,
        shutdowns: Arc<AtomicUsize>,
    ) -> Result<SshTunnelInner, AppError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| {
                AppError::unavailable(
                    "ssh_tunnel_bind_failed",
                    "The SSH loopback tunnel port is unavailable",
                )
            })?;
        let local_port = listener
            .local_addr()
            .map_err(|_| AppError::internal())?
            .port();
        let (shutdown, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _listener = listener;
            let _ = receiver.await;
            shutdowns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        Ok(SshTunnelInner {
            local_port,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }
}
