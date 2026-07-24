use std::{
    fmt::{Debug, Formatter, Write as _},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use rand::{TryRngCore, rngs::OsRng};
use zeroize::Zeroizing;

use crate::{
    SecretRef, SecretValue, SecretVault, SecretVaultError, StorageError, secure_directory,
    secure_file, sync_directory,
};

const SECRETS_DIRECTORY: &str = "secrets";
const SECRET_FILE_EXTENSION: &str = "c2secret";
const VAULT_MAGIC: &[u8; 8] = b"C2DBSECR";
const VAULT_FORMAT_VERSION: u8 = 1;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const HEADER_BYTES: usize = VAULT_MAGIC.len() + 1 + NONCE_BYTES;
const MAX_SECRET_BYTES: usize = 16 * 1024 * 1024;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const KEYRING_SERVICE: &str = "ai.chat2db.Chat2DB.storage";
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const KEYRING_ACCOUNT_PREFIX: &str = "master-key-v1:";
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const KEY_INITIALIZATION_LOCK: &str = ".chat2db-vault-key.lock";

/// AES-256-GCM vault whose ciphertext records live beside the product store.
///
/// The master key is never written to the data directory. Each immutable
/// record is bound to its [`SecretRef`] as authenticated additional data.
pub struct EncryptedFileVault {
    secrets_dir: PathBuf,
    master_key: Zeroizing<[u8; 32]>,
}

impl Debug for EncryptedFileVault {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EncryptedFileVault")
            .field("secrets_dir", &self.secrets_dir)
            .field("master_key", &"[REDACTED]")
            .finish()
    }
}

impl EncryptedFileVault {
    /// Opens an encrypted vault under `data_dir` using an exact 256-bit key.
    ///
    /// # Errors
    ///
    /// Returns a safe vault classification when the private directories cannot
    /// be created or secured.
    pub fn new(data_dir: impl AsRef<Path>, master_key: [u8; 32]) -> Result<Self, SecretVaultError> {
        Self::from_zeroizing_key(data_dir, Zeroizing::new(master_key))
    }

    /// Opens a headless vault from a standard-base64 encoded 256-bit key.
    ///
    /// This is the production entrypoint for containers that intentionally do
    /// not depend on a desktop Secret Service.
    ///
    /// # Errors
    ///
    /// Returns [`SecretVaultError::InvalidConfiguration`] unless the input is
    /// valid base64 containing exactly 32 decoded bytes.
    pub fn from_base64_master_key(
        data_dir: impl AsRef<Path>,
        encoded_master_key: &str,
    ) -> Result<Self, SecretVaultError> {
        let decoded = Zeroizing::new(
            BASE64_STANDARD
                .decode(encoded_master_key)
                .map_err(|_| SecretVaultError::InvalidConfiguration)?,
        );
        if decoded.len() != 32 {
            return Err(SecretVaultError::InvalidConfiguration);
        }
        let mut master_key = Zeroizing::new([0_u8; 32]);
        master_key.copy_from_slice(decoded.as_slice());
        Self::from_zeroizing_key(data_dir, master_key)
    }

    fn from_zeroizing_key(
        data_dir: impl AsRef<Path>,
        master_key: Zeroizing<[u8; 32]>,
    ) -> Result<Self, SecretVaultError> {
        let data_dir = prepare_data_dir(data_dir.as_ref())?;
        let secrets_dir = prepare_secrets_dir(&data_dir)?;
        Ok(Self {
            secrets_dir,
            master_key,
        })
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn from_prepared_directory(secrets_dir: PathBuf, master_key: Zeroizing<[u8; 32]>) -> Self {
        Self {
            secrets_dir,
            master_key,
        }
    }

    fn secret_path(&self, reference: &SecretRef) -> Result<PathBuf, SecretVaultError> {
        let uuid = reference
            .validated_uuid()
            .ok_or(SecretVaultError::Backend)?;
        Ok(self
            .secrets_dir
            .join(format!("{uuid}.{SECRET_FILE_EXTENSION}")))
    }

    fn cipher(&self) -> Result<Aes256Gcm, SecretVaultError> {
        Aes256Gcm::new_from_slice(self.master_key.as_ref()).map_err(|_| SecretVaultError::Backend)
    }

    fn matches_master_key(&self, candidate: &[u8]) -> bool {
        if candidate.len() != self.master_key.len() {
            return false;
        }
        self.master_key
            .iter()
            .zip(candidate)
            .fold(0_u8, |difference, (expected, actual)| {
                difference | (expected ^ actual)
            })
            == 0
    }

    fn aad(reference: &SecretRef) -> Vec<u8> {
        let mut aad = Vec::with_capacity(VAULT_MAGIC.len() + 1 + reference.as_str().len());
        aad.extend_from_slice(VAULT_MAGIC);
        aad.push(VAULT_FORMAT_VERSION);
        aad.extend_from_slice(reference.as_str().as_bytes());
        aad
    }

    fn encrypted_record(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
    ) -> Result<Vec<u8>, SecretVaultError> {
        if value.expose_secret().len() > MAX_SECRET_BYTES {
            return Err(SecretVaultError::Backend);
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        fill_random(&mut nonce)?;
        let ciphertext = self
            .cipher()?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: value.expose_secret(),
                    aad: &Self::aad(reference),
                },
            )
            .map_err(|_| SecretVaultError::Backend)?;
        let mut record = Vec::with_capacity(HEADER_BYTES + ciphertext.len());
        record.extend_from_slice(VAULT_MAGIC);
        record.push(VAULT_FORMAT_VERSION);
        record.extend_from_slice(&nonce);
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }

    fn decrypt_record(
        &self,
        reference: &SecretRef,
        record: &[u8],
    ) -> Result<SecretValue, SecretVaultError> {
        if record.len() < HEADER_BYTES + TAG_BYTES
            || record.len() > HEADER_BYTES + TAG_BYTES + MAX_SECRET_BYTES
            || &record[..VAULT_MAGIC.len()] != VAULT_MAGIC
            || record[VAULT_MAGIC.len()] != VAULT_FORMAT_VERSION
        {
            return Err(SecretVaultError::Backend);
        }
        let nonce_start = VAULT_MAGIC.len() + 1;
        let ciphertext_start = nonce_start + NONCE_BYTES;
        let plaintext = self
            .cipher()?
            .decrypt(
                Nonce::from_slice(&record[nonce_start..ciphertext_start]),
                Payload {
                    msg: &record[ciphertext_start..],
                    aad: &Self::aad(reference),
                },
            )
            .map_err(|_| SecretVaultError::Backend)?;
        Ok(SecretValue::new(plaintext))
    }
}

impl SecretVault for EncryptedFileVault {
    fn probe(&self) -> Result<(), SecretVaultError> {
        let reference = SecretRef::generate();
        let mut probe_bytes = Zeroizing::new([0_u8; 32]);
        fill_random(probe_bytes.as_mut())?;
        let expected = SecretValue::new(probe_bytes.to_vec());
        self.create(&reference, &expected)?;

        let verification = self.get(&reference).and_then(|actual| {
            actual
                .filter(|value| value.expose_secret() == expected.expose_secret())
                .ok_or(SecretVaultError::Backend)
        });
        let deletion = self.delete(&reference);
        verification?;
        deletion?;
        if self.get(&reference)?.is_some() {
            return Err(SecretVaultError::Backend);
        }
        Ok(())
    }

    fn create(&self, reference: &SecretRef, value: &SecretValue) -> Result<(), SecretVaultError> {
        let path = self.secret_path(reference)?;
        let record = self.encrypted_record(reference, value)?;
        let mut file = create_private_file(&path)?;
        if let Err(error) = secure_file(&path).map_err(map_storage_error) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        let write_result = file
            .write_all(&record)
            .and_then(|()| file.sync_all())
            .map_err(|error| map_io_error(&error));
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&path);
            let _ = sync_directory(&self.secrets_dir);
            return Err(error);
        }
        sync_directory(&self.secrets_dir).map_err(map_storage_error)
    }

    fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
        let path = self.secret_path(reference)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_io_error(&error)),
        };
        if !metadata.file_type().is_file() || !private_file_permissions(&metadata) {
            return Err(SecretVaultError::AccessDenied);
        }
        let maximum_length = HEADER_BYTES + TAG_BYTES + MAX_SECRET_BYTES;
        if metadata.len() > maximum_length as u64 {
            return Err(SecretVaultError::Backend);
        }
        let file = File::open(&path).map_err(|error| map_io_error(&error))?;
        let mut record = Vec::with_capacity(
            usize::try_from(metadata.len()).map_err(|_| SecretVaultError::Backend)?,
        );
        file.take((maximum_length + 1) as u64)
            .read_to_end(&mut record)
            .map_err(|error| map_io_error(&error))?;
        if record.len() > maximum_length {
            return Err(SecretVaultError::Backend);
        }
        self.decrypt_record(reference, &record).map(Some)
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
        let path = self.secret_path(reference)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => return Err(SecretVaultError::AccessDenied),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(map_io_error(&error)),
        }
        fs::remove_file(&path).map_err(|error| map_io_error(&error))?;
        sync_directory(&self.secrets_dir).map_err(map_storage_error)
    }
}

/// OS-keyring-rooted encrypted file vault for interactive desktop hosts.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub struct OsSecretVault {
    inner: EncryptedFileVault,
    keyring_account: String,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl Debug for OsSecretVault {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OsSecretVault")
            .field("inner", &self.inner)
            .field("keyring_account", &"[DERIVED]")
            .finish()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl OsSecretVault {
    /// Opens the OS-backed vault, generating its keyring master key on first use.
    ///
    /// The keyring account is derived from a SHA-256 digest of the canonical
    /// data-directory path, so independent profiles never share a master key.
    ///
    /// # Errors
    ///
    /// Returns a safe classification when the OS credential store or private
    /// encrypted-file boundary cannot be initialized.
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self, SecretVaultError> {
        let data_dir = prepare_data_dir(data_dir.as_ref())?;
        let secrets_dir = prepare_secrets_dir(&data_dir)?;
        let keyring_account = keyring_account(&data_dir);
        let _initialization_lock = lock_key_initialization(&data_dir)?;
        let entry = keyring_entry(&keyring_account)?;
        let master_key = if let Some(master_key) = read_keyring_master_key(&entry)? {
            master_key
        } else {
            if contains_encrypted_records(&secrets_dir)? {
                return Err(SecretVaultError::Unavailable);
            }
            create_keyring_master_key(&entry)?
        };
        Ok(Self {
            inner: EncryptedFileVault::from_prepared_directory(secrets_dir, master_key),
            keyring_account,
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl SecretVault for OsSecretVault {
    fn probe(&self) -> Result<(), SecretVaultError> {
        let entry = keyring_entry(&self.keyring_account)?;
        let stored_key = read_keyring_master_key(&entry)?.ok_or(SecretVaultError::Unavailable)?;
        if !self.inner.matches_master_key(stored_key.as_ref()) {
            return Err(SecretVaultError::Backend);
        }
        self.inner.probe()
    }

    fn create(&self, reference: &SecretRef, value: &SecretValue) -> Result<(), SecretVaultError> {
        self.inner.create(reference, value)
    }

    fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
        self.inner.get(reference)
    }

    fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
        self.inner.delete(reference)
    }
}

fn prepare_data_dir(data_dir: &Path) -> Result<PathBuf, SecretVaultError> {
    fs::create_dir_all(data_dir).map_err(|error| map_io_error(&error))?;
    secure_directory(data_dir).map_err(map_storage_error)?;
    fs::canonicalize(data_dir).map_err(|error| map_io_error(&error))
}

fn prepare_secrets_dir(data_dir: &Path) -> Result<PathBuf, SecretVaultError> {
    let secrets_dir = data_dir.join(SECRETS_DIRECTORY);
    fs::create_dir_all(&secrets_dir).map_err(|error| map_io_error(&error))?;
    secure_directory(&secrets_dir).map_err(map_storage_error)?;
    sync_directory(data_dir).map_err(map_storage_error)?;
    Ok(secrets_dir)
}

fn create_private_file(path: &Path) -> Result<File, SecretVaultError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|error| map_io_error(&error))
}

#[cfg(unix)]
fn private_file_permissions(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode().trailing_zeros() >= 6
}

#[cfg(not(unix))]
fn private_file_permissions(_metadata: &fs::Metadata) -> bool {
    true
}

fn fill_random(destination: &mut [u8]) -> Result<(), SecretVaultError> {
    OsRng
        .try_fill_bytes(destination)
        .map_err(|_| SecretVaultError::Unavailable)
}

fn map_io_error(error: &std::io::Error) -> SecretVaultError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        SecretVaultError::AccessDenied
    } else {
        SecretVaultError::Backend
    }
}

fn map_storage_error(error: StorageError) -> SecretVaultError {
    match error {
        StorageError::Io { source, .. } => map_io_error(&source),
        _ => SecretVaultError::Backend,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn keyring_account(data_dir: &Path) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(data_dir.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in data_dir.as_os_str().encode_wide() {
            hasher.update(unit.to_le_bytes());
        }
    }
    let digest = hasher.finalize();
    let mut account = String::with_capacity(KEYRING_ACCOUNT_PREFIX.len() + digest.len() * 2);
    account.push_str(KEYRING_ACCOUNT_PREFIX);
    for byte in digest {
        write!(&mut account, "{byte:02x}").expect("writing to a String cannot fail");
    }
    account
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn lock_key_initialization(data_dir: &Path) -> Result<File, SecretVaultError> {
    let path = data_dir.join(KEY_INITIALIZATION_LOCK);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(|error| map_io_error(&error))?;
    secure_file(&path).map_err(map_storage_error)?;
    fs2::FileExt::lock_exclusive(&file).map_err(|error| map_io_error(&error))?;
    sync_directory(data_dir).map_err(map_storage_error)?;
    Ok(file)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn keyring_entry(account: &str) -> Result<keyring::Entry, SecretVaultError> {
    keyring::Entry::new(KEYRING_SERVICE, account).map_err(|error| map_keyring_error(&error))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn read_keyring_master_key(
    entry: &keyring::Entry,
) -> Result<Option<Zeroizing<[u8; 32]>>, SecretVaultError> {
    let bytes = match entry.get_secret() {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(error) => return Err(map_keyring_error(&error)),
    };
    if bytes.len() != 32 {
        return Err(SecretVaultError::Backend);
    }
    let mut master_key = Zeroizing::new([0_u8; 32]);
    master_key.copy_from_slice(bytes.as_slice());
    Ok(Some(master_key))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn create_keyring_master_key(
    entry: &keyring::Entry,
) -> Result<Zeroizing<[u8; 32]>, SecretVaultError> {
    let mut generated = Zeroizing::new([0_u8; 32]);
    fill_random(generated.as_mut())?;
    entry
        .set_secret(generated.as_ref())
        .map_err(|error| map_keyring_error(&error))?;
    let persisted = read_keyring_master_key(entry)?.ok_or(SecretVaultError::Backend)?;
    if persisted.as_ref() != generated.as_ref() {
        return Err(SecretVaultError::Backend);
    }
    Ok(persisted)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn contains_encrypted_records(secrets_dir: &Path) -> Result<bool, SecretVaultError> {
    for entry in fs::read_dir(secrets_dir).map_err(|error| map_io_error(&error))? {
        let entry = entry.map_err(|error| map_io_error(&error))?;
        if entry.path().extension().and_then(|value| value.to_str()) == Some(SECRET_FILE_EXTENSION)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn map_keyring_error(error: &keyring::Error) -> SecretVaultError {
    match error {
        keyring::Error::NoStorageAccess(_) => SecretVaultError::AccessDenied,
        keyring::Error::NoEntry | keyring::Error::PlatformFailure(_) => {
            SecretVaultError::Unavailable
        }
        _ => SecretVaultError::Backend,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
    use tempfile::TempDir;

    use super::{EncryptedFileVault, SecretRef, SecretValue, SecretVault, SecretVaultError};

    fn reference() -> SecretRef {
        SecretRef::generate()
    }

    #[test]
    fn encrypted_file_round_trip_never_persists_plaintext() {
        let directory = TempDir::new().expect("temp dir");
        let vault = EncryptedFileVault::new(directory.path(), [7_u8; 32]).expect("vault opens");
        let reference = reference();
        let sentinel = b"jdbc:sentinel-password-never-on-disk";
        vault
            .create(&reference, &SecretValue::new(sentinel.to_vec()))
            .expect("secret creates");

        let path = vault.secret_path(&reference).expect("valid secret path");
        let record = fs::read(path).expect("record reads");
        assert!(
            !record
                .windows(sentinel.len())
                .any(|bytes| bytes == sentinel)
        );
        assert_eq!(
            vault
                .get(&reference)
                .expect("secret reads")
                .expect("secret exists")
                .expose_secret(),
            sentinel
        );
        assert!(!format!("{vault:?}").contains("jdbc:sentinel"));
    }

    #[test]
    fn create_is_immutable_and_delete_is_idempotent() {
        let directory = TempDir::new().expect("temp dir");
        let vault = EncryptedFileVault::new(directory.path(), [8_u8; 32]).expect("vault opens");
        let reference = reference();
        vault
            .create(&reference, &SecretValue::new(b"first".to_vec()))
            .expect("first create succeeds");
        assert_eq!(
            vault
                .create(&reference, &SecretValue::new(b"replacement".to_vec()))
                .expect_err("duplicate create must fail"),
            SecretVaultError::Backend
        );
        assert_eq!(
            vault
                .get(&reference)
                .expect("secret reads")
                .expect("secret exists")
                .expose_secret(),
            b"first"
        );

        vault.delete(&reference).expect("first delete succeeds");
        vault.delete(&reference).expect("second delete succeeds");
        assert!(vault.get(&reference).expect("missing reads").is_none());
    }

    #[test]
    fn corruption_wrong_key_and_wrong_aad_fail_closed() {
        let directory = TempDir::new().expect("temp dir");
        let vault = EncryptedFileVault::new(directory.path(), [9_u8; 32]).expect("vault opens");
        let original = reference();
        vault
            .create(&original, &SecretValue::new(b"secret".to_vec()))
            .expect("secret creates");

        let wrong_key =
            EncryptedFileVault::new(directory.path(), [10_u8; 32]).expect("second vault opens");
        assert_eq!(
            wrong_key
                .get(&original)
                .expect_err("wrong key must not decrypt"),
            SecretVaultError::Backend
        );

        let original_path = vault.secret_path(&original).expect("original path");
        let rebound = reference();
        let rebound_path = vault.secret_path(&rebound).expect("rebound path");
        fs::rename(&original_path, &rebound_path).expect("record renames");
        assert_eq!(
            vault
                .get(&rebound)
                .expect_err("reference AAD must reject a renamed record"),
            SecretVaultError::Backend
        );

        let mut bytes = fs::read(&rebound_path).expect("record reads");
        let last = bytes.last_mut().expect("ciphertext has tag");
        *last ^= 1;
        fs::write(&rebound_path, bytes).expect("record corrupts");
        assert_eq!(
            vault
                .get(&rebound)
                .expect_err("corruption must fail authentication"),
            SecretVaultError::Backend
        );
    }

    #[test]
    fn base64_helper_requires_exactly_32_bytes_and_probe_exercises_boundary() {
        let directory = TempDir::new().expect("temp dir");
        let encoded = BASE64_STANDARD.encode([11_u8; 32]);
        let vault = EncryptedFileVault::from_base64_master_key(directory.path(), &encoded)
            .expect("headless vault opens");
        vault.probe().expect("probe round trip succeeds");

        for invalid in [
            "not base64".to_owned(),
            BASE64_STANDARD.encode([0_u8; 31]),
            BASE64_STANDARD.encode([0_u8; 33]),
        ] {
            assert_eq!(
                EncryptedFileVault::from_base64_master_key(directory.path(), &invalid)
                    .expect_err("invalid key must fail"),
                SecretVaultError::InvalidConfiguration
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn secrets_directory_and_records_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new().expect("temp dir");
        let vault = EncryptedFileVault::new(directory.path(), [12_u8; 32]).expect("vault opens");
        let reference = reference();
        vault
            .create(&reference, &SecretValue::new(b"secret".to_vec()))
            .expect("secret creates");

        let directory_mode = fs::metadata(&vault.secrets_dir)
            .expect("secrets metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(vault.secret_path(&reference).expect("secret path"))
            .expect("secret metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
