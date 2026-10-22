//! On-demand installation of the command-line entry point.

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

const CLI_INSTALL_PROGRESS_EVENT: &str = "chat2db://cli-install-progress";
const CLI_RELEASE_BASE_ENV: &str = "CHAT2DB_CLI_RELEASE_BASE";
const DEFAULT_RELEASE_BASE: &str = "https://github.com/OtterMind/Chat2DB-Rust/releases/download";
const CLI_VERSION_FILE: &str = ".cli-version";
const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Installation state of the optional command-line entry point.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliInstallProgress {
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

/// Name of the CLI executable inside the published release archive.
fn cli_archive_binary_name() -> &'static str {
    if cfg!(windows) {
        "chat2db.exe"
    } else {
        "chat2db"
    }
}

fn cli_executable_name() -> &'static str {
    if cfg!(windows) {
        "chat2db-bin.exe"
    } else {
        "chat2db-bin"
    }
}

fn cli_launcher_name() -> &'static str {
    if cfg!(windows) {
        "chat2db.cmd"
    } else {
        "chat2db"
    }
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
    std::env::var(CLI_RELEASE_BASE_ENV).unwrap_or_else(|_| DEFAULT_RELEASE_BASE.to_owned())
}

// The CLI must not be written into the application bundle: macOS code
// signatures cover it, and modifying a signed bundle breaks Gatekeeper.
fn cli_bin_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join("bin"))
        .map_err(|error| format!("application data directory is unavailable: {error}"))
}

fn cli_launcher_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(cli_bin_directory(app)?.join(cli_launcher_name()))
}

fn cli_executable_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(cli_bin_directory(app)?.join(cli_executable_name()))
}

fn cli_version_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(cli_bin_directory(app)?.join(CLI_VERSION_FILE))
}

fn read_installed_version(app: &AppHandle) -> Option<String> {
    cli_version_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn status_from_disk(app: &AppHandle) -> Result<CliInstallStatus, String> {
    let path = cli_launcher_path(app)?;
    let installed = path.is_file();
    Ok(CliInstallStatus {
        installed,
        version: installed.then(|| read_installed_version(app)).flatten(),
        path: installed.then(|| path.display().to_string()),
    })
}

/// Reports whether the command-line tool is installed for this desktop app.
///
/// # Errors
///
/// Returns an error when the application data directory cannot be resolved.
#[tauri::command]
pub async fn cli_status(app: AppHandle) -> Result<CliInstallStatus, String> {
    status_from_disk(&app)
}

fn emit_progress(
    app: &AppHandle,
    phase: &'static str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    let _ = app.emit(
        CLI_INSTALL_PROGRESS_EVENT,
        CliInstallProgress {
            phase,
            downloaded_bytes,
            total_bytes,
        },
    );
}

async fn download_archive(app: &AppHandle, url: &str, destination: &Path) -> Result<(), String> {
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
        if downloaded_bytes - last_reported >= 256 * 1024 {
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

fn extract_cli_binary(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file =
        fs::File::open(archive_path).map_err(|error| format!("archive open failed: {error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("archive is invalid: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("archive entry failed: {error}"))?;
        let entry_name = entry.name().to_owned();
        if Path::new(&entry_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(cli_archive_binary_name())
        {
            continue;
        }
        let staged = destination.with_extension("staged");
        {
            let mut output = fs::File::create(&staged)
                .map_err(|error| format!("CLI staging failed: {error}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("CLI extraction failed: {error}"))?;
            output
                .flush()
                .map_err(|error| format!("CLI extraction failed: {error}"))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
                .map_err(|error| format!("CLI permission update failed: {error}"))?;
        }
        fs::rename(&staged, destination).map_err(|error| format!("CLI install failed: {error}"))?;
        return Ok(());
    }
    Err("archive does not contain the chat2db binary".to_owned())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn launcher_script(
    java_bin: &Path,
    engine_jar: &Path,
    classpath: &Path,
    driver_packs: &Path,
) -> String {
    if cfg!(windows) {
        // Percent signs expand inside batch files, so escape them literally.
        let quote = |path: &Path| path.display().to_string().replace('%', "%%");
        format!(
            "@echo off\r\n\
             set \"CHAT2DB_JAVA_BIN={}\"\r\n\
             set \"CHAT2DB_JAVA_ENGINE_JAR={}\"\r\n\
             set \"CHAT2DB_COMMUNITY_CLASSPATH_DIR={}\"\r\n\
             set \"CHAT2DB_DRIVER_PACK_DIR={}\"\r\n\
             \"%~dp0{}\" %*\r\n",
            quote(java_bin),
            quote(engine_jar),
            quote(classpath),
            quote(driver_packs),
            cli_executable_name(),
        )
    } else {
        format!(
            "#!/bin/sh\n\
             CHAT2DB_JAVA_BIN={}\n\
             CHAT2DB_JAVA_ENGINE_JAR={}\n\
             CHAT2DB_COMMUNITY_CLASSPATH_DIR={}\n\
             CHAT2DB_DRIVER_PACK_DIR={}\n\
             export CHAT2DB_JAVA_BIN CHAT2DB_JAVA_ENGINE_JAR \
             CHAT2DB_COMMUNITY_CLASSPATH_DIR CHAT2DB_DRIVER_PACK_DIR\n\
             exec \"$(dirname \"$0\")/{}\" \"$@\"\n",
            shell_quote(&java_bin.display().to_string()),
            shell_quote(&engine_jar.display().to_string()),
            shell_quote(&classpath.display().to_string()),
            shell_quote(&driver_packs.display().to_string()),
            cli_executable_name(),
        )
    }
}

fn write_launcher(app: &AppHandle, bin_directory: &Path) -> Result<(), String> {
    // The on-demand runtime lives outside the app bundle; older packages that
    // still embed it fall back to the resource directory.
    let on_demand_root = app
        .path()
        .app_data_dir()
        .ok()
        .map(|directory| directory.join("java-runtime").join(PRODUCT_VERSION))
        .filter(|root| {
            root.join("engine")
                .join("chat2db-compat-runtime.jar")
                .is_file()
        });
    let resource_root = match on_demand_root {
        Some(root) => root,
        None => app
            .path()
            .resource_dir()
            .map(|directory| directory.join("chat2db"))
            .map_err(|error| format!("resource directory is unavailable: {error}"))?,
    };
    let java_bin = resource_root
        .join("java")
        .join("bin")
        .join(if cfg!(windows) { "java.exe" } else { "java" });
    let engine_jar = resource_root
        .join("engine")
        .join("chat2db-compat-runtime.jar");
    let classpath = resource_root.join("community-classpath");
    let driver_packs = resource_root.join("driver-packs");

    let script = launcher_script(&java_bin, &engine_jar, &classpath, &driver_packs);

    let launcher = bin_directory.join(cli_launcher_name());
    let staged = launcher.with_extension("staged");
    fs::write(&staged, script).map_err(|error| format!("CLI launcher write failed: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("CLI launcher permission update failed: {error}"))?;
    }
    fs::rename(&staged, &launcher).map_err(|error| format!("CLI launcher install failed: {error}"))
}

/// Downloads, verifies and installs the command-line tool for this desktop app.
///
/// # Errors
///
/// Returns an error when the download, checksum verification, extraction or
/// installation fails.
#[tauri::command]
pub async fn cli_install(app: AppHandle) -> Result<CliInstallStatus, String> {
    let bin_directory = cli_bin_directory(&app)?;
    fs::create_dir_all(&bin_directory)
        .map_err(|error| format!("CLI directory creation failed: {error}"))?;
    let archive_name = format!("chat2db-cli-{}.zip", platform_id());
    let base = format!("{}/v{}", release_base(), PRODUCT_VERSION);
    let archive_url = format!("{base}/{archive_name}");
    let checksum_url = format!("{archive_url}.sha256");

    let archive_path = bin_directory.join(".chat2db-cli-download.zip");
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
    let binary_path = cli_executable_path(&app)?;
    if let Err(error) = extract_cli_binary(&archive_path, &binary_path) {
        let _ = fs::remove_file(&archive_path);
        return Err(error);
    }
    let _ = fs::remove_file(&archive_path);
    write_launcher(&app, &bin_directory)?;
    fs::write(
        bin_directory.join(CLI_VERSION_FILE),
        format!("{PRODUCT_VERSION}\n"),
    )
    .map_err(|error| format!("CLI version record failed: {error}"))?;
    emit_progress(&app, "installed", 0, None);
    status_from_disk(&app)
}

/// Removes the installed command-line tool.
///
/// # Errors
///
/// Returns an error when the installed binary cannot be removed.
#[tauri::command]
pub async fn cli_uninstall(app: AppHandle) -> Result<CliInstallStatus, String> {
    for path in [cli_launcher_path(&app)?, cli_executable_path(&app)?] {
        if path.is_file() {
            fs::remove_file(&path).map_err(|error| format!("CLI removal failed: {error}"))?;
        }
    }
    if let Ok(version_path) = cli_version_path(&app) {
        let _ = fs::remove_file(version_path);
    }
    status_from_disk(&app)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn launcher_exports_runtime_paths_and_executes_the_cli() {
        let script = launcher_script(
            Path::new("/Applications/Chat2DB Rust.app/Contents/Resources/chat2db/java/bin/java"),
            Path::new(
                "/Applications/Chat2DB Rust.app/Contents/Resources/chat2db/engine/chat2db-compat-runtime.jar",
            ),
            Path::new(
                "/Applications/Chat2DB Rust.app/Contents/Resources/chat2db/community-classpath",
            ),
            Path::new("/Applications/Chat2DB Rust.app/Contents/Resources/chat2db/driver-packs"),
        );

        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(
            "CHAT2DB_JAVA_BIN='/Applications/Chat2DB Rust.app/Contents/Resources/chat2db/java/bin/java'"
        ));
        assert!(script.contains("export CHAT2DB_JAVA_BIN CHAT2DB_JAVA_ENGINE_JAR"));
        assert!(script.contains("exec \"$(dirname \"$0\")/chat2db-bin\" \"$@\""));
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("/tmp/it's here"), "'/tmp/it'\\''s here'");
    }
}
