use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

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
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
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
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVersionInfo {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderEntry {
    pub loader: FabricLoaderInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderInfo {
    pub version: String,
    #[serde(default = "default_true")]
    pub stable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricProfileJson {
    pub main_class: String,
    pub libraries: Vec<FabricLibrary>,
    #[serde(default)]
    pub arguments: Option<VersionArguments>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLibrary {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NeoForgeVersionJson {
    pub id: String,
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<VersionArguments>,
}

/// Prepared launch classpath and metadata for a Minecraft version.
#[derive(Debug, Clone)]
pub struct PreparedVersion {
    pub version_id: String,
    pub main_class: String,
    pub asset_index_id: String,
    pub classpath: Vec<PathBuf>,
    pub extra_jvm_args: Vec<String>,
    pub extra_game_args: Vec<String>,
}

#[derive(Clone)]
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
        let http = Client::builder()
            .user_agent("ModsUndBots/0.2.0 (https://github.com/modsundbots)")
            .pool_max_idle_per_host(16)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { http }
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

    pub async fn fetch_fabric_loader_versions(
        &self,
        mc_version: &str,
    ) -> Result<Vec<LoaderVersionInfo>> {
        let entries = self.fetch_fabric_loaders(mc_version).await?;
        Ok(entries
            .into_iter()
            .map(|e| LoaderVersionInfo {
                version: e.loader.version,
                stable: e.loader.stable,
            })
            .collect())
    }

    pub async fn fetch_quilt_loaders(&self, mc_version: &str) -> Result<Vec<LoaderVersionInfo>> {
        let url = format!("https://meta.quiltmc.org/v3/versions/loader/{mc_version}");
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to fetch Quilt loaders for {mc_version}: {}",
                resp.status()
            )));
        }
        let entries: Vec<FabricLoaderEntry> = resp.json().await?;
        Ok(entries
            .into_iter()
            .map(|e| {
                let ver = e.loader.version;
                let stable = e.loader.stable
                    && !ver.contains("beta")
                    && !ver.contains("alpha")
                    && !ver.contains("pre");
                LoaderVersionInfo {
                    version: ver,
                    stable,
                }
            })
            .collect())
    }

    pub async fn fetch_neoforge_versions(
        &self,
        mc_version: &str,
    ) -> Result<Vec<LoaderVersionInfo>> {
        let prefix = neoforge_mc_prefix(mc_version);
        let url =
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to fetch NeoForge metadata: {}",
                resp.status()
            )));
        }
        let xml = resp.text().await?;
        let mut versions = parse_maven_metadata_versions(&xml, &prefix);
        versions.sort_by(|a, b| compare_neoforge_versions(&b.version, &a.version));
        Ok(versions)
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

        // Serialize shared downloads/metadata writes for the same MC version so
        // parallel collection launches never race on shared files in `shared/`.
        // A short-lived in-memory cache avoids re-hashing thousands of asset files
        // when starting multiple profiles of the same MC version in parallel.
        let (meta, mut classpath, native_jars, client_jar) = {
            let lock = shared_prep_lock(&format!("vanilla:{mc_version}"));
            let _guard = lock.lock().await;

            let cache_key = (paths.root.clone(), mc_version.to_string());
            if let Some(cached) = get_cached_vanilla(&cache_key)
                && cached.client_jar.exists()
            {
                on_progress(format!("Using verified files for {mc_version}..."));
                (
                    cached.meta,
                    cached.classpath,
                    cached.native_jars,
                    cached.client_jar,
                )
            } else {
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
                    atomic_write_file(&meta_path, raw.as_bytes())?;
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

                for lib in &meta.libraries {
                    if !rules_allow(&lib.rules) {
                        continue;
                    }
                    if is_wrong_arch_native_coord(&lib.name) {
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
                            if is_current_arch_native_coord(&lib.name) {
                                native_jars.push(dest.clone());
                            }
                            download_tasks.push((artifact.url.clone(), dest, artifact.sha1.clone()));
                        }
                        if let Some(native_art) =
                            select_native_classifier(lib, &downloads.classifiers)
                        {
                            let rel = native_art
                                .path
                                .clone()
                                .unwrap_or_else(|| maven_to_path(&lib.name));
                            let dest = paths.libraries_dir().join(&rel);
                            native_jars.push(dest.clone());
                            download_tasks.push((
                                native_art.url.clone(),
                                dest,
                                native_art.sha1.clone(),
                            ));
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

                put_cached_vanilla(
                    cache_key,
                    CachedVanilla {
                        meta: meta.clone(),
                        classpath: classpath.clone(),
                        native_jars: native_jars.clone(),
                        client_jar: client_jar.clone(),
                        verified_at: std::time::Instant::now(),
                    },
                );

                (meta, classpath, native_jars, client_jar)
            }
        };

        // Extract natives into instance-specific natives dir (runs in parallel across instances)
        on_progress("Extracting natives...".to_string());
        std::fs::create_dir_all(natives_dir)?;
        for jar_path in native_jars {
            if jar_path.exists() {
                extract_natives(&jar_path, natives_dir)?;
            }
        }

        // Collect vanilla JVM arguments (e.g. -XX:StackShadowPages=32, --enable-native-access,
        // --add-exports, per-instance JNA/LWJGL/Netty native temp dirs) and ensure per-instance
        // native extraction subdirectories exist so parallel JVMs never collide in %TEMP%.
        let mut extra_jvm_args = meta
            .arguments
            .as_ref()
            .map(|a| collect_vanilla_jvm_args(&a.jvm, paths, &meta.id, natives_dir))
            .unwrap_or_default();
        ensure_isolated_native_jvm_args(&mut extra_jvm_args, natives_dir)?;

        classpath.push(client_jar);

        Ok(PreparedVersion {
            version_id: meta.id,
            main_class: meta.main_class,
            asset_index_id: meta.asset_index.id,
            classpath,
            extra_jvm_args,
            extra_game_args: Vec::new(),
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
        self.apply_loader_profile_json(
            paths,
            &url,
            "https://maven.fabricmc.net",
            &mut prepared,
            natives_dir,
            max_concurrent,
            &on_progress,
        )
        .await?;
        Ok(prepared)
    }

    /// Overlay Quilt loader libraries and main class on top of a prepared Vanilla version.
    pub async fn prepare_quilt<F>(
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

        on_progress(format!("Fetching Quilt {loader_version} profile..."));
        let url = format!(
            "https://meta.quiltmc.org/v3/versions/loader/{mc_version}/{loader_version}/profile/json"
        );
        self.apply_loader_profile_json(
            paths,
            &url,
            "https://maven.quiltmc.org/repository/release",
            &mut prepared,
            natives_dir,
            max_concurrent,
            &on_progress,
        )
        .await?;
        Ok(prepared)
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_loader_profile_json<F>(
        &self,
        paths: &AppPaths,
        url: &str,
        default_maven_base: &str,
        prepared: &mut PreparedVersion,
        natives_dir: &Path,
        max_concurrent: usize,
        on_progress: &F,
    ) -> Result<()>
    where
        F: Fn(String) + Send + Sync,
    {
        let lock = shared_prep_lock(&format!("loader:{url}"));
        let _guard = lock.lock().await;

        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to fetch loader profile {url}: {}",
                resp.status()
            )));
        }
        let profile: FabricProfileJson = resp.json().await?;

        let mut tasks = Vec::new();
        let mut loader_cp = Vec::new();
        for lib in profile.libraries {
            let rel = maven_to_path(&lib.name);
            let dest = paths.libraries_dir().join(&rel);
            let base = lib
                .url
                .as_deref()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or(default_maven_base)
                .trim_end_matches('/');
            let dl_url = format!("{base}/{rel}");
            loader_cp.push(dest.clone());
            tasks.push((dl_url, dest, None));
        }

        self.run_download_batch(tasks, max_concurrent, on_progress)
            .await?;
        loader_cp.extend(std::mem::take(&mut prepared.classpath));
        prepared.classpath = dedup_paths(loader_cp);
        prepared.main_class = profile.main_class;

        if let Some(args) = profile.arguments {
            let version_id = prepared.version_id.clone();
            prepared.extra_jvm_args.extend(collect_allowed_args(
                &args.jvm,
                paths,
                &version_id,
                natives_dir,
            ));
            prepared.extra_game_args.extend(collect_allowed_args(
                &args.game,
                paths,
                &version_id,
                natives_dir,
            ));
        }

        Ok(())
    }

    /// Install (if needed) and overlay NeoForge on top of a prepared Vanilla version.
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_neoforge<F>(
        &self,
        paths: &AppPaths,
        mc_version: &str,
        loader_version: &str,
        natives_dir: &Path,
        max_concurrent: usize,
        java_bin: &str,
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

        let lock = shared_prep_lock(&format!("neoforge:{loader_version}"));
        let _guard = lock.lock().await;

        let nf_version_id = format!("neoforge-{loader_version}");
        let nf_dir = paths.versions_dir().join(&nf_version_id);
        let nf_json_path = nf_dir.join(format!("{nf_version_id}.json"));

        if !nf_json_path.exists() {
            // Ensure minimal launcher_profiles.json exists for NeoForge installer
            let launcher_profiles = paths.shared_dir().join("launcher_profiles.json");
            if !launcher_profiles.exists() {
                std::fs::write(
                    &launcher_profiles,
                    r#"{"profiles":{},"selectedProfile":"","clientToken":"00000000-0000-0000-0000-000000000000"}"#,
                )?;
            }

            let cache_dir = paths.shared_dir().join("cache");
            std::fs::create_dir_all(&cache_dir)?;
            let installer_jar =
                cache_dir.join(format!("neoforge-{loader_version}-installer.jar"));
            let installer_url = format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar"
            );

            on_progress(format!("Downloading NeoForge {loader_version} installer..."));
            self.download_if_missing(&installer_url, &installer_jar, None)
                .await?;

            let resolved_java = if java_bin.trim().is_empty() || java_bin.trim() == "java" {
                let mc = mc_version.to_string();
                let jb = java_bin.to_string();
                tokio::task::spawn_blocking(move || {
                    crate::java::resolve_java_for_mc(None, &jb, &mc)
                })
                .await
                .unwrap_or_else(|_| java_bin.to_string())
            } else {
                java_bin.to_string()
            };

            on_progress(format!(
                "Running NeoForge {loader_version} installer (this may take a minute)..."
            ));
            let mut cmd = tokio::process::Command::new(&resolved_java);
            cmd.arg("-jar")
                .arg(&installer_jar)
                .arg("--installClient")
                .arg(paths.shared_dir());
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let output = cmd.output().await.map_err(|e| {
                MubError::Launch(format!(
                    "Failed to run NeoForge installer with '{resolved_java}': {e}"
                ))
            })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Err(MubError::Launch(format!(
                    "NeoForge installer exited with {}: {}\n{}",
                    output.status,
                    stderr.trim(),
                    stdout.trim()
                )));
            }
            if !nf_json_path.exists() {
                return Err(MubError::Launch(format!(
                    "NeoForge installer finished but {} was not created",
                    nf_json_path.display()
                )));
            }
        }

        on_progress(format!("Loading NeoForge {loader_version} metadata..."));
        let nf_meta: NeoForgeVersionJson =
            serde_json::from_str(&std::fs::read_to_string(&nf_json_path)?)?;

        let mut nf_cp = Vec::new();
        let mut download_tasks = Vec::new();
        for lib in &nf_meta.libraries {
            if !rules_allow(&lib.rules) || is_wrong_arch_native_coord(&lib.name) {
                continue;
            }
            if let Some(downloads) = &lib.downloads
                && let Some(artifact) = &downloads.artifact
            {
                let rel = artifact
                    .path
                    .clone()
                    .unwrap_or_else(|| maven_to_path(&lib.name));
                let dest = paths.libraries_dir().join(&rel);
                nf_cp.push(dest.clone());
                if !artifact.url.is_empty() {
                    download_tasks.push((artifact.url.clone(), dest, artifact.sha1.clone()));
                }
            } else {
                let rel = maven_to_path(&lib.name);
                let dest = paths.libraries_dir().join(&rel);
                nf_cp.push(dest.clone());
                if let Some(base) = &lib.url
                    && !base.trim().is_empty()
                {
                    let url = format!("{}/{rel}", base.trim_end_matches('/'));
                    download_tasks.push((url, dest, None));
                }
            }
        }

        if !download_tasks.is_empty() {
            self.run_download_batch(download_tasks, max_concurrent, &on_progress)
                .await?;
        }

        nf_cp.extend(std::mem::take(&mut prepared.classpath));
        prepared.classpath = dedup_paths(nf_cp);
        prepared.main_class = nf_meta.main_class;
        prepared.version_id = nf_meta.id.clone();

        if let Some(args) = nf_meta.arguments {
            prepared.extra_jvm_args.extend(collect_allowed_args(
                &args.jvm,
                paths,
                &nf_meta.id,
                natives_dir,
            ));
            prepared.extra_game_args.extend(collect_allowed_args(
                &args.game,
                paths,
                &nf_meta.id,
                natives_dir,
            ));
        }

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

        atomic_write_file(dest, &bytes)?;
        Ok(())
    }
}

#[derive(Clone)]
struct CachedVanilla {
    meta: VersionMeta,
    classpath: Vec<PathBuf>,
    native_jars: Vec<PathBuf>,
    client_jar: PathBuf,
    verified_at: std::time::Instant,
}

const VANILLA_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

fn vanilla_cache() -> &'static std::sync::Mutex<HashMap<(PathBuf, String), CachedVanilla>> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<(PathBuf, String), CachedVanilla>>> =
        OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn get_cached_vanilla(key: &(PathBuf, String)) -> Option<CachedVanilla> {
    let map = vanilla_cache().lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get(key)?;
    if entry.verified_at.elapsed() <= VANILLA_CACHE_TTL {
        Some(entry.clone())
    } else {
        None
    }
}

fn put_cached_vanilla(key: (PathBuf, String), value: CachedVanilla) {
    let mut map = vanilla_cache().lock().unwrap_or_else(|e| e.into_inner());
    map.insert(key, value);
}

fn shared_prep_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let map = LOCKS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(key.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn atomic_write_file(dest: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = dest.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    match std::fs::rename(&tmp, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            if dest.exists() {
                let _ = std::fs::remove_file(dest);
                if std::fs::rename(&tmp, dest).is_ok() {
                    return Ok(());
                }
            }
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

fn verify_sha1(path: &Path, expected: &str) -> Result<bool> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    Ok(actual.eq_ignore_ascii_case(expected))
}

fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn arch_matches(arch: &str) -> bool {
    let lower = arch.trim().to_ascii_lowercase();
    if cfg!(target_arch = "x86_64") {
        matches!(lower.as_str(), "x86_64" | "amd64" | "x64")
    } else if cfg!(target_arch = "x86") {
        matches!(lower.as_str(), "x86" | "i386" | "i686")
    } else if cfg!(target_arch = "aarch64") {
        matches!(lower.as_str(), "arm64" | "aarch64")
    } else {
        false
    }
}

fn rules_allow(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let current_os = current_os_name();

    let mut allowed = false;
    for rule in rules {
        if let Some(features) = &rule.features
            && !features.is_empty()
        {
            continue;
        }
        let os_matches = match &rule.os {
            Some(os) => {
                let name_ok = os.name.as_deref().is_none_or(|n| n == current_os);
                let arch_ok = os.arch.as_deref().is_none_or(arch_matches);
                name_ok && arch_ok
            }
            None => true,
        };
        if os_matches {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn extract_coord_classifier(coord: &str) -> Option<&str> {
    let core = coord.split('@').next().unwrap_or(coord);
    let mut parts = core.split(':');
    let _g = parts.next()?;
    let _a = parts.next()?;
    let _v = parts.next()?;
    parts.next()
}

fn native_classifier_os_and_arch(classifier: &str) -> Option<(&'static str, Option<&'static str>)> {
    let lower = classifier.to_ascii_lowercase();
    let (os, rest) = if let Some(r) = lower.strip_prefix("natives-windows") {
        ("windows", r)
    } else if let Some(r) = lower.strip_prefix("natives-macos") {
        ("osx", r)
    } else if let Some(r) = lower.strip_prefix("natives-osx") {
        ("osx", r)
    } else {
        let r = lower.strip_prefix("natives-linux")?;
        ("linux", r)
    };

    let arch = if rest.is_empty() {
        None
    } else if matches!(rest, "-x86_64" | "-amd64" | "-x64" | "-64") {
        Some("x86_64")
    } else if matches!(rest, "-x86" | "-i386" | "-i686" | "-32") {
        Some("x86")
    } else if matches!(rest, "-arm64" | "-aarch64") {
        Some("aarch64")
    } else {
        Some("other")
    };
    Some((os, arch))
}

fn native_classifier_matches_current(classifier: &str) -> bool {
    let Some((os, arch)) = native_classifier_os_and_arch(classifier) else {
        return false;
    };
    if os != current_os_name() {
        return false;
    }
    match arch {
        Some("x86_64") => cfg!(target_arch = "x86_64"),
        Some("x86") => cfg!(target_arch = "x86"),
        Some("aarch64") => cfg!(target_arch = "aarch64"),
        Some(_) => false,
        None => !cfg!(target_arch = "aarch64") || os == "osx",
    }
}

fn is_wrong_arch_native_coord(coord: &str) -> bool {
    let Some(classifier) = extract_coord_classifier(coord) else {
        return false;
    };
    if native_classifier_os_and_arch(classifier).is_none() {
        return false;
    }
    !native_classifier_matches_current(classifier)
}

fn is_current_arch_native_coord(coord: &str) -> bool {
    extract_coord_classifier(coord).is_some_and(native_classifier_matches_current)
}

fn select_native_classifier<'a>(
    lib: &Library,
    classifiers: &'a HashMap<String, DownloadArtifact>,
) -> Option<&'a DownloadArtifact> {
    if classifiers.is_empty() {
        return None;
    }
    let arch_bits = if cfg!(target_pointer_width = "64") {
        "64"
    } else {
        "32"
    };

    if let Some(natives_map) = &lib.natives
        && let Some(tmpl) = natives_map.get(current_os_name())
    {
        let resolved = tmpl.replace("${arch}", arch_bits);
        if let Some(art) = classifiers.get(&resolved) {
            return Some(art);
        }
    }

    let candidates: &[&str] = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            &["natives-windows-arm64", "natives-windows-aarch64"]
        } else if cfg!(target_arch = "x86") {
            &["natives-windows-x86", "natives-windows-32", "natives-windows"]
        } else {
            &["natives-windows-x86_64", "natives-windows-64", "natives-windows"]
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            &[
                "natives-macos-arm64",
                "natives-osx-arm64",
                "natives-macos",
                "natives-osx",
            ]
        } else {
            &[
                "natives-macos-x86_64",
                "natives-osx-x86_64",
                "natives-macos",
                "natives-osx",
            ]
        }
    } else if cfg!(target_arch = "aarch64") {
        &["natives-linux-arm64", "natives-linux-aarch64"]
    } else if cfg!(target_arch = "x86") {
        &["natives-linux-x86", "natives-linux-32", "natives-linux"]
    } else {
        &["natives-linux-x86_64", "natives-linux-64", "natives-linux"]
    };

    for key in candidates {
        if let Some(art) = classifiers.get(*key) {
            return Some(art);
        }
    }
    None
}

fn dedup_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}

fn collect_allowed_args(
    items: &[serde_json::Value],
    paths: &AppPaths,
    version_name: &str,
    natives_dir: &Path,
) -> Vec<String> {
    let lib_dir = paths.libraries_dir().to_string_lossy().into_owned();
    let cp_sep = if cfg!(windows) { ";" } else { ":" };
    let natives_str = natives_dir.to_string_lossy().into_owned();
    let replace_vars = |s: &str| -> String {
        s.replace("${library_directory}", &lib_dir)
            .replace("${classpath_separator}", cp_sep)
            .replace("${version_name}", version_name)
            .replace("${natives_directory}", &natives_str)
            .replace("${launcher_name}", "ModsUndBots")
            .replace("${launcher_version}", "0.2.0")
    };

    let mut out = Vec::new();
    for item in items {
        match item {
            serde_json::Value::String(s) => out.push(replace_vars(s)),
            serde_json::Value::Object(map) => {
                let rules_ok = map
                    .get("rules")
                    .and_then(|v| serde_json::from_value::<Vec<Rule>>(v.clone()).ok())
                    .is_none_or(|r| rules_allow(&r));
                if !rules_ok {
                    continue;
                }
                match map.get("value") {
                    Some(serde_json::Value::String(s)) => out.push(replace_vars(s)),
                    Some(serde_json::Value::Array(arr)) => {
                        for v in arr {
                            if let Some(s) = v.as_str() {
                                out.push(replace_vars(s));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn neoforge_mc_prefix(mc_version: &str) -> String {
    let core = mc_version.trim().split(['-', '+']).next().unwrap_or(mc_version);
    let mut parts = core.split('.');
    let _major = parts.next().unwrap_or("1");
    let minor = parts.next().unwrap_or("0");
    let patch = parts.next().unwrap_or("0");
    format!("{minor}.{patch}.")
}

fn parse_maven_metadata_versions(xml: &str, prefix: &str) -> Vec<LoaderVersionInfo> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        let Some(end) = after.find("</version>") else {
            break;
        };
        let ver = after[..end].trim();
        if ver.starts_with(prefix) {
            let stable = !ver.contains("beta") && !ver.contains("alpha");
            out.push(LoaderVersionInfo {
                version: ver.to_string(),
                stable,
            });
        }
        rest = &after[end + "</version>".len()..];
    }
    out
}

fn compare_neoforge_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> (Vec<u64>, bool) {
        let is_beta = s.contains("beta") || s.contains("alpha");
        let core = s.split('-').next().unwrap_or(s);
        let nums = core
            .split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect();
        (nums, !is_beta)
    };
    let (na, sa) = parse(a);
    let (nb, sb) = parse(b);
    na.cmp(&nb).then_with(|| sa.cmp(&sb)).then_with(|| a.cmp(b))
}

fn maven_to_path(coord: &str) -> String {
    let (base_coord, ext) = match coord.split_once('@') {
        Some((c, e)) if !e.is_empty() => (c, e),
        _ => (coord, "jar"),
    };
    let parts: Vec<&str> = base_coord.split(':').collect();
    if parts.len() < 3 {
        return coord.to_string();
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    if parts.len() >= 4 && !parts[3].is_empty() {
        let classifier = parts[3];
        format!("{group}/{artifact}/{version}/{artifact}-{version}-{classifier}.{ext}")
    } else {
        format!("{group}/{artifact}/{version}/{artifact}-{version}.{ext}")
    }
}

fn collect_vanilla_jvm_args(
    items: &[serde_json::Value],
    paths: &AppPaths,
    version_name: &str,
    natives_dir: &Path,
) -> Vec<String> {
    let raw = collect_allowed_args(items, paths, version_name, natives_dir);
    let mut out = Vec::with_capacity(raw.len());
    let mut iter = raw.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "-cp" || arg == "-classpath" {
            // Skip `-cp` and its `${classpath}` value; launcher.rs supplies the merged classpath.
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("-Djava.library.path=")
            || arg.starts_with("-Dminecraft.launcher.brand=")
            || arg.starts_with("-Dminecraft.launcher.version=")
        {
            continue;
        }
        out.push(arg);
    }
    out
}

fn ensure_isolated_native_jvm_args(extra_jvm_args: &mut Vec<String>, natives_dir: &Path) -> Result<()> {
    let jna_dir = natives_dir.join("jna");
    let lwjgl_dir = natives_dir.join("lwjgl");
    let netty_dir = natives_dir.join("netty");
    std::fs::create_dir_all(&jna_dir)?;
    std::fs::create_dir_all(&lwjgl_dir)?;
    std::fs::create_dir_all(&netty_dir)?;

    let defaults = [
        ("-Djna.tmpdir=", jna_dir),
        ("-Dorg.lwjgl.system.SharedLibraryExtractPath=", lwjgl_dir),
        ("-Dio.netty.native.workdir=", netty_dir),
    ];
    for (prefix, dir) in defaults {
        if !extra_jvm_args.iter().any(|a| a.starts_with(prefix)) {
            extra_jvm_args.push(format!("{prefix}{}", dir.display()));
        }
    }
    Ok(())
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
        if let Ok(meta) = std::fs::metadata(&out_path)
            && meta.len() == entry.size()
        {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut buf)?;
        atomic_write_file(&out_path, &buf)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_jvm_args_preserves_critical_flags_and_filters_managed() {
        let tmp = std::env::temp_dir().join(format!("mub_mojang_test_{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&tmp);
        let natives_dir = paths.natives_dir("prof-1");

        let jvm_json: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
                "-XX:StackShadowPages=32",
                "--enable-native-access=ALL-UNNAMED",
                "--add-exports",
                "java.base/jdk.internal.misc=ALL-UNNAMED",
                "-Djava.library.path=${natives_directory}/java",
                "-Djna.tmpdir=${natives_directory}/jna",
                "-Dorg.lwjgl.system.SharedLibraryExtractPath=${natives_directory}/lwjgl",
                "-Dio.netty.native.workdir=${natives_directory}/netty",
                "-Dminecraft.launcher.brand=${launcher_name}",
                "-Dminecraft.launcher.version=${launcher_version}",
                "-cp",
                "${classpath}"
            ]"#,
        )
        .unwrap();

        let mut args = collect_vanilla_jvm_args(&jvm_json, &paths, "26.3", &natives_dir);
        ensure_isolated_native_jvm_args(&mut args, &natives_dir).unwrap();

        assert!(args.contains(&"-XX:StackShadowPages=32".to_string()));
        assert!(args.contains(&"--enable-native-access=ALL-UNNAMED".to_string()));
        assert!(args.contains(&"--add-exports".to_string()));
        assert!(args.contains(&"java.base/jdk.internal.misc=ALL-UNNAMED".to_string()));
        assert!(args.iter().any(|a| a.starts_with("-Djna.tmpdir=")));
        assert!(args
            .iter()
            .any(|a| a.starts_with("-Dorg.lwjgl.system.SharedLibraryExtractPath=")));
        assert!(args
            .iter()
            .any(|a| a.starts_with("-Dio.netty.native.workdir=")));

        assert!(!args.iter().any(|a| a == "-cp" || a == "${classpath}"));
        assert!(!args.iter().any(|a| a.starts_with("-Djava.library.path=")));
        assert!(!args
            .iter()
            .any(|a| a.starts_with("-Dminecraft.launcher.brand=")));
        assert!(!args
            .iter()
            .any(|a| a.starts_with("-Dminecraft.launcher.version=")));

        assert!(natives_dir.join("jna").is_dir());
        assert!(natives_dir.join("lwjgl").is_dir());
        assert!(natives_dir.join("netty").is_dir());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn atomic_write_overwrites_cleanly() {
        let tmp = std::env::temp_dir().join(format!("mub_atomic_{}", uuid::Uuid::new_v4()));
        let file = tmp.join("nested").join("data.bin");
        atomic_write_file(&file, b"first").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"first");
        atomic_write_file(&file, b"second-version").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second-version");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
