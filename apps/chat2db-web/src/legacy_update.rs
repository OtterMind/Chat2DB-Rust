//! Update discovery and staging for the desktop product.
//!
//! The retained Community settings page calls the historical \`/api/system*\`
//! endpoints. The upstream desktop backend keeps them closed source, so this
//! module implements the same contract against the repository's GitHub releases.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;

const RELEASES_API: &str = "https://api.github.com/repos/OtterMind/Chat2DB-Rust/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersion {
    pub desktop: bool,
    pub version: String,
    pub hot_upgrade_url: Option<String>,
    pub r#type: String,
    pub need_update: bool,
    pub download_link: Option<String>,
    pub update_log: Option<String>,
    pub white_list: Option<String>,
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn state_dir() -> Option<PathBuf> {
    chat2db_storage::Storage::default_data_dir()
        .ok()
        .map(|directory| directory.join("updates"))
}

fn preference_path(directory: &Path) -> PathBuf {
    directory.join("update-preference.json")
}

fn staged_archive(directory: &Path, version: &str) -> Option<PathBuf> {
    let entry = std::fs::read_dir(directory.join(version))
        .ok()?
        .flatten()
        .next()?;
    let path = entry.path();
    path.is_file().then_some(path)
}

fn digits(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let candidate = digits(candidate);
    let current = digits(current);
    for index in 0..candidate.len().max(current.len()) {
        let left = candidate.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

const fn asset_suffix() -> &'static [&'static str] {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        &["aarch64.dmg"]
    } else if cfg!(target_os = "macos") {
        &["x86_64.dmg"]
    } else if cfg!(target_os = "windows") {
        &["x64-setup.exe", "x64_en-US.msi"]
    } else {
        &[".AppImage", "amd64.deb"]
    }
}

fn pick_asset(release: &Release) -> Option<&Asset> {
    asset_suffix().iter().find_map(|suffix| {
        release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with(suffix))
    })
}

fn update_type(directory: &Path) -> String {
    std::fs::read_to_string(preference_path(directory))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("type")
                .and_then(|kind| kind.as_str())
                .map(str::to_owned)
        })
        .filter(|kind| kind == "auto" || kind == "manual")
        .unwrap_or_else(|| "manual".to_owned())
}

fn unchanged(directory: Option<&Path>) -> LatestVersion {
    LatestVersion {
        desktop: true,
        version: CURRENT_VERSION.to_owned(),
        hot_upgrade_url: None,
        r#type: directory.map_or_else(|| "manual".to_owned(), update_type),
        need_update: false,
        download_link: None,
        update_log: None,
        white_list: None,
    }
}

/// Asks the release feed whether a newer desktop build exists.
pub async fn latest_version() -> LatestVersion {
    let directory = state_dir();
    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("chat2db-rust/", env!("CARGO_PKG_VERSION")))
        .timeout(NETWORK_TIMEOUT)
        .build()
    else {
        return unchanged(directory.as_deref());
    };
    let release = match client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response.json::<Release>().await.ok(),
        _ => None,
    };
    let Some(release) = release else {
        return unchanged(directory.as_deref());
    };
    let version = release.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&version, CURRENT_VERSION) {
        return unchanged(directory.as_deref());
    }
    let download_link = pick_asset(&release).map(|asset| asset.browser_download_url.clone());
    LatestVersion {
        desktop: true,
        version,
        hot_upgrade_url: None,
        r#type: directory
            .as_deref()
            .map_or_else(|| "manual".to_owned(), update_type),
        need_update: true,
        download_link,
        update_log: release.body,
        white_list: None,
    }
}

/// Stores whether the user asked for manual or automatic updates.
#[must_use]
pub fn set_update_type(kind: &str) -> bool {
    let Some(directory) = state_dir() else {
        return false;
    };
    if kind != "auto" && kind != "manual" {
        return false;
    }
    let _ = std::fs::create_dir_all(&directory);
    let payload = serde_json::json!({ "type": kind });
    match serde_json::to_vec_pretty(&payload) {
        Ok(bytes) => std::fs::write(preference_path(&directory), bytes).is_ok(),
        Err(_) => false,
    }
}

#[must_use]
pub fn is_update_success(version: &str) -> bool {
    state_dir()
        .as_deref()
        .and_then(|directory| staged_archive(directory, version))
        .is_some()
}

/// Downloads the platform installer and keeps it ready for the user to run.
pub async fn stage_update(version: &str, download_link: &str) -> bool {
    let Some(directory) = state_dir() else {
        return false;
    };
    if version.is_empty() || download_link.is_empty() {
        return false;
    }
    let target_dir = directory.join(version);
    if staged_archive(&directory, version).is_some() {
        return true;
    }
    if std::fs::create_dir_all(&target_dir).is_err() {
        return false;
    }
    let file_name = download_link
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("chat2db-update");
    let target = target_dir.join(file_name);
    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("chat2db-rust/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .build()
    else {
        return false;
    };
    let response = match client.get(download_link).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return false,
    };
    let Ok(bytes) = response.bytes().await else {
        return false;
    };
    std::fs::write(&target, bytes).is_ok()
}

#[must_use]
pub fn current_version() -> &'static str {
    CURRENT_VERSION
}
