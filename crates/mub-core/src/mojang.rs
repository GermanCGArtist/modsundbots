use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::sync::Semaphore;

use crate::error::{MubError, Result};
use crate::paths::AppPaths;

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMeta {
    pub id: String,
    pub main_class: String,
    pub asset_index: AssetIndexInfo,
    pub downloads: VersionDownloads,
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<VersionArguments>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexInfo {
    pub id: String,
    pub sha1: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionDownloads {
    pub client: DownloadArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadArtifact {
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadArtifact>,
    #[serde(default)]
    pub classifiers: HashMap<String, DownloadArtifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionArguments {
    #[serde(default)]
    pub game: Vec<serde_json::Value>,
    #[serde(default)]
    pub jvm: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderEntry {
    pub loader: FabricLoaderInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderInfo {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricProfileJson {
    pub main_class: String,
    pub libraries: Vec<FabricLibrary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLibrary {
    pub name: String,
    pub url: String,
}

/// Prepared launch classpath and metadata for a Minecraft version.
#[derive(Debug, Clone)]
pub struct PreparedVersion {
    pub version_id: String,
    pub main_class: String,
    pub asset_index_id: String,
    pub classpath: Vec<PathBuf>,
}

pub struct VersionManager {
    http: Client,
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionManager {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }

    pub async fn fetch_manifest(&self) -> Result<VersionManifest> {
        let resp = self.http.get(VERSION_MANIFEST_URL).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to fetch Mojang manifest: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    pub async fn fetch_fabric_loaders(&self, mc_version: &str) -> Result<Vec<FabricLoaderEntry>> {
        let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc_version}");
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to fetch Fabric loaders for {mc_version}: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    /// Ensure all shared assets, libraries, and client jar for `mc_version` are downloaded.
    pub async fn prepare_vanilla<F>(
        &self,
        paths: &AppPaths,
        mc_version: &str,
        natives_dir: &Path,
        max_concurrent: usize,
        on_progress: F,
    ) -> Result<PreparedVersion>
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        paths.ensure_dirs()?;
        let version_dir = paths.versions_dir().join(mc_version);
        std::fs::create_dir_all(&version_dir)?;

        on_progress(format!("Fetching version metadata for {mc_version}..."));
        let meta_path = version_dir.join(format!("{mc_version}.json"));
        let meta: VersionMeta = if meta_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?
        } else {
            let manifest = self.fetch_manifest().await?;
            let entry = manifest
                .versions
                .into_iter()
                .find(|v| v.id == mc_version)
                .ok_or_else(|| {
                    MubError::Download(format!("Minecraft version '{mc_version}' not found"))
                })?;
            let raw = self.http.get(&entry.url).send().await?.text().await?;
            std::fs::write(&meta_path, &raw)?;
            serde_json::from_str(&raw)?
        };

        // Download client.jar
        on_progress(format!("Checking client jar ({mc_version}.jar)..."));
        let client_jar = version_dir.join(format!("{mc_version}.jar"));
        self.download_if_missing(
            &meta.downloads.client.url,
            &client_jar,
            meta.downloads.client.sha1.as_deref(),
        )
        .await?;

        // Collect library download tasks + classpath
        let mut classpath = Vec::new();
        let mut download_tasks: Vec<(String, PathBuf, Option<String>)> = Vec::new();
        let mut native_jars: Vec<PathBuf> = Vec::new();
        let native_key = current_native_key();

        for lib in &meta.libraries {
            if !rules_allow(&lib.rules) {
                continue;
            }
            if let Some(downloads) = &lib.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let rel = artifact
                        .path
                        .clone()
                        .unwrap_or_else(|| maven_to_path(&lib.name));
                    let dest = paths.libraries_dir().join(&rel);
                    classpath.push(dest.clone());
                    if let Some(nk) = native_key
                        && lib.name.contains(nk)
                    {
                        native_jars.push(dest.clone());
                    }
                    download_tasks.push((artifact.url.clone(), dest, artifact.sha1.clone()));
                }
                if let Some(nk) = native_key
                    && let Some(native_art) = downloads.classifiers.get(nk)
                {
                    let rel = native_art
                        .path
                        .clone()
                        .unwrap_or_else(|| maven_to_path(&lib.name));
                    let dest = paths.libraries_dir().join(&rel);
                    native_jars.push(dest.clone());
                    download_tasks.push((native_art.url.clone(), dest, native_art.sha1.clone()));
                }
            }
        }

        // Download asset index
        on_progress(format!("Checking asset index {}...", meta.asset_index.id));
        let indexes_dir = paths.assets_dir().join("indexes");
        std::fs::create_dir_all(&indexes_dir)?;
        let index_path = indexes_dir.join(format!("{}.json", meta.asset_index.id));
        self.download_if_missing(
            &meta.asset_index.url,
            &index_path,
            Some(&meta.asset_index.sha1),
        )
        .await?;

        let asset_index: AssetIndex =
            serde_json::from_str(&std::fs::read_to_string(&index_path)?)?;
        let objects_dir = paths.assets_dir().join("objects");
        for obj in asset_index.objects.values() {
            let prefix = &obj.hash[..2];
            let dest = objects_dir.join(prefix).join(&obj.hash);
            let url = format!("{RESOURCES_BASE_URL}/{prefix}/{}", obj.hash);
            download_tasks.push((url, dest, Some(obj.hash.clone())));
        }

        // Execute all downloads concurrently with progress reporting
        let total = download_tasks.len();
        on_progress(format!("Verifying/downloading {total} files..."));
        self.run_download_batch(download_tasks, max_concurrent, &on_progress)
            .await?;

        // Extract natives into instance-specific natives dir
        on_progress("Extracting natives...".to_string());
        std::fs::create_dir_all(natives_dir)?;
        for jar_path in native_jars {
            if jar_path.exists() {
                extract_natives(&jar_path, natives_dir)?;
            }
        }

        classpath.push(client_jar);

        Ok(PreparedVersion {
            version_id: meta.id,
            main_class: meta.main_class,
            asset_index_id: meta.asset_index.id,
            classpath,
        })
    }

    /// Overlay Fabric loader libraries and main class on top of a prepared Vanilla version.
    pub async fn prepare_fabric<F>(
        &self,
        paths: &AppPaths,
        mc_version: &str,
        loader_version: &str,
        natives_dir: &Path,
        max_concurrent: usize,
        on_progress: F,
    ) -> Result<PreparedVersion>
    where
        F: Fn(String) + Send + Sync + Clone + 'static,
    {
        let mut prepared = self
            .prepare_vanilla(
                paths,
                mc_version,
                natives_dir,
                max_concurrent,
                on_progress.clone(),
            )
            .await?;

        on_progress(format!("Fetching Fabric {loader_version} profile..."));
        let url = format!(
            "https://meta.fabricmc.net/v2/versions/loader/{mc_version}/{loader_version}/profile/json"
        );
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to fetch Fabric profile for {mc_version}/{loader_version}: {}",
                resp.status()
            )));
        }
        let fabric: FabricProfileJson = resp.json().await?;

        let mut tasks = Vec::new();
        let mut fabric_cp = Vec::new();
        for lib in fabric.libraries {
            let rel = maven_to_path(&lib.name);
            let dest = paths.libraries_dir().join(&rel);
            let base = lib.url.trim_end_matches('/');
            let dl_url = format!("{base}/{rel}");
            fabric_cp.push(dest.clone());
            tasks.push((dl_url, dest, None));
        }

        self.run_download_batch(tasks, max_concurrent, &on_progress)
            .await?;
        fabric_cp.extend(prepared.classpath);
        prepared.classpath = fabric_cp;
        prepared.main_class = fabric.main_class;
        Ok(prepared)
    }

    async fn run_download_batch<F>(
        &self,
        tasks: Vec<(String, PathBuf, Option<String>)>,
        max_concurrent: usize,
        on_progress: &F,
    ) -> Result<()>
    where
        F: Fn(String) + Send + Sync,
    {
        let total = tasks.len();
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sem = Arc::new(Semaphore::new(max_concurrent.max(1)));
        let results: Vec<Result<()>> = stream::iter(tasks)
            .map(|(url, dest, sha1)| {
                let sem = Arc::clone(&sem);
                let done = Arc::clone(&done);
                async move {
                    let _permit = sem.acquire().await.map_err(|e| {
                        MubError::Download(format!("Semaphore error: {e}"))
                    })?;
                    let res = self.download_if_missing(&url, &dest, sha1.as_deref()).await;
                    let count = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if count.is_multiple_of(100) || count == total {
                        on_progress(format!("Files {count}/{total}"));
                    }
                    res
                }
            })
            .buffer_unordered(max_concurrent.max(1))
            .collect()
            .await;

        for res in results {
            res?;
        }
        Ok(())
    }

    async fn download_if_missing(
        &self,
        url: &str,
        dest: &Path,
        expected_sha1: Option<&str>,
    ) -> Result<()> {
        if dest.exists() {
            if let Some(expected) = expected_sha1 {
                if verify_sha1(dest, expected)? {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "GET {url} failed with status {}",
                resp.status()
            )));
        }
        let bytes = resp.bytes().await?;
        if let Some(expected) = expected_sha1 {
            let mut hasher = Sha1::new();
            hasher.update(&bytes);
            let actual = hex::encode(hasher.finalize());
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(MubError::Download(format!(
                    "SHA-1 mismatch for {url}: expected {expected}, got {actual}"
                )));
            }
        }

        std::fs::write(dest, &bytes)?;
        Ok(())
    }
}

fn verify_sha1(path: &Path, expected: &str) -> Result<bool> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    Ok(actual.eq_ignore_ascii_case(expected))
}

fn rules_allow(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let current_os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };

    let mut allowed = false;
    for rule in rules {
        let os_matches = match &rule.os {
            Some(os) => os.name.as_deref() == Some(current_os),
            None => true,
        };
        if os_matches {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn current_native_key() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("natives-windows")
    } else if cfg!(target_os = "macos") {
        Some("natives-osx")
    } else if cfg!(target_os = "linux") {
        Some("natives-linux")
    } else {
        None
    }
}

fn maven_to_path(coord: &str) -> String {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return coord.to_string();
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    if parts.len() >= 4 {
        let classifier = parts[3];
        format!("{group}/{artifact}/{version}/{artifact}-{version}-{classifier}.jar")
    } else {
        format!("{group}/{artifact}/{version}/{artifact}-{version}.jar")
    }
}

fn extract_natives(jar_path: &Path, natives_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if entry.is_dir() || name.starts_with("META-INF") {
            continue;
        }
        let Some(file_name) = Path::new(&name).file_name() else {
            continue;
        };
        let out_path = natives_dir.join(file_name);
        let mut out_file = std::fs::File::create(out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
    }
    Ok(())
}
