use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::DatasourceConnection;

/// SSH user authentication material accepted only at a connection boundary.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SshAuthentication {
    /// Password authentication.
    Password {
        /// SSH password, never returned or logged.
        password: String,
    },
    /// OpenSSH-compatible private-key authentication.
    PrivateKey {
        /// User-selected local private-key path.
        key_file: String,
        /// Optional encrypted-key passphrase, never returned or logged.
        #[serde(skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
    },
}

/// Non-secret SSH authentication mode used by edit and export projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthenticationType {
    /// Password authentication. The password itself is never projected.
    Password,
    /// Private-key authentication. The key passphrase is never projected.
    PrivateKey,
}

impl Debug for SshAuthentication {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password { .. } => formatter.write_str("Password([REDACTED])"),
            Self::PrivateKey { key_file, .. } => formatter
                .debug_struct("PrivateKey")
                .field("key_file", key_file)
                .field("passphrase", &"[REDACTED]")
                .finish(),
        }
    }
}

/// SSH server host-key verification policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SshHostKeyVerification {
    /// Require a matching entry in the user's standard OpenSSH `known_hosts` file.
    #[default]
    KnownHosts,
}

/// Complete ephemeral SSH connection descriptor.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnelConfig {
    /// SSH server hostname or IP address.
    pub host_name: String,
    /// SSH server port.
    pub port: u16,
    /// SSH username.
    pub user_name: String,
    /// Password or private-key authentication.
    pub authentication: SshAuthentication,
    /// Server host-key verification policy.
    #[serde(default)]
    pub host_key_verification: SshHostKeyVerification,
    /// Preferred loopback listener port, or an OS-assigned port when absent/zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
}

/// Secret-free SSH settings returned to datasource edit surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnelEditProjection {
    /// SSH server hostname or IP address.
    pub host_name: String,
    /// SSH server port.
    pub port: u16,
    /// SSH username.
    pub user_name: String,
    /// Preferred loopback listener port, when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    /// Authentication mode without its password or passphrase.
    pub authentication_type: SshAuthenticationType,
    /// Selected local private-key path for private-key authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
    /// Host keys are always checked against OpenSSH `known_hosts`.
    pub host_key_verification: SshHostKeyVerification,
}

impl Debug for SshTunnelConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SshTunnelConfig")
            .field("host_name", &self.host_name)
            .field("port", &self.port)
            .field("user_name", &self.user_name)
            .field("authentication", &self.authentication)
            .field("host_key_verification", &self.host_key_verification)
            .field("local_port", &self.local_port)
            .finish()
    }
}

/// Unsaved datasource connection test with optional SSH local forwarding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshDatasourcePreConnectRequest {
    /// Native or compatibility driver identity.
    pub driver_id: String,
    /// Database connection descriptor whose target is forwarded through SSH.
    pub connection: DatasourceConnection,
    /// Optional SSH tunnel. Absence performs a direct database test.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnelConfig>,
}

/// Successful standalone SSH authentication result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionTestResult {
    /// True only after transport, key verification, and user authentication succeeded.
    pub verified: bool,
    /// Host-key policy used by the successful connection.
    pub host_key_verification: SshHostKeyVerification,
}

/// Successful database pre-connect result after optional SSH forwarding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshDatasourcePreConnectResult {
    /// True only after a real database open/ping/close cycle succeeded.
    pub verified: bool,
    /// Loopback port used for the ephemeral tunnel, absent for a direct connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::{SshAuthentication, SshHostKeyVerification, SshTunnelConfig};

    #[test]
    fn ssh_debug_output_redacts_passwords_and_passphrases() {
        for authentication in [
            SshAuthentication::Password {
                password: "sentinel-password".to_owned(),
            },
            SshAuthentication::PrivateKey {
                key_file: "/tmp/id_ed25519".to_owned(),
                passphrase: Some("sentinel-passphrase".to_owned()),
            },
        ] {
            let config = SshTunnelConfig {
                host_name: "ssh.example.test".to_owned(),
                port: 22,
                user_name: "developer".to_owned(),
                authentication,
                host_key_verification: SshHostKeyVerification::KnownHosts,
                local_port: None,
            };
            let debug = format!("{config:?}");
            assert!(!debug.contains("sentinel-password"));
            assert!(!debug.contains("sentinel-passphrase"));
        }
    }
}
