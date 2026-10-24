//! SQLX command line integration for the desktop host.
//!
//! The Community settings page drives this through the \`client-command\` bridge:
//! it asks for the status of the local \`sqlx\` executable and can point the app
//! at a binary the user already has. Downloading a release and importing
//! datasources into SQLX are not implemented yet and report that explicitly
//! instead of failing silently.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const BINARY_FILE: &str = "sqlx-binary.json";
const INSTALL_DIRECTORY: &str = "sqlx";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlxLatestVersion {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlxStatus {
    pub state: &'static str,
    pub platform: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
    pub on_path: bool,
    pub install_dir: String,
    pub latest: SqlxLatestVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoredBinary {
    path: String,
}

pub const fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

fn root_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(INSTALL_DIRECTORY))
        .map_err(|error| format!("application data directory is unavailable: {error}"))
}

fn binary_record_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(root_directory(app)?.join(BINARY_FILE))
}

fn install_directory(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(root_directory(app)?.join("bin"))
}

fn stored_binary(app: &AppHandle) -> Option<PathBuf> {
    let path = binary_record_path(app).ok()?;
    let raw = fs::read_to_string(path).ok()?;
    let stored: StoredBinary = serde_json::from_str(&raw).ok()?;
    let path = PathBuf::from(stored.path.trim());
    path.is_file().then_some(path)
}

fn binary_on_path() -> Option<PathBuf> {
    let output = Command::new("sqlx").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.split_whitespace().next()?;
    if first.eq_ignore_ascii_case("sqlx") {
        which_sqlx()
    } else {
        None
    }
}

fn which_sqlx() -> Option<PathBuf> {
    let command = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let output = Command::new(command).arg("sqlx").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next()?.trim();
    let path = PathBuf::from(first);
    path.is_file().then_some(path)
}

/// Reports the version printed by \`<binary> --version\`, or \`None\` when the
/// executable does not answer like the SQLX CLI.
fn probe_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split_whitespace();
    let name = parts.next()?;
    if !name.eq_ignore_ascii_case("sqlx") {
        return None;
    }
    parts
        .find(|part| part.chars().any(|c| c.is_ascii_digit()))
        .map(|version| version.trim_start_matches('v').to_owned())
}

pub fn status(app: &AppHandle) -> Result<SqlxStatus, String> {
    let install_dir = install_directory(app)?.display().to_string();
    let on_path_binary = binary_on_path();
    let stored = stored_binary(app);

    let (path, source) = match stored {
        Some(path) => (Some(path), Some("external")),
        None => (
            on_path_binary.clone(),
            on_path_binary.as_ref().map(|_| "external"),
        ),
    };
    let version = path.as_deref().and_then(probe_version);
    let state = if path.is_some() && version.is_some() {
        "installed"
    } else {
        "notInstalled"
    };

    Ok(SqlxStatus {
        state,
        platform: platform(),
        version,
        path: path.as_ref().map(|path| path.display().to_string()),
        source,
        on_path: on_path_binary.is_some(),
        install_dir,
        latest: SqlxLatestVersion {
            status: "unknown",
            version: None,
            checked_at: None,
            error: None,
        },
        message: None,
    })
}

pub fn set_binary(app: &AppHandle, path: &str) -> Result<SqlxStatus, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("The SQLX binary path is empty".to_owned());
    }
    let candidate = PathBuf::from(trimmed);
    if !candidate.is_file() {
        return Err(format!("{trimmed} is not a file"));
    }
    if probe_version(&candidate).is_none() {
        return Err(format!("{trimmed} is not the SQLX command line tool"));
    }
    fs::create_dir_all(root_directory(app)?).map_err(|error| error.to_string())?;
    let record = serde_json::json!({ "path": candidate.display().to_string() });
    fs::write(
        binary_record_path(app)?,
        serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    status(app)
}

/// Installation from a release feed is not part of this build yet.
pub fn unsupported(operation: &str) -> String {
    format!(
        "{operation} is not supported by this build yet; point the app at an existing SQLX binary instead"
    )
}
