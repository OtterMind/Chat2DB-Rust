use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use chat2db_contract::JdbcDriver;
use chat2db_java_bridge::{
    BridgeError, DriverArtifact, DriverSpec, EngineSupervisor, MAX_DRIVER_ARTIFACT_BYTES,
    MAX_DRIVER_ARTIFACTS, MAX_DRIVER_TOTAL_BYTES,
};
use serde::Deserialize;
use thiserror::Error;

use crate::AppError;

pub(crate) const DEFAULT_DRIVER_PACK_DIRECTORY: &str = "driver-packs";
pub(crate) const DRIVER_RUNTIME_DIRECTORY: &str = "jdbc-driver-runtime";

const MANIFEST_FILE: &str = "driver-pack.json";
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MAX_DRIVER_PACKS: usize = 128;
const MAX_DRIVER_PACK_ROOT_ENTRIES: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_PACK_ID_BYTES: usize = 64;
const MAX_PACK_NAME_BYTES: usize = 128;
const MAX_PACK_VERSION_BYTES: usize = 128;
const MAX_DRIVER_CLASS_BYTES: usize = 512;
const MAX_RELATIVE_ARTIFACT_PATH_BYTES: usize = 1024;

#[derive(Debug)]
pub(crate) struct PreparedDriverPack {
    id: String,
    name: String,
    version: String,
    specification: DriverSpec,
    artifact_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct PreparedDriverPacks {
    packs: Vec<PreparedDriverPack>,
    staging: Option<tempfile::TempDir>,
}

#[derive(Debug, Error)]
pub(crate) enum DriverPackError {
    #[error("unable to {operation} driver-pack path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid driver pack at {path}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error("invalid driver-pack JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unable to verify a driver artifact declared by {manifest}: {source}")]
    Artifact {
        manifest: PathBuf,
        #[source]
        source: BridgeError,
    },
    #[error("driver pack {pack_id} could not be loaded: {source}")]
    Load {
        pack_id: String,
        #[source]
        source: BridgeError,
    },
}

impl DriverPackError {
    pub(crate) fn into_app_error(self) -> AppError {
        match self {
            Self::Load { source, .. } => source.into(),
            error => AppError::invalid("invalid_driver_pack", error.to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DriverPackManifest {
    schema_version: u32,
    id: String,
    name: String,
    version: String,
    driver_class: String,
    artifacts: Vec<ManifestArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestArtifact {
    path: String,
    sha256: String,
}

struct ArtifactPreparation<'a> {
    manifest_path: &'a Path,
    pack_directory: &'a Path,
    canonical_pack: &'a Path,
    staging_directory: &'a Path,
}

pub(crate) fn reset_runtime_directory(data_dir: &Path) -> Result<PathBuf, DriverPackError> {
    let runtime_directory = data_dir.join(DRIVER_RUNTIME_DIRECTORY);
    match fs::symlink_metadata(&runtime_directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(invalid(
                    &runtime_directory,
                    "the JDBC runtime directory cannot be a symbolic link",
                ));
            }
            if !metadata.is_dir() {
                return Err(invalid(
                    &runtime_directory,
                    "the JDBC runtime path must be a directory",
                ));
            }
            fs::remove_dir_all(&runtime_directory)
                .map_err(|source| io_error("clean", &runtime_directory, source))?;
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(io_error("inspect", &runtime_directory, source)),
    }
    fs::create_dir(&runtime_directory)
        .map_err(|source| io_error("create", &runtime_directory, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        fs::set_permissions(&runtime_directory, fs::Permissions::from_mode(0o700))
            .map_err(|source| io_error("secure", &runtime_directory, source))?;
    }
    fs::canonicalize(&runtime_directory)
        .map_err(|source| io_error("resolve", &runtime_directory, source))
}

pub(crate) fn discover(
    root: &Path,
    runtime_directory: &Path,
) -> Result<PreparedDriverPacks, DriverPackError> {
    let Some((canonical_root, pack_directories)) = scan_pack_directories(root)? else {
        return Ok(PreparedDriverPacks {
            packs: Vec::new(),
            staging: None,
        });
    };
    if pack_directories.is_empty() {
        return Ok(PreparedDriverPacks {
            packs: Vec::new(),
            staging: None,
        });
    }
    let staging = tempfile::Builder::new()
        .prefix("host-")
        .tempdir_in(runtime_directory)
        .map_err(|source| io_error("create staging directory in", runtime_directory, source))?;
    let mut pack_ids = HashSet::with_capacity(pack_directories.len());
    let mut driver_ids = HashMap::with_capacity(pack_directories.len());
    let mut total_bytes = 0_u64;
    let mut artifact_number = 0_usize;
    let mut packs = Vec::with_capacity(pack_directories.len());
    for pack_directory in pack_directories {
        let pack = prepare_pack(
            &canonical_root,
            &pack_directory,
            staging.path(),
            &mut pack_ids,
            &mut total_bytes,
            &mut artifact_number,
        )?;
        let driver_id = pack.specification.driver_id();
        if let Some(existing_pack_id) = driver_ids.insert(driver_id, pack.id.clone()) {
            return Err(invalid(
                &pack_directory.join(MANIFEST_FILE),
                format!(
                    "driver pack {:?} has the same driver identity as pack {existing_pack_id:?}",
                    pack.id
                ),
            ));
        }
        packs.push(pack);
    }
    Ok(PreparedDriverPacks {
        packs,
        staging: Some(staging),
    })
}

fn scan_pack_directories(root: &Path) -> Result<Option<(PathBuf, Vec<PathBuf>)>, DriverPackError> {
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io_error("inspect", root, source)),
    };
    if root_metadata.file_type().is_symlink() {
        return Err(invalid(
            root,
            "the driver-pack root cannot be a symbolic link",
        ));
    }
    if !root_metadata.is_dir() {
        return Err(invalid(root, "the driver-pack root must be a directory"));
    }
    let canonical_root =
        fs::canonicalize(root).map_err(|source| io_error("resolve", root, source))?;

    let mut pack_directories = Vec::new();
    let entries = fs::read_dir(root).map_err(|source| io_error("list", root, source))?;
    let mut entry_count = 0_usize;
    for entry in entries {
        let entry = entry.map_err(|source| io_error("list", root, source))?;
        entry_count += 1;
        if entry_count > MAX_DRIVER_PACK_ROOT_ENTRIES {
            return Err(invalid(
                root,
                format!(
                    "no more than {MAX_DRIVER_PACK_ROOT_ENTRIES} entries are allowed in the driver-pack root"
                ),
            ));
        }
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| io_error("inspect", &path, source))?;
        if file_type.is_symlink() {
            return Err(invalid(
                &path,
                "symbolic links are not accepted in the driver-pack root",
            ));
        }
        if file_type.is_dir() {
            pack_directories.push(path);
            if pack_directories.len() > MAX_DRIVER_PACKS {
                return Err(invalid(
                    root,
                    format!("no more than {MAX_DRIVER_PACKS} driver packs are allowed"),
                ));
            }
        } else if !file_type.is_file() {
            return Err(invalid(
                &path,
                "only regular files and pack directories are allowed in the driver-pack root",
            ));
        }
    }
    pack_directories.sort();
    Ok(Some((canonical_root, pack_directories)))
}

fn prepare_pack(
    canonical_root: &Path,
    pack_directory: &Path,
    staging_directory: &Path,
    pack_ids: &mut HashSet<String>,
    total_bytes: &mut u64,
    artifact_number: &mut usize,
) -> Result<PreparedDriverPack, DriverPackError> {
    let canonical_pack = fs::canonicalize(pack_directory)
        .map_err(|source| io_error("resolve", pack_directory, source))?;
    if canonical_pack.parent() != Some(canonical_root) {
        return Err(invalid(
            pack_directory,
            "the pack directory must resolve directly below the driver-pack root",
        ));
    }
    let manifest_path = pack_directory.join(MANIFEST_FILE);
    let manifest = read_manifest(&manifest_path)?;
    validate_manifest(&manifest_path, &manifest)?;
    if !pack_ids.insert(manifest.id.clone()) {
        return Err(invalid(
            &manifest_path,
            format!("duplicate driver-pack id {:?}", manifest.id),
        ));
    }

    let mut canonical_artifacts = HashSet::with_capacity(manifest.artifacts.len());
    let mut artifacts = Vec::with_capacity(manifest.artifacts.len());
    let mut pack_bytes = 0_u64;
    let preparation = ArtifactPreparation {
        manifest_path: &manifest_path,
        pack_directory,
        canonical_pack: &canonical_pack,
        staging_directory,
    };
    for declared in manifest.artifacts {
        let artifact = prepare_artifact(
            &preparation,
            &declared,
            &mut canonical_artifacts,
            *total_bytes,
            artifact_number,
        )?;
        pack_bytes = checked_artifact_total(&manifest_path, pack_bytes, artifact.byte_len())?;
        *total_bytes = checked_artifact_total(&manifest_path, *total_bytes, artifact.byte_len())?;
        artifacts.push(artifact);
    }

    Ok(PreparedDriverPack {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        specification: DriverSpec {
            driver_class: manifest.driver_class,
            artifacts,
        },
        artifact_bytes: pack_bytes,
    })
}

fn prepare_artifact(
    preparation: &ArtifactPreparation<'_>,
    declared: &ManifestArtifact,
    canonical_artifacts: &mut HashSet<PathBuf>,
    total_bytes: u64,
    artifact_number: &mut usize,
) -> Result<DriverArtifact, DriverPackError> {
    let manifest_path = preparation.manifest_path;
    let pack_directory = preparation.pack_directory;
    let canonical_pack = preparation.canonical_pack;
    let staging_directory = preparation.staging_directory;
    let relative_path = validate_artifact_path(manifest_path, &declared.path)?;
    let artifact_path = pack_directory.join(&relative_path);
    reject_symlink_components(manifest_path, pack_directory, &relative_path)?;

    let canonical_artifact = fs::canonicalize(&artifact_path)
        .map_err(|source| io_error("resolve", &artifact_path, source))?;
    if !canonical_artifact.starts_with(canonical_pack) {
        return Err(invalid(
            manifest_path,
            format!("artifact {:?} resolves outside its pack", declared.path),
        ));
    }
    if !canonical_artifacts.insert(canonical_artifact.clone()) {
        return Err(invalid(
            manifest_path,
            format!("artifact {:?} is declared more than once", declared.path),
        ));
    }

    let remaining_bytes = MAX_DRIVER_TOTAL_BYTES.saturating_sub(total_bytes);
    if remaining_bytes == 0 {
        return Err(invalid(
            manifest_path,
            format!("all driver packs together cannot exceed {MAX_DRIVER_TOTAL_BYTES} bytes"),
        ));
    }
    let artifact_limit = MAX_DRIVER_ARTIFACT_BYTES.min(remaining_bytes);
    let staged_path = staging_directory.join(format!("artifact-{artifact_number:04}.jar"));
    stage_regular_file(&canonical_artifact, &staged_path, artifact_limit)
        .map_err(|source| io_error("stage", &canonical_artifact, source))?;
    let artifact = DriverArtifact::from_path_with_max_bytes(&staged_path, artifact_limit).map_err(
        |source| DriverPackError::Artifact {
            manifest: manifest_path.to_path_buf(),
            source,
        },
    )?;
    let expected_digest = parse_sha256(manifest_path, &declared.sha256)?;
    if artifact.sha256() != &expected_digest {
        return Err(invalid(
            manifest_path,
            format!("artifact {:?} does not match its sha256", declared.path),
        ));
    }
    *artifact_number += 1;
    Ok(artifact)
}

fn checked_artifact_total(
    manifest_path: &Path,
    current: u64,
    artifact_bytes: u64,
) -> Result<u64, DriverPackError> {
    current
        .checked_add(artifact_bytes)
        .ok_or_else(|| invalid(manifest_path, "driver-pack byte count overflowed"))
}

fn stage_regular_file(source: &Path, target: &Path, maximum_bytes: u64) -> io::Result<()> {
    let mut input = open_regular_file_no_follow(source)?;
    if input.metadata()?.len() > maximum_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("driver artifact exceeds the byte limit of {maximum_bytes}"),
        ));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut copied = 0_u64;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let count_u64 = u64::try_from(count).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "driver artifact read size is not representable",
            )
        })?;
        if count_u64 > maximum_bytes.saturating_sub(copied) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("driver artifact exceeds the byte limit of {maximum_bytes}"),
            ));
        }
        output.write_all(&buffer[..count])?;
        copied += count_u64;
    }
    output.flush()
}

fn open_regular_file_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        let flags =
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits())
                .map_err(|_| io::Error::other("artifact open flags are not representable"))?;
        options.custom_flags(flags);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;

        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

pub(crate) async fn preload(
    supervisor: &EngineSupervisor,
    prepared: PreparedDriverPacks,
) -> Result<Vec<JdbcDriver>, DriverPackError> {
    let PreparedDriverPacks {
        packs,
        staging: _staging,
    } = prepared;
    if packs.is_empty() {
        return Ok(Vec::new());
    }
    let client = supervisor
        .client()
        .driver_client()
        .map_err(|source| DriverPackError::Load {
            pack_id: "[startup]".to_owned(),
            source,
        })?;
    let mut inventory = Vec::with_capacity(packs.len());
    for pack in packs {
        let loaded = client
            .load_driver(pack.specification)
            .await
            .map_err(|source| DriverPackError::Load {
                pack_id: pack.id.clone(),
                source,
            })?;
        inventory.push(JdbcDriver {
            pack_id: pack.id,
            name: pack.name,
            version: pack.version,
            driver_id: loaded.driver_id,
            driver_class: loaded.driver_class,
            artifact_count: loaded.artifact_count,
            artifact_bytes: pack.artifact_bytes.to_string(),
        });
    }
    Ok(inventory)
}

fn read_manifest(path: &Path) -> Result<DriverPackManifest, DriverPackError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspect", path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid(path, "the manifest cannot be a symbolic link"));
    }
    if !metadata.is_file() {
        return Err(invalid(path, "the manifest must be a regular file"));
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(invalid(
            path,
            format!("the manifest cannot exceed {MAX_MANIFEST_BYTES} bytes"),
        ));
    }

    let file =
        open_regular_file_no_follow(path).map_err(|source| io_error("open", path, source))?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error("read", path, source))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_MANIFEST_BYTES {
        return Err(invalid(
            path,
            format!("the manifest cannot exceed {MAX_MANIFEST_BYTES} bytes"),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|source| DriverPackError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_manifest(path: &Path, manifest: &DriverPackManifest) -> Result<(), DriverPackError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(invalid(
            path,
            format!(
                "schemaVersion must be {MANIFEST_SCHEMA_VERSION}, got {}",
                manifest.schema_version
            ),
        ));
    }
    validate_pack_id(path, &manifest.id)?;
    validate_text(path, &manifest.name, MAX_PACK_NAME_BYTES, "name")?;
    validate_text(path, &manifest.version, MAX_PACK_VERSION_BYTES, "version")?;
    validate_text(
        path,
        &manifest.driver_class,
        MAX_DRIVER_CLASS_BYTES,
        "driverClass",
    )?;
    if manifest.artifacts.is_empty() {
        return Err(invalid(path, "artifacts must contain at least one JAR"));
    }
    if manifest.artifacts.len() > MAX_DRIVER_ARTIFACTS {
        return Err(invalid(
            path,
            format!("artifacts cannot contain more than {MAX_DRIVER_ARTIFACTS} JARs"),
        ));
    }
    Ok(())
}

fn validate_pack_id(path: &Path, value: &str) -> Result<(), DriverPackError> {
    if value.is_empty() || value.len() > MAX_PACK_ID_BYTES {
        return Err(invalid(
            path,
            format!("id must contain between 1 and {MAX_PACK_ID_BYTES} ASCII bytes"),
        ));
    }
    let mut bytes = value.bytes();
    let first = bytes.next().expect("non-empty ids have a first byte");
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(invalid(
            path,
            "id must start with a lowercase ASCII letter or digit",
        ));
    }
    if !bytes.all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) {
        return Err(invalid(
            path,
            "id may contain only lowercase ASCII letters, digits, '.', '_', and '-'",
        ));
    }
    Ok(())
}

fn validate_text(
    path: &Path,
    value: &str,
    maximum_bytes: usize,
    field: &str,
) -> Result<(), DriverPackError> {
    if value.trim().is_empty() {
        return Err(invalid(path, format!("{field} is required")));
    }
    if value.len() > maximum_bytes {
        return Err(invalid(
            path,
            format!("{field} cannot exceed {maximum_bytes} UTF-8 bytes"),
        ));
    }
    Ok(())
}

fn validate_artifact_path(path: &Path, value: &str) -> Result<PathBuf, DriverPackError> {
    if value.is_empty() || value.len() > MAX_RELATIVE_ARTIFACT_PATH_BYTES {
        return Err(invalid(
            path,
            format!(
                "artifact path must contain between 1 and {MAX_RELATIVE_ARTIFACT_PATH_BYTES} UTF-8 bytes"
            ),
        ));
    }
    if value.contains('\\') || value.contains(':') {
        return Err(invalid(
            path,
            format!("artifact path {value:?} must use portable forward-slash components"),
        ));
    }
    if value
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(invalid(
            path,
            format!(
                "artifact path {value:?} must be normalized, relative, and cannot traverse directories"
            ),
        ));
    }
    let relative = PathBuf::from(value);
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(invalid(
            path,
            format!("artifact path {value:?} must be relative and cannot traverse directories"),
        ));
    }
    if !relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
    {
        return Err(invalid(
            path,
            format!("artifact path {value:?} must name a JAR"),
        ));
    }
    Ok(relative)
}

fn reject_symlink_components(
    manifest: &Path,
    pack_directory: &Path,
    relative_path: &Path,
) -> Result<(), DriverPackError> {
    let mut current = pack_directory.to_path_buf();
    let component_count = relative_path.components().count();
    for (index, component) in relative_path.components().enumerate() {
        let Component::Normal(component) = component else {
            return Err(invalid(manifest, "artifact path is not normalized"));
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|source| io_error("inspect", &current, source))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                manifest,
                format!(
                    "artifact path component {} cannot be a symbolic link",
                    current.display()
                ),
            ));
        }
        let is_artifact = index + 1 == component_count;
        if is_artifact && !metadata.is_file() {
            return Err(invalid(
                manifest,
                "the declared artifact must be a regular file",
            ));
        }
        if !is_artifact && !metadata.is_dir() {
            return Err(invalid(
                manifest,
                "intermediate artifact path components must be directories",
            ));
        }
    }
    Ok(())
}

fn parse_sha256(path: &Path, value: &str) -> Result<[u8; 32], DriverPackError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            path,
            "artifact sha256 must contain exactly 64 hexadecimal characters",
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| invalid(path, "artifact sha256 is invalid"))?;
    }
    Ok(digest)
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> DriverPackError {
    DriverPackError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn invalid(path: &Path, message: impl Into<String>) -> DriverPackError {
    DriverPackError::Invalid {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, fs};

    use chat2db_java_bridge::DriverArtifact;
    use serde_json::json;
    use tempfile::TempDir;

    use super::{
        DRIVER_RUNTIME_DIRECTORY, DriverPackError, MANIFEST_FILE, MAX_DRIVER_PACK_ROOT_ENTRIES,
        PreparedDriverPacks, discover, reset_runtime_directory, validate_artifact_path,
    };

    #[test]
    fn missing_root_has_an_empty_inventory() {
        let directory = TempDir::new().expect("temporary directory");
        let prepared =
            discover_test(&directory.path().join("missing")).expect("missing root is optional");
        assert!(prepared.packs.is_empty());
    }

    #[test]
    fn discovers_a_hash_verified_pack() {
        let directory = TempDir::new().expect("temporary directory");
        write_pack(directory.path(), "01-h2", "h2", "drivers/h2.jar");

        let prepared = discover_test(directory.path()).expect("valid pack must be discovered");
        let packs = prepared.packs;

        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "h2");
        assert_eq!(packs[0].name, "H2");
        assert_eq!(packs[0].version, "2.3.232");
        assert_eq!(packs[0].specification.driver_class, "org.h2.Driver");
        assert_eq!(packs[0].specification.artifacts.len(), 1);
        assert_eq!(packs[0].artifact_bytes, 8);
    }

    #[test]
    fn rejects_digest_mismatch_and_path_traversal() {
        let digest_directory = TempDir::new().expect("temporary directory");
        let manifest = write_pack(digest_directory.path(), "h2", "h2", "h2.jar");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).expect("manifest readable"))
                .expect("manifest JSON");
        value["artifacts"][0]["sha256"] = json!("00".repeat(32));
        fs::write(
            &manifest,
            serde_json::to_vec(&value).expect("manifest serializes"),
        )
        .expect("manifest rewrites");
        let error = discover_test(digest_directory.path()).expect_err("digest mismatch must fail");
        assert!(error.to_string().contains("does not match its sha256"));

        let traversal_directory = TempDir::new().expect("temporary directory");
        let manifest = write_pack(traversal_directory.path(), "h2", "h2", "h2.jar");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).expect("manifest readable"))
                .expect("manifest JSON");
        value["artifacts"][0]["path"] = json!("../outside.jar");
        fs::write(
            &manifest,
            serde_json::to_vec(&value).expect("manifest serializes"),
        )
        .expect("manifest rewrites");
        let error = discover_test(traversal_directory.path()).expect_err("traversal must fail");
        assert!(error.to_string().contains("cannot traverse"));
    }

    #[test]
    fn rejects_duplicate_pack_ids() {
        let directory = TempDir::new().expect("temporary directory");
        write_pack(directory.path(), "01-h2", "h2", "h2.jar");
        write_pack(directory.path(), "02-h2", "h2", "h2.jar");

        let error = discover_test(directory.path()).expect_err("duplicate ids must fail");
        assert!(error.to_string().contains("duplicate driver-pack id"));
    }

    #[test]
    fn rejects_duplicate_driver_identities_across_pack_aliases() {
        let directory = TempDir::new().expect("temporary directory");
        write_pack(directory.path(), "01-h2", "h2", "h2.jar");
        write_pack(directory.path(), "02-h2-alias", "h2-alias", "h2.jar");

        let error = discover_test(directory.path()).expect_err("duplicate identity must fail");
        assert!(error.to_string().contains("same driver identity"));
    }

    #[test]
    fn bounds_all_entries_in_the_driver_pack_root() {
        let directory = TempDir::new().expect("temporary directory");
        for index in 0..=MAX_DRIVER_PACK_ROOT_ENTRIES {
            fs::write(directory.path().join(format!("ignored-{index:04}")), [])
                .expect("ignored root file must be created");
        }

        let error = discover_test(directory.path()).expect_err("root entries must be bounded");
        assert!(error.to_string().contains("entries are allowed"));
    }

    #[test]
    fn rejects_non_normalized_artifact_paths() {
        let manifest = Path::new("driver-pack.json");

        for path in [
            "./h2.jar",
            "drivers/./h2.jar",
            "drivers//h2.jar",
            "drivers/../h2.jar",
            "h2.jar/",
        ] {
            let error = validate_artifact_path(manifest, path)
                .expect_err("non-normalized path must be rejected");
            assert!(error.to_string().contains("must be normalized"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_pack_and_artifact_directories() {
        use std::os::unix::fs::symlink;

        let pack_link_root = TempDir::new().expect("temporary directory");
        let external = TempDir::new().expect("external directory");
        symlink(external.path(), pack_link_root.path().join("linked-pack")).expect("pack symlink");
        let error = discover_test(pack_link_root.path()).expect_err("pack symlink must fail");
        assert!(error.to_string().contains("symbolic links"));

        let artifact_link_root = TempDir::new().expect("temporary directory");
        let manifest = write_pack(artifact_link_root.path(), "h2", "h2", "drivers/h2.jar");
        let pack = manifest.parent().expect("manifest has parent");
        fs::remove_dir_all(pack.join("drivers")).expect("remove real driver directory");
        symlink(external.path(), pack.join("drivers")).expect("artifact directory symlink");
        let error =
            discover_test(artifact_link_root.path()).expect_err("artifact symlink must fail");
        assert!(error.to_string().contains("symbolic link"));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_cleanup_rejects_symlinks_without_touching_their_target() {
        use std::os::unix::fs::symlink;

        let data_directory = TempDir::new().expect("temporary data directory");
        let external = TempDir::new().expect("external directory");
        let marker = external.path().join("must-survive");
        fs::write(&marker, b"owned elsewhere").expect("external marker");
        symlink(
            external.path(),
            data_directory.path().join(DRIVER_RUNTIME_DIRECTORY),
        )
        .expect("runtime symlink");

        let error = reset_runtime_directory(data_directory.path())
            .expect_err("runtime symlink must fail closed");
        assert!(error.to_string().contains("cannot be a symbolic link"));
        assert!(
            marker.is_file(),
            "runtime cleanup must not follow the symlink"
        );
    }

    fn discover_test(root: &Path) -> Result<PreparedDriverPacks, DriverPackError> {
        discover(root, root)
    }

    fn write_pack(root: &Path, directory: &str, id: &str, artifact_path: &str) -> PathBuf {
        let pack = root.join(directory);
        let artifact = pack.join(artifact_path);
        fs::create_dir_all(artifact.parent().expect("artifact has parent"))
            .expect("artifact directory");
        fs::write(&artifact, b"fake-jar").expect("artifact writes");
        let digest = DriverArtifact::from_path(&artifact).expect("artifact hashes");
        let mut sha256 = String::with_capacity(64);
        for byte in digest.sha256() {
            write!(&mut sha256, "{byte:02x}").expect("writing to String cannot fail");
        }
        let manifest = pack.join(MANIFEST_FILE);
        fs::write(
            &manifest,
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 1,
                "id": id,
                "name": "H2",
                "version": "2.3.232",
                "driverClass": "org.h2.Driver",
                "artifacts": [{
                    "path": artifact_path,
                    "sha256": sha256
                }]
            }))
            .expect("manifest serializes"),
        )
        .expect("manifest writes");
        manifest
    }

    use std::path::{Path, PathBuf};
}
