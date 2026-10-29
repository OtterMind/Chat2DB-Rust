//! On-demand installation of the Java compatibility runtime.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

const INSTALL_PROGRESS_EVENT: &str = "chat2db://java-runtime-install-progress";
const RELEASE_BASE_ENV: &str = "CHAT2DB_JAVA_RUNTIME_RELEASE_BASE";
const DEFAULT_RELEASE_BASE: &str = "https://github.com/OtterMind/Chat2DB-Rust/releases/download";
const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RUNTIME_DIRECTORIES: [&str; 4] = ["java", "engine", "community-classpath", "driver-packs"];

/// Installation state of the optional Java compatibility runtime.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntimeStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallProgress {
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

fn platform_id() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else if cfg!(target_os = "macos") {
        "macos-x64"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x86_64"
    }
}

fn release_base() -> String {
    std::env::var(RELEASE_BASE_ENV).unwrap_or_else(|_| DEFAULT_RELEASE_BASE.to_owned())
}

fn runtime_root<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join("java-runtime"))
        .map_err(|error| format!("application data directory is unavailable: {error}"))
}

fn installed_runtime_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(runtime_root(app)?.join(PRODUCT_VERSION))
}

fn java_binary_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

fn runtime_java_binary<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(installed_runtime_dir(app)?
        .join("java")
        .join("bin")
        .join(java_binary_name()))
}

fn runtime_engine_jar<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(installed_runtime_dir(app)?
        .join("engine")
        .join("chat2db-compat-runtime.jar"))
}

fn status_from_disk<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<JavaRuntimeStatus, String> {
    let installed = runtime_java_binary(app)?.is_file() && runtime_engine_jar(app)?.is_file();
    Ok(JavaRuntimeStatus {
        installed,
        version: installed.then(|| PRODUCT_VERSION.to_owned()),
        path: installed
            .then(|| installed_runtime_dir(app).map(|path| path.display().to_string()))
            .transpose()?,
    })
}

/// Reports whether the Java compatibility runtime is installed.
///
/// # Errors
///
/// Returns an error when the application data directory cannot be resolved.
#[tauri::command]
pub async fn java_runtime_status<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<JavaRuntimeStatus, String> {
    status_from_disk(&app)
}

fn emit_progress<R: tauri::Runtime>(
    app: &AppHandle<R>,
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    let _ = app.emit(
        INSTALL_PROGRESS_EVENT,
        InstallProgress {
            phase,
            downloaded_bytes,
            total_bytes,
        },
    );
}

async fn download_archive<R: tauri::Runtime>(
    app: &AppHandle<R>,
    url: &str,
    destination: &Path,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| format!("download client failed: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("download failed with status {}", response.status()));
    }
    let total_bytes = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(destination)
        .await
        .map_err(|error| format!("temporary file failed: {error}"))?;
    let mut downloaded_bytes = 0_u64;
    let mut last_reported = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("download interrupted: {error}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("temporary file write failed: {error}"))?;
        downloaded_bytes += chunk.len() as u64;
        if downloaded_bytes - last_reported >= 1024 * 1024 {
            last_reported = downloaded_bytes;
            emit_progress(app, "downloading", downloaded_bytes, total_bytes);
        }
    }
    file.flush()
        .await
        .map_err(|error| format!("temporary file flush failed: {error}"))?;
    emit_progress(app, "downloading", downloaded_bytes, total_bytes);
    Ok(())
}

async fn download_checksum(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| format!("download client failed: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("checksum download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "checksum download failed with status {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|error| format!("checksum read failed: {error}"))?;
    body.split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .filter(|value| value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| "checksum file is invalid".to_owned())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| format!("archive open failed: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("archive read failed: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn entry_path(destination: &Path, name: &str) -> Result<PathBuf, String> {
    let relative = Path::new(name);
    if relative.is_absolute() || name.split('/').any(|component| component == "..") {
        return Err(format!(
            "archive entry escapes the runtime directory: {name}"
        ));
    }
    let first = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str());
    if !first.is_some_and(|component| RUNTIME_DIRECTORIES.contains(&component)) {
        return Err(format!(
            "archive entry is outside the runtime layout: {name}"
        ));
    }
    Ok(destination.join(relative))
}

fn extract_runtime(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file =
        fs::File::open(archive_path).map_err(|error| format!("archive open failed: {error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("archive is invalid: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("archive entry failed: {error}"))?;
        let name = entry.name().to_owned();
        let output = entry_path(destination, &name)?;
        if name.ends_with('/') {
            fs::create_dir_all(&output)
                .map_err(|error| format!("runtime directory creation failed: {error}"))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("runtime directory creation failed: {error}"))?;
        }
        let mode = entry.unix_mode();
        let is_symlink = mode.is_some_and(|mode| mode & 0o170_000 == 0o120_000);
        if is_symlink {
            #[cfg(unix)]
            {
                let mut target = String::new();
                entry
                    .read_to_string(&mut target)
                    .map_err(|error| format!("runtime link read failed: {error}"))?;
                std::os::unix::fs::symlink(target, &output)
                    .map_err(|error| format!("runtime link creation failed: {error}"))?;
                continue;
            }
            #[cfg(not(unix))]
            return Err(format!(
                "runtime archive contains an unsupported link: {name}"
            ));
        }
        let mut output_file =
            fs::File::create(&output).map_err(|error| format!("runtime write failed: {error}"))?;
        std::io::copy(&mut entry, &mut output_file)
            .map_err(|error| format!("runtime extraction failed: {error}"))?;
        output_file
            .flush()
            .map_err(|error| format!("runtime extraction failed: {error}"))?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|error| format!("runtime permission update failed: {error}"))?;
        }
    }
    Ok(())
}

/// Downloads, verifies and installs the Java compatibility runtime.
///
/// # Errors
///
/// Returns an error when the download, checksum verification, extraction or
/// installation fails.
#[tauri::command]
pub async fn java_runtime_install<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<JavaRuntimeStatus, String> {
    let root = runtime_root(&app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("runtime directory creation failed: {error}"))?;
    let archive_name = format!("chat2db-java-runtime-{}.zip", platform_id());
    let base = format!("{}/v{}", release_base(), PRODUCT_VERSION);
    let archive_url = format!("{base}/{archive_name}");
    let checksum_url = format!("{archive_url}.sha256");

    let archive_path = root.join(".java-runtime-download.zip");
    emit_progress(&app, "downloading", 0, None);
    download_archive(&app, &archive_url, &archive_path).await?;

    emit_progress(&app, "verifying", 0, None);
    let expected = download_checksum(&checksum_url).await?;
    let actual = file_sha256(&archive_path)?;
    if actual != expected {
        let _ = fs::remove_file(&archive_path);
        return Err("download checksum mismatch".to_owned());
    }

    emit_progress(&app, "installing", 0, None);
    let target = installed_runtime_dir(&app)?;
    let staging = root.join(format!(".{PRODUCT_VERSION}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| format!("staging cleanup failed: {error}"))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("staging directory creation failed: {error}"))?;
    if let Err(error) = extract_runtime(&archive_path, &staging) {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&archive_path);
        return Err(error);
    }
    let _ = fs::remove_file(&archive_path);
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("runtime replacement failed: {error}"))?;
    }
    fs::rename(&staging, &target)
        .map_err(|error| format!("runtime installation failed: {error}"))?;
    emit_progress(&app, "installed", 0, None);
    status_from_disk(&app)
}

/// Removes the installed Java compatibility runtime.
///
/// # Errors
///
/// Returns an error when the installed runtime cannot be removed.
#[tauri::command]
pub async fn java_runtime_uninstall<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<JavaRuntimeStatus, String> {
    let target = installed_runtime_dir(&app)?;
    if target.is_dir() {
        fs::remove_dir_all(&target).map_err(|error| format!("runtime removal failed: {error}"))?;
    }
    status_from_disk(&app)
}

/// Database types whose JDBC driver pack ships as its own release asset.
fn driver_pack_id(database_type: &str) -> Option<&'static str> {
    match database_type.trim().to_ascii_uppercase().as_str() {
        "DM" | "DAMENG" => Some("dm"),
        _ => None,
    }
}

/// Downloads and installs one JDBC driver pack into the runtime directory.
///
/// # Errors
///
/// Returns an error when the runtime is missing, the database type has no
/// published pack, or the download, checksum, or extraction fails.
#[tauri::command]
pub async fn jdbc_driver_pack_install<R: tauri::Runtime>(
    app: AppHandle<R>,
    database_type: String,
) -> Result<JavaRuntimeStatus, String> {
    let pack_id = driver_pack_id(&database_type)
        .ok_or_else(|| format!("no JDBC driver pack is published for {database_type}"))?;
    let runtime = installed_runtime_dir(&app)?;
    if !runtime.is_dir() {
        return Err("install the driver runtime before its driver packs".to_owned());
    }
    let archive_name = format!("chat2db-driver-pack-{pack_id}.zip");
    let base = format!("{}/v{}", release_base(), PRODUCT_VERSION);
    let archive_url = format!("{base}/{archive_name}");
    let checksum_url = format!("{archive_url}.sha256");
    let archive_path = runtime.join(format!(".driver-pack-{pack_id}.zip"));
    emit_progress(&app, "downloading", 0, None);
    download_archive(&app, &archive_url, &archive_path).await?;
    emit_progress(&app, "verifying", 0, None);
    let expected = download_checksum(&checksum_url).await?;
    if file_sha256(&archive_path)? != expected {
        let _ = fs::remove_file(&archive_path);
        return Err("driver pack checksum mismatch".to_owned());
    }
    emit_progress(&app, "installing", 0, None);
    let extracted = extract_runtime(&archive_path, &runtime);
    let _ = fs::remove_file(&archive_path);
    extracted?;
    emit_progress(&app, "installed", 0, None);
    status_from_disk(&app)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn entry_path_accepts_only_the_runtime_layout() {
        let destination = Path::new("/tmp/runtime");
        assert!(entry_path(destination, "java/bin/java").is_ok());
        assert!(entry_path(destination, "engine/chat2db-compat-runtime.jar").is_ok());
        assert!(entry_path(destination, "../escape").is_err());
        assert!(entry_path(destination, "/absolute").is_err());
        assert!(entry_path(destination, "other/file").is_err());
    }
}
