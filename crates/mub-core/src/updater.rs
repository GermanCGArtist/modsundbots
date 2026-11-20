use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use tar::Archive;
use zip::ZipArchive;

use crate::error::{MubError, Result};

pub const GITHUB_OWNER: &str = "GermanCGArtist";
pub const GITHUB_REPO: &str = "modsundbots";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub tag_name: String,
    pub release_name: String,
    pub release_notes: String,
    pub html_url: String,
    pub published_at: Option<String>,
    pub update_available: bool,
    pub asset_name: Option<String>,
    pub asset_api_url: Option<String>,
    pub asset_download_url: Option<String>,
    pub asset_size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct InstalledUpdateResult {
    pub new_version: String,
    pub updated_binaries: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    url: String,
    browser_download_url: String,
    size: u64,
}

/// Parses a version string like `"v0.3.0"` or `"0.3.0-beta.1"` into `(major, minor, patch, pre)`.
pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64, bool)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let (core, is_pre) = match trimmed.split_once('-') {
        Some((c, _)) => (c, true),
        None => (trimmed, false),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch, is_pre))
}

/// Returns `true` when `latest` is strictly newer than `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let Some((c_maj, c_min, c_pat, c_pre)) = parse_semver(current) else {
        return false;
    };
    let Some((l_maj, l_min, l_pat, l_pre)) = parse_semver(latest) else {
        return false;
    };
    if (l_maj, l_min, l_pat) != (c_maj, c_min, c_pat) {
        return (l_maj, l_min, l_pat) > (c_maj, c_min, c_pat);
    }
    // Same core version: stable (pre=false) is newer than pre-release (pre=true)
    c_pre && !l_pre
}

fn preferred_asset_names(running_bin_name: &str) -> Vec<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("windows", "x86_64") => {
            if running_bin_name.eq_ignore_ascii_case("mub.exe") {
                vec!["modsundbots-windows-x86_64.zip", "mub.exe", "modsundbots.exe"]
            } else {
                vec!["modsundbots-windows-x86_64.zip", "modsundbots.exe"]
            }
        }
        ("windows", "aarch64") => vec!["modsundbots-windows-arm64.zip", "modsundbots-windows-x86_64.zip"],
        ("macos", "aarch64") => vec!["modsundbots-macos-arm64.tar.gz"],
        ("macos", "x86_64") => vec!["modsundbots-macos-x86_64.tar.gz"],
        ("linux", "x86_64") => vec!["modsundbots-linux-x86_64.tar.gz"],
        ("linux", "aarch64") => vec!["modsundbots-linux-arm64.tar.gz", "modsundbots-linux-aarch64.tar.gz"],
        _ => vec![],
    }
}

fn select_asset<'a>(assets: &'a [GhAsset], running_bin_name: &str) -> Option<&'a GhAsset> {
    let candidates = preferred_asset_names(running_bin_name);
    for candidate in candidates {
        if let Some(found) = assets.iter().find(|a| a.name.eq_ignore_ascii_case(candidate)) {
            return Some(found);
        }
    }
    None
}

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| MubError::Launch(format!("Failed to initialize HTTP client: {e}")))
}

/// Checks GitHub Releases for the latest version of ModsUndBots.
pub async fn check_for_updates(github_token: Option<&str>) -> Result<UpdateCheckResult> {
    let client = build_client()?;
    let url = format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest");

    let mut req = client
        .get(&url)
        .header(USER_AGENT, format!("ModsUndBots/{CURRENT_VERSION}"))
        .header(ACCEPT, "application/vnd.github+json");

    let token = github_token.map(str::trim).filter(|t| !t.is_empty());
    if let Some(tok) = token {
        req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
    }

    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        let detail = if matches!(status.as_u16(), 401 | 403 | 404) && token.is_none() {
            format!(
                "GitHub returned {status}. Because {GITHUB_OWNER}/{GITHUB_REPO} is private, configure a GitHub Personal Access Token (repo / Contents: Read) in Settings -> App Updates or set MUB_GITHUB_TOKEN."
            )
        } else {
            let body = resp.text().await.unwrap_or_default();
            format!("GitHub update check failed ({status}): {body}")
        };
        return Err(MubError::Launch(detail));
    }

    let release: GhRelease = resp.json().await?;
    let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    let update_available = is_newer_version(CURRENT_VERSION, &release.tag_name);

    let running_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();
    let asset = select_asset(&release.assets, &running_name);

    Ok(UpdateCheckResult {
        current_version: CURRENT_VERSION.to_string(),
        latest_version,
        tag_name: release.tag_name,
        release_name: release.name.unwrap_or_default(),
        release_notes: release.body.unwrap_or_default(),
        html_url: release.html_url,
        published_at: release.published_at,
        update_available,
        asset_name: asset.map(|a| a.name.clone()),
        asset_api_url: asset.map(|a| a.url.clone()),
        asset_download_url: asset.map(|a| a.browser_download_url.clone()),
        asset_size: asset.map(|a| a.size),
    })
}

/// Downloads the release asset for the current OS/arch and replaces the running binary
/// (plus sibling `mub` / `modsundbots` binary if present in the same directory).
pub async fn download_and_install_update(
    check: &UpdateCheckResult,
    github_token: Option<&str>,
) -> Result<InstalledUpdateResult> {
    let asset_name = check.asset_name.as_deref().ok_or_else(|| {
        MubError::Launch(format!(
            "No compatible release asset found for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;

    let token = github_token.map(str::trim).filter(|t| !t.is_empty());
    let client = build_client()?;

    let bytes = if let (Some(tok), Some(api_url)) = (token, check.asset_api_url.as_deref()) {
        let resp = client
            .get(api_url)
            .header(USER_AGENT, format!("ModsUndBots/{CURRENT_VERSION}"))
            .header(ACCEPT, "application/octet-stream")
            .header(AUTHORIZATION, format!("Bearer {tok}"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(MubError::Launch(format!(
                "Failed to download update asset ({}): HTTP {}",
                asset_name,
                resp.status()
            )));
        }
        resp.bytes().await?
    } else if let Some(dl_url) = check.asset_download_url.as_deref() {
        let resp = client
            .get(dl_url)
            .header(USER_AGENT, format!("ModsUndBots/{CURRENT_VERSION}"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(MubError::Launch(format!(
                "Failed to download update asset ({}): HTTP {}",
                asset_name,
                resp.status()
            )));
        }
        resp.bytes().await?
    } else {
        return Err(MubError::Launch("Release asset has no download URL".into()));
    };

    let current_exe = std::env::current_exe()
        .map_err(|e| MubError::Launch(format!("Cannot locate current executable: {e}")))?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| MubError::Launch("Current executable has no parent directory".into()))?;
    let current_name = current_exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MubError::Launch("Invalid executable filename".into()))?
        .to_string();

    let extracted = extract_binaries_from_asset(asset_name, &bytes)?;
    if extracted.is_empty() {
        return Err(MubError::Launch(format!(
            "Update archive '{asset_name}' did not contain any expected binaries"
        )));
    }

    let mut updated_binaries = Vec::new();
    for (bin_name, bin_bytes) in extracted {
        let target_path = exe_dir.join(&bin_name);
        let is_current = bin_name.eq_ignore_ascii_case(&current_name);
        if is_current || target_path.exists() {
            replace_binary_atomically(&target_path, &bin_bytes)?;
            updated_binaries.push(target_path);
        }
    }

    if updated_binaries.is_empty() {
        return Err(MubError::Launch(format!(
            "Archive '{asset_name}' did not contain '{current_name}'"
        )));
    }

    Ok(InstalledUpdateResult {
        new_version: check.latest_version.clone(),
        updated_binaries,
    })
}

fn is_expected_binary(name: &str) -> bool {
    matches!(
        name,
        "modsundbots" | "modsundbots.exe" | "mub" | "mub.exe"
    )
}

fn extract_binaries_from_asset(asset_name: &str, bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let lower = asset_name.to_ascii_lowercase();
    let mut out = Vec::new();

    if lower.ends_with(".zip") {
        let reader = Cursor::new(bytes);
        let mut archive = ZipArchive::new(reader)
            .map_err(|e| MubError::Launch(format!("Invalid zip update archive: {e}")))?;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| MubError::Launch(format!("Failed reading zip entry: {e}")))?;
            if entry.is_dir() {
                continue;
            }
            let file_name = Path::new(entry.name())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if is_expected_binary(&file_name) {
                let mut buf = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut buf)?;
                out.push((file_name, buf));
            }
        }
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let gz = GzDecoder::new(Cursor::new(bytes));
        let mut archive = Archive::new(gz);
        let entries = archive
            .entries()
            .map_err(|e| MubError::Launch(format!("Invalid tar.gz update archive: {e}")))?;
        for entry_res in entries {
            let mut entry = entry_res
                .map_err(|e| MubError::Launch(format!("Failed reading tar entry: {e}")))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry
                .path()
                .map_err(|e| MubError::Launch(format!("Invalid tar entry path: {e}")))?;
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if is_expected_binary(&file_name) {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                out.push((file_name, buf));
            }
        }
    } else if is_expected_binary(asset_name) {
        out.push((asset_name.to_string(), bytes.to_vec()));
    }

    Ok(out)
}

fn replace_binary_atomically(target: &Path, new_bytes: &[u8]) -> Result<()> {
    let new_path = PathBuf::from(format!("{}.new", target.display()));
    let old_path = PathBuf::from(format!("{}.old", target.display()));

    if old_path.exists() {
        let _ = fs::remove_file(&old_path);
    }

    {
        let mut file = File::create(&new_path)?;
        file.write_all(new_bytes)?;
        file.sync_all()?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&new_path, perms)?;
    }

    if target.exists() {
        fs::rename(target, &old_path).map_err(|e| {
            let _ = fs::remove_file(&new_path);
            MubError::Launch(format!(
                "Failed to stage old binary '{}': {e}",
                target.display()
            ))
        })?;
    }

    if let Err(e) = fs::rename(&new_path, target) {
        // Attempt rollback if moving the new binary into place failed.
        if old_path.exists() {
            let _ = fs::rename(&old_path, target);
        }
        return Err(MubError::Launch(format!(
            "Failed to install new binary '{}': {e}",
            target.display()
        )));
    }

    // Best-effort cleanup of .old (on Windows the running .exe cannot be deleted until next launch).
    let _ = fs::remove_file(&old_path);
    Ok(())
}

/// Removes leftover `.old` / `.new` update staging files next to the running executable.
pub fn cleanup_old_update_files() {
    let Ok(current) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = current.parent() else {
        return;
    };
    for name in ["modsundbots", "modsundbots.exe", "mub", "mub.exe"] {
        for ext in [".old", ".new"] {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.exists() {
                let _ = fs::remove_file(&candidate);
            }
        }
    }
}

/// Spawns a fresh instance of the current executable (used after installing an update).
pub fn spawn_updated_relaunch() -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| MubError::Launch(format!("Cannot locate executable to relaunch: {e}")))?;
    std::process::Command::new(exe)
        .spawn()
        .map_err(|e| MubError::Launch(format!("Failed to relaunch updated app: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing_and_comparison() {
        assert_eq!(parse_semver("v0.3.0"), Some((0, 3, 0, false)));
        assert_eq!(parse_semver("0.2.1-beta"), Some((0, 2, 1, true)));
        assert!(is_newer_version("0.2.0", "v0.3.0"));
        assert!(is_newer_version("0.3.0", "0.3.1"));
        assert!(is_newer_version("0.3.0-rc.1", "0.3.0"));
        assert!(!is_newer_version("0.3.0", "v0.3.0"));
        assert!(!is_newer_version("0.3.1", "v0.3.0"));
        assert!(!is_newer_version("0.3.0", "v0.3.0-rc.1"));
    }
}
