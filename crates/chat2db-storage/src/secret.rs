use std::fmt::{Debug, Formatter};

use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroize;

/// Opaque reference persisted by `SQLite` in place of secret material.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    pub(crate) fn generate() -> Self {
        Self(format!("chat2db:datasource:{}", Uuid::new_v4()))
    }

    pub(crate) fn from_persisted(value: String) -> Result<Self, crate::StorageError> {
        let Some(uuid) = value.strip_prefix("chat2db:datasource:") else {
            return Err(crate::StorageError::InvalidDatasource(
                "persisted secret reference is invalid",
            ));
        };
        Uuid::parse_str(uuid).map_err(|_| {
            crate::StorageError::InvalidDatasource("persisted secret reference is invalid")
        })?;
        Ok(Self(value))
    }

    /// Returns the non-secret vault lookup key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for SecretRef {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("SecretRef").field(&self.0).finish()
    }
}

/// Secret bytes that are zeroed when their owner is dropped.
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    /// Takes ownership of secret bytes.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Exposes the bytes only at the vault or session-opening boundary.
    #[must_use]
    pub fn expose_secret(&self) -> &[u8] {
        &self.0
    }
}

impl Debug for SecretValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Safe, non-secret error classifications returned by a credential vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SecretVaultError {
    /// The platform credential store is not available.
    #[error("vault unavailable")]
    Unavailable,
    /// The operating system denied credential access.
    #[error("vault access denied")]
    AccessDenied,
    /// The vault rejected or could not persist the value.
    #[error("vault backend failure")]
    Backend,
}

/// External credential-store boundary. Implementations must never log values.
pub trait SecretVault: Send + Sync {
    /// Verifies that this vault can serve the storage instance.
    ///
    /// # Errors
    ///
    /// Returns a safe vault classification when credentials cannot currently
    /// be read, created, and deleted by this process.
    fn probe(&self) -> Result<(), SecretVaultError>;

    /// Creates one immutable secret under a fresh opaque reference.
    ///
    /// # Errors
    ///
    /// Returns a safe vault classification when the value was not durably
    /// created or its outcome is unknown.
    fn create(&self, reference: &SecretRef, value: &SecretValue) -> Result<(), SecretVaultError>;

    /// Loads one secret into zeroizing memory.
    ///
    /// # Errors
    ///
    /// Returns a safe vault classification when access fails.
    fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError>;

    /// Idempotently removes one secret.
    ///
    /// # Errors
    ///
    /// Returns a safe vault classification when deletion cannot be confirmed.
    fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError>;
}

#[cfg(test)]
mod tests {
    use super::SecretValue;

    #[test]
    fn debug_output_never_exposes_secret_bytes() {
        let value = SecretValue::new(b"sentinel-password".to_vec());
        let debug = format!("{value:?}");

        assert_eq!(debug, "SecretValue([REDACTED])");
        assert!(!debug.contains("sentinel-password"));
    }
}
