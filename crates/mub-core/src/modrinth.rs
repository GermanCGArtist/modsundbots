use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::{MubError, Result};
use crate::paths::AppPaths;
use crate::profile::ModLoader;

const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModrinthSort {
    Downloads,
    Relevance,
    Updated,
    Newest,
}

impl ModrinthSort {
    pub fn as_api_str(&self) -> &'static str {
        match self {
            ModrinthSort::Downloads => "downloads",
            ModrinthSort::Relevance => "relevance",
            ModrinthSort::Updated => "updated",
            ModrinthSort::Newest => "newest",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModrinthEnvironment {
    Any,
    Client,
    Server,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthSearchResponse {
    pub hits: Vec<ModrinthHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    #[serde(default)]
    pub follows: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthProject {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub body: String,
    pub downloads: u64,
    #[serde(default)]
    pub followers: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub gallery: Vec<ModrinthGalleryItem>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub issues_url: Option<String>,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthGalleryItem {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default = "default_version_type")]
    pub version_type: String,
    #[serde(default)]
    pub date_published: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<ModrinthDependency>,
    pub files: Vec<ModrinthFile>,
}

fn default_version_type() -> String {
    "release".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthDependency {
    pub version_id: Option<String>,
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModrinthFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
    #[serde(default)]
    pub hashes: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct InstalledMod {
    pub filename: String,
    pub enabled: bool,
    pub size_bytes: u64,
    pub path: PathBuf,
}

pub struct ModrinthClient {
    http: Client,
}

impl Default for ModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ModrinthClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent("ModsUndBots/0.1.0 (https://github.com/modsundbots)")
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { http }
    }

    pub fn loader_slug(loader: &ModLoader) -> Option<&'static str> {
        match loader {
            ModLoader::Vanilla => None,
            ModLoader::Fabric { .. } => Some("fabric"),
            ModLoader::NeoForge { .. } => Some("neoforge"),
        }
    }

    /// Search or browse Modrinth with full facet filtering and pagination.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_mods_advanced(
        &self,
        query: &str,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
        categories: &[String],
        environment: ModrinthEnvironment,
        sort: ModrinthSort,
        offset: u32,
        limit: u32,
    ) -> Result<ModrinthSearchResponse> {
        let mut facets = vec![r#"["project_type:mod"]"#.to_string()];
        if let Some(mc) = mc_version
            && !mc.trim().is_empty()
        {
            facets.push(format!(r#"["versions:{mc}"]"#));
        }
        if let Some(l) = loader.and_then(Self::loader_slug) {
            facets.push(format!(r#"["categories:{l}"]"#));
        }
        for cat in categories {
            facets.push(format!(r#"["categories:{cat}"]"#));
        }
        match environment {
            ModrinthEnvironment::Any => {}
            ModrinthEnvironment::Client => {
                facets.push(r#"["client_side:required","client_side:optional"]"#.to_string());
            }
            ModrinthEnvironment::Server => {
                facets.push(r#"["server_side:required","server_side:optional"]"#.to_string());
            }
        }
        let facets_json = format!("[{}]", facets.join(","));

        let resp = self
            .http
            .get(format!("{MODRINTH_API_BASE}/search"))
            .query(&[
                ("query", query),
                ("facets", facets_json.as_str()),
                ("index", sort.as_api_str()),
                ("offset", &offset.to_string()),
                ("limit", &limit.to_string()),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "Modrinth search failed: {text}"
            )));
        }

        Ok(resp.json().await?)
    }

    /// Convenience wrapper for simple searches.
    pub async fn search_mods(
        &self,
        query: &str,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
        sort: ModrinthSort,
        limit: u32,
    ) -> Result<Vec<ModrinthHit>> {
        let resp = self
            .search_mods_advanced(
                query,
                mc_version,
                loader,
                &[],
                ModrinthEnvironment::Any,
                sort,
                0,
                limit,
            )
            .await?;
        Ok(resp.hits)
    }

    /// Fetch full project details (body, gallery, links, followers).
    pub async fn get_project(&self, project_id_or_slug: &str) -> Result<ModrinthProject> {
        let url = format!("{MODRINTH_API_BASE}/project/{project_id_or_slug}");
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "Modrinth project fetch failed for '{project_id_or_slug}': {text}"
            )));
        }
        Ok(resp.json().await?)
    }

    /// List compatible versions for a Modrinth project ID or slug.
    pub async fn list_project_versions(
        &self,
        project_id_or_slug: &str,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
    ) -> Result<Vec<ModrinthVersion>> {
        let url = format!("{MODRINTH_API_BASE}/project/{project_id_or_slug}/version");
        let mut req = self.http.get(&url);

        if let Some(l) = loader.and_then(Self::loader_slug) {
            let loaders_json = format!(r#"["{l}"]"#);
            req = req.query(&[("loaders", loaders_json)]);
        }
        if let Some(mc) = mc_version
            && !mc.trim().is_empty()
        {
            let gv_json = format!(r#"["{mc}"]"#);
            req = req.query(&[("game_versions", gv_json)]);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "Modrinth version lookup failed for '{project_id_or_slug}': {text}"
            )));
        }

        Ok(resp.json().await?)
    }

    pub async fn get_version(&self, version_id: &str) -> Result<ModrinthVersion> {
        let url = format!("{MODRINTH_API_BASE}/version/{version_id}");
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "Modrinth version fetch failed for '{version_id}': {text}"
            )));
        }
        Ok(resp.json().await?)
    }

    /// Download and install a `ModrinthVersion` AND its required dependencies into `profile_ids`.
    /// Returns the list of installed filenames.
    pub async fn install_with_dependencies(
        &self,
        paths: &AppPaths,
        profile_ids: &[String],
        root_version: &ModrinthVersion,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
    ) -> Result<Vec<String>> {
        let mut installed_files = Vec::new();
        let mut visited_projects = HashSet::new();
        let mut queue = vec![root_version.clone()];

        while let Some(ver) = queue.pop() {
            if !visited_projects.insert(ver.project_id.clone()) {
                continue;
            }
            let fname = self
                .install_version_to_profiles(paths, profile_ids, &ver)
                .await?;
            installed_files.push(fname);

            for dep in &ver.dependencies {
                if dep.dependency_type != "required" {
                    continue;
                }
                if let Some(dep_vid) = &dep.version_id {
                    if let Ok(dep_ver) = self.get_version(dep_vid).await
                        && !visited_projects.contains(&dep_ver.project_id)
                    {
                        queue.push(dep_ver);
                    }
                } else if let Some(dep_pid) = &dep.project_id
                    && !visited_projects.contains(dep_pid)
                    && let Ok(vers) = self
                        .list_project_versions(dep_pid, mc_version, loader)
                        .await
                    && let Some(latest) = vers.into_iter().next()
                {
                    queue.push(latest);
                }
            }
        }

        Ok(installed_files)
    }

    /// Download and install a specific `ModrinthVersion` into `profile_ids`.
    pub async fn install_version_to_profiles(
        &self,
        paths: &AppPaths,
        profile_ids: &[String],
        version: &ModrinthVersion,
    ) -> Result<String> {
        let file = version
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| version.files.first())
            .ok_or_else(|| {
                MubError::Download(format!(
                    "Modrinth version '{}' has no downloadable files",
                    version.version_number
                ))
            })?;

        let resp = self.http.get(&file.url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "Failed to download mod '{}': {}",
                file.filename,
                resp.status()
            )));
        }

        let bytes = resp.bytes().await?;
        if let Some(expected_sha1) = file.hashes.get("sha1") {
            let mut hasher = Sha1::new();
            hasher.update(&bytes);
            let actual = hex::encode(hasher.finalize());
            if !actual.eq_ignore_ascii_case(expected_sha1) {
                return Err(MubError::Download(format!(
                    "SHA-1 mismatch for '{}': expected {expected_sha1}, got {actual}",
                    file.filename
                )));
            }
        }

        for pid in profile_ids {
            let mods_dir = paths.mods_dir(pid);
            std::fs::create_dir_all(&mods_dir)?;
            std::fs::write(mods_dir.join(&file.filename), &bytes)?;
            record_installed_mod(
                paths,
                pid,
                &version.project_id,
                &version.id,
                &version.version_number,
                &file.filename,
            )?;
        }

        Ok(file.filename.clone())
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InstalledModEntry {
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct InstalledModManifest {
    pub mods: HashMap<String, InstalledModEntry>,
}

fn manifest_path(paths: &AppPaths, profile_id: &str) -> PathBuf {
    paths.mods_dir(profile_id).join(".mub-mods.json")
}

fn record_installed_mod(
    paths: &AppPaths,
    profile_id: &str,
    project_id: &str,
    version_id: &str,
    version_number: &str,
    filename: &str,
) -> Result<()> {
    let path = manifest_path(paths, profile_id);
    let mut manifest: InstalledModManifest = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&path)?).unwrap_or_default()
    } else {
        InstalledModManifest::default()
    };
    manifest.mods.insert(
        project_id.to_string(),
        InstalledModEntry {
            project_id: project_id.to_string(),
            version_id: version_id.to_string(),
            version_number: version_number.to_string(),
            filename: filename.to_string(),
        },
    );
    std::fs::write(path, serde_json::to_string_pretty(&manifest)?)?;
    Ok(())
}

/// Returns the set of Modrinth `project_id`s currently installed (and present on disk) in `profile_id`.
pub fn list_installed_project_ids(paths: &AppPaths, profile_id: &str) -> HashSet<String> {
    let path = manifest_path(paths, profile_id);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HashSet::new();
    };
    let Ok(manifest) = serde_json::from_str::<InstalledModManifest>(&raw) else {
        return HashSet::new();
    };
    let mods_dir = paths.mods_dir(profile_id);
    manifest
        .mods
        .into_iter()
        .filter(|(_, entry)| {
            mods_dir.join(&entry.filename).exists()
                || mods_dir
                    .join(format!("{}.disabled", entry.filename))
                    .exists()
        })
        .map(|(pid, _)| pid)
        .collect()
}

pub fn list_installed_mods(paths: &AppPaths, profile_id: &str) -> Result<Vec<InstalledMod>> {
    let mods_dir = paths.mods_dir(profile_id);
    if !mods_dir.exists() {
        return Ok(Vec::new());
    }
    let mut mods = Vec::new();
    for entry in std::fs::read_dir(&mods_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = filename.to_lowercase();
        let enabled = if lower.ends_with(".jar") {
            true
        } else if lower.ends_with(".jar.disabled") {
            false
        } else {
            continue;
        };
        let size_bytes = entry.metadata()?.len();
        mods.push(InstalledMod {
            filename: filename.to_string(),
            enabled,
            size_bytes,
            path,
        });
    }
    mods.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));
    Ok(mods)
}

pub fn toggle_installed_mod(paths: &AppPaths, profile_id: &str, filename: &str) -> Result<()> {
    let mods_dir = paths.mods_dir(profile_id);
    let src = mods_dir.join(filename);
    if !src.exists() {
        return Ok(());
    }
    let new_name = if let Some(stripped) = filename.strip_suffix(".disabled") {
        stripped.to_string()
    } else {
        format!("{filename}.disabled")
    };
    std::fs::rename(src, mods_dir.join(new_name))?;
    Ok(())
}

pub fn remove_installed_mod(paths: &AppPaths, profile_id: &str, filename: &str) -> Result<()> {
    let target = paths.mods_dir(profile_id).join(filename);
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    Ok(())
}
