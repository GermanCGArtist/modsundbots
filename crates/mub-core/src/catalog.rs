use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::curseforge::{
    self, CLASS_DATAPACKS, CLASS_MODPACKS, CLASS_MODS, CLASS_RESOURCEPACKS, CLASS_SHADERS,
    CfFile, CfMod, CurseForgeClient, compute_murmur2_fingerprint,
};
use crate::error::{MubError, Result};
use crate::github::{
    DEFAULT_GITHUB_OWNER, FEATURED_GITHUB_REPOS, GhRelease, GhRepo, GitHubClient,
    extract_mc_versions_and_loaders, is_relevant_asset, parse_repo_spec,
};
use crate::modrinth::{ModrinthClient, ModrinthEnvironment, ModrinthSort, ModrinthVersion};
use crate::paths::AppPaths;
use crate::profile::ModLoader;

const CONTENT_MANIFEST_FILE: &str = ".mub-content.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentSource {
    Modrinth,
    CurseForge,
    GitHub,
}

impl std::fmt::Display for ContentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentSource::Modrinth => write!(f, "Modrinth"),
            ContentSource::CurseForge => write!(f, "CurseForge"),
            ContentSource::GitHub => write!(f, "GitHub"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFilter {
    All,
    Modrinth,
    CurseForge,
    GitHub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Mod,
    Modpack,
    ResourcePack,
    Shader,
    DataPack,
}

impl ContentKind {
    pub fn modrinth_project_type(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::Modpack => "modpack",
            ContentKind::ResourcePack => "resourcepack",
            ContentKind::Shader => "shader",
            ContentKind::DataPack => "datapack",
        }
    }

    pub fn curseforge_class_id(&self) -> u32 {
        match self {
            ContentKind::Mod => CLASS_MODS,
            ContentKind::Modpack => CLASS_MODPACKS,
            ContentKind::ResourcePack => CLASS_RESOURCEPACKS,
            ContentKind::Shader => CLASS_SHADERS,
            ContentKind::DataPack => CLASS_DATAPACKS,
        }
    }

    pub fn dir_for_profile(&self, paths: &AppPaths, profile_id: &str) -> PathBuf {
        match self {
            ContentKind::Mod => paths.mods_dir(profile_id),
            ContentKind::ResourcePack => paths.resourcepacks_dir(profile_id),
            ContentKind::Shader => paths.shaderpacks_dir(profile_id),
            ContentKind::DataPack => paths.datapacks_dir(profile_id),
            ContentKind::Modpack => paths.instance_dir(profile_id),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ContentKind::Mod => "Mods",
            ContentKind::Modpack => "Modpacks",
            ContentKind::ResourcePack => "Resource Packs",
            ContentKind::Shader => "Shaders",
            ContentKind::DataPack => "Data Packs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSort {
    Downloads,
    Relevance,
    Updated,
    Newest,
}

impl CatalogSort {
    fn to_modrinth(self) -> ModrinthSort {
        match self {
            CatalogSort::Downloads => ModrinthSort::Downloads,
            CatalogSort::Relevance => ModrinthSort::Relevance,
            CatalogSort::Updated => ModrinthSort::Updated,
            CatalogSort::Newest => ModrinthSort::Newest,
        }
    }

    fn to_curseforge_field(self) -> u32 {
        match self {
            CatalogSort::Downloads => 6,
            CatalogSort::Relevance => 2,
            CatalogSort::Updated => 3,
            CatalogSort::Newest => 11,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogHit {
    pub modrinth_id: Option<String>,
    pub curseforge_id: Option<u64>,
    #[serde(default)]
    pub github_repo: Option<String>,
    pub sources: Vec<ContentSource>,
    pub kind: ContentKind,
    pub title: String,
    pub slug: String,
    pub author: String,
    pub description: String,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSearchResponse {
    pub hits: Vec<CatalogHit>,
    pub total_hits: u64,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogGalleryItem {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogDetail {
    pub modrinth_id: Option<String>,
    pub curseforge_id: Option<u64>,
    #[serde(default)]
    pub github_repo: Option<String>,
    pub sources: Vec<ContentSource>,
    pub kind: ContentKind,
    pub title: String,
    pub slug: String,
    pub author: String,
    pub description: String,
    pub body: String,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    pub gallery: Vec<CatalogGalleryItem>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub issues_url: Option<String>,
    pub discord_url: Option<String>,
    pub website_url: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogVersion {
    pub id: String,
    pub project_id: String,
    pub source: ContentSource,
    pub name: String,
    pub version_number: String,
    pub version_type: String,
    pub date_published: String,
    pub downloads: u64,
    pub changelog: Option<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub dependencies: Vec<CatalogDependency>,
    pub files: Vec<CatalogFile>,
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub required: bool,
    pub source: ContentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogFile {
    pub url: Option<String>,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
    pub sha1: Option<String>,
    pub murmur2: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledContentMeta {
    pub source: ContentSource,
    pub kind: ContentKind,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub murmur2: Option<u32>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledContentItem {
    pub filename: String,
    pub kind: ContentKind,
    pub enabled: bool,
    pub size_bytes: u64,
    pub path: PathBuf,
    pub meta: Option<InstalledContentMeta>,
    pub sha1: Option<String>,
    pub murmur2: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentUpdateInfo {
    pub filename: String,
    pub kind: ContentKind,
    pub current_version: String,
    pub latest_version: CatalogVersion,
    pub source: ContentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfo {
    pub folder_name: String,
    pub path: PathBuf,
    pub datapacks_dir: PathBuf,
    pub datapack_count: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ContentManifest {
    #[serde(default)]
    items: Vec<InstalledContentMeta>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct LegacyModManifest {
    #[serde(default)]
    mods: HashMap<String, crate::modrinth::InstalledModEntry>,
}

#[derive(Clone)]
pub struct CatalogClient {
    modrinth: ModrinthClient,
    curseforge: Option<CurseForgeClient>,
    github: GitHubClient,
}

impl CatalogClient {
    pub fn new(curseforge_api_key: Option<String>) -> Self {
        let gh_token = std::env::var("MUB_GITHUB_TOKEN")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok());
        Self::with_github(curseforge_api_key, gh_token, Vec::new())
    }

    pub fn with_github(
        curseforge_api_key: Option<String>,
        github_token: Option<String>,
        github_mod_repos: Vec<String>,
    ) -> Self {
        let curseforge = curseforge_api_key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .map(CurseForgeClient::new);
        Self {
            modrinth: ModrinthClient::new(),
            curseforge,
            github: GitHubClient::new(github_token, github_mod_repos),
        }
    }

    pub fn has_curseforge(&self) -> bool {
        self.curseforge.is_some()
    }

    pub fn modrinth(&self) -> &ModrinthClient {
        &self.modrinth
    }

    pub fn curseforge(&self) -> Option<&CurseForgeClient> {
        self.curseforge.as_ref()
    }

    pub fn github(&self) -> &GitHubClient {
        &self.github
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        &self,
        source_filter: SourceFilter,
        kind: ContentKind,
        query: &str,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
        categories: &[String],
        sort: CatalogSort,
        offset: u32,
        limit: u32,
    ) -> Result<CatalogSearchResponse> {
        let effective_loader = match kind {
            ContentKind::Mod | ContentKind::Modpack => loader,
            _ => None,
        };

        match source_filter {
            SourceFilter::Modrinth => {
                let resp = self
                    .modrinth
                    .search_projects_advanced(
                        kind.modrinth_project_type(),
                        query,
                        mc_version,
                        effective_loader,
                        categories,
                        ModrinthEnvironment::Any,
                        sort.to_modrinth(),
                        offset,
                        limit,
                    )
                    .await?;
                let hits = resp
                    .hits
                    .into_iter()
                    .map(|h| modrinth_hit_to_catalog(h, kind))
                    .collect();
                Ok(CatalogSearchResponse {
                    hits,
                    total_hits: resp.total_hits,
                    offset: resp.offset,
                    limit: resp.limit,
                })
            }
            SourceFilter::CurseForge => {
                let cf = self.curseforge.as_ref().ok_or_else(|| {
                    MubError::CurseForgeAuth("CurseForge API key is not configured".into())
                })?;
                let loader_id = match kind {
                    ContentKind::Mod => effective_loader.and_then(|l| l.curseforge_loader_id()),
                    _ => None,
                };
                let resp = cf
                    .search_mods(
                        kind.curseforge_class_id(),
                        query,
                        mc_version,
                        loader_id,
                        sort.to_curseforge_field(),
                        offset,
                        limit,
                    )
                    .await?;
                let hits = resp
                    .data
                    .into_iter()
                    .map(|m| cf_mod_to_catalog_hit(m, kind))
                    .collect();
                Ok(CatalogSearchResponse {
                    hits,
                    total_hits: resp.pagination.total_count,
                    offset: resp.pagination.index,
                    limit: resp.pagination.page_size,
                })
            }
            SourceFilter::GitHub => {
                self.search_github(kind, query, offset, limit).await
            }
            SourceFilter::All => {
                let gh_fut = async {
                    if query.trim().is_empty() {
                        Vec::new()
                    } else {
                        self.search_github_matching_query(kind, query).await
                    }
                };

                let Some(cf) = &self.curseforge else {
                    let mr_fut = self.modrinth.search_projects_advanced(
                        kind.modrinth_project_type(),
                        query,
                        mc_version,
                        effective_loader,
                        categories,
                        ModrinthEnvironment::Any,
                        sort.to_modrinth(),
                        offset,
                        limit,
                    );
                    let (mr_res, gh_hits) = tokio::join!(mr_fut, gh_fut);
                    let resp = mr_res?;
                    let mut hits: Vec<CatalogHit> = if offset == 0 { gh_hits } else { Vec::new() };
                    let extra = hits.len() as u64;
                    hits.extend(resp.hits.into_iter().map(|h| modrinth_hit_to_catalog(h, kind)));
                    return Ok(CatalogSearchResponse {
                        hits,
                        total_hits: resp.total_hits.saturating_add(extra),
                        offset: resp.offset,
                        limit: resp.limit,
                    });
                };

                let loader_id = match kind {
                    ContentKind::Mod => effective_loader.and_then(|l| l.curseforge_loader_id()),
                    _ => None,
                };

                let mr_fut = self.modrinth.search_projects_advanced(
                    kind.modrinth_project_type(),
                    query,
                    mc_version,
                    effective_loader,
                    categories,
                    ModrinthEnvironment::Any,
                    sort.to_modrinth(),
                    offset,
                    limit,
                );
                let cf_fut = cf.search_mods(
                    kind.curseforge_class_id(),
                    query,
                    mc_version,
                    loader_id,
                    sort.to_curseforge_field(),
                    offset,
                    limit,
                );

                let (mr_res, cf_res, gh_hits) = tokio::join!(mr_fut, cf_fut, gh_fut);

                let (mr_hits, mr_total) = match mr_res {
                    Ok(r) => (
                        r.hits
                            .into_iter()
                            .map(|h| modrinth_hit_to_catalog(h, kind))
                            .collect::<Vec<_>>(),
                        r.total_hits,
                    ),
                    Err(e) => {
                        tracing::warn!("Modrinth search failed during unified search: {e}");
                        (Vec::new(), 0)
                    }
                };

                let (cf_hits, cf_total) = match cf_res {
                    Ok(r) => (
                        r.data
                            .into_iter()
                            .map(|m| cf_mod_to_catalog_hit(m, kind))
                            .collect::<Vec<_>>(),
                        r.pagination.total_count,
                    ),
                    Err(e) => {
                        tracing::warn!("CurseForge search failed during unified search: {e}");
                        if mr_hits.is_empty() && gh_hits.is_empty() {
                            return Err(e);
                        }
                        (Vec::new(), 0)
                    }
                };

                let mut merged = if offset == 0 { gh_hits } else { Vec::new() };
                let extra = merged.len() as u64;
                merged.extend(merge_catalog_hits(mr_hits, cf_hits, sort));
                Ok(CatalogSearchResponse {
                    hits: merged,
                    total_hits: mr_total.saturating_add(cf_total).saturating_add(extra),
                    offset,
                    limit,
                })
            }
        }
    }

    pub async fn get_detail(&self, hit: &CatalogHit) -> Result<CatalogDetail> {
        match (&hit.modrinth_id, hit.curseforge_id, &hit.github_repo) {
            (Some(mr_id), Some(cf_id), _) => {
                let mut detail = self
                    .get_detail_by_id(ContentSource::Modrinth, mr_id, hit.kind)
                    .await?;
                detail.curseforge_id = Some(cf_id);
                if !detail.sources.contains(&ContentSource::CurseForge) {
                    detail.sources.push(ContentSource::CurseForge);
                }
                Ok(detail)
            }
            (Some(mr_id), None, _) => {
                self.get_detail_by_id(ContentSource::Modrinth, mr_id, hit.kind)
                    .await
            }
            (None, Some(cf_id), _) => {
                self.get_detail_by_id(ContentSource::CurseForge, &cf_id.to_string(), hit.kind)
                    .await
            }
            (None, None, Some(gh_repo)) => {
                self.get_detail_by_id(ContentSource::GitHub, gh_repo, hit.kind)
                    .await
            }
            (None, None, None) => Err(MubError::Download(
                "CatalogHit has no Modrinth, CurseForge, or GitHub ID".into(),
            )),
        }
    }

    pub async fn get_detail_by_id(
        &self,
        source: ContentSource,
        id: &str,
        kind: ContentKind,
    ) -> Result<CatalogDetail> {
        match source {
            ContentSource::Modrinth => {
                let proj = self.modrinth.get_project(id).await?;
                let slug = proj.slug.clone();
                let website_url = Some(modrinth_website_url(kind, &slug));
                Ok(CatalogDetail {
                    modrinth_id: Some(proj.id),
                    curseforge_id: None,
                    github_repo: None,
                    sources: vec![ContentSource::Modrinth],
                    kind,
                    title: proj.title,
                    slug,
                    author: String::new(),
                    description: proj.description,
                    body: proj.body,
                    downloads: proj.downloads,
                    follows: proj.followers,
                    icon_url: proj.icon_url,
                    categories: proj.categories,
                    gallery: proj
                        .gallery
                        .into_iter()
                        .map(|g| CatalogGalleryItem {
                            url: g.url,
                            title: g.title,
                            description: g.description,
                        })
                        .collect(),
                    source_url: proj.source_url,
                    wiki_url: proj.wiki_url,
                    issues_url: proj.issues_url,
                    discord_url: proj.discord_url,
                    website_url,
                    license: proj.license.map(|l| l.name.unwrap_or(l.id)),
                })
            }
            ContentSource::CurseForge => {
                let cf = self.curseforge.as_ref().ok_or_else(|| {
                    MubError::CurseForgeAuth("CurseForge API key is not configured".into())
                })?;
                let mod_id: u64 = id.parse().map_err(|_| {
                    MubError::Download(format!("Invalid CurseForge mod ID '{id}'"))
                })?;
                let (mod_res, desc_res) =
                    tokio::join!(cf.get_mod(mod_id), cf.get_mod_description(mod_id));
                let m = mod_res?;
                let body = desc_res.unwrap_or_default();
                let author = m
                    .authors
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let links = m.links.as_ref();
                Ok(CatalogDetail {
                    modrinth_id: None,
                    curseforge_id: Some(m.id),
                    github_repo: None,
                    sources: vec![ContentSource::CurseForge],
                    kind,
                    title: m.name,
                    slug: m.slug,
                    author,
                    description: m.summary,
                    body,
                    downloads: m.download_count,
                    follows: m.thumbs_up_count,
                    icon_url: m.logo.and_then(|l| l.thumbnail_url.or(l.url)),
                    categories: m.categories.into_iter().map(|c| c.name).collect(),
                    gallery: m
                        .screenshots
                        .into_iter()
                        .filter_map(|s| {
                            s.url.or(s.thumbnail_url).map(|url| CatalogGalleryItem {
                                url,
                                title: s.title,
                                description: s.description,
                            })
                        })
                        .collect(),
                    source_url: links.and_then(|l| l.source_url.clone()),
                    wiki_url: links.and_then(|l| l.wiki_url.clone()),
                    issues_url: links.and_then(|l| l.issues_url.clone()),
                    discord_url: None,
                    website_url: links.and_then(|l| l.website_url.clone()).or_else(|| {
                        Some(format!("https://www.curseforge.com/projects/{mod_id}"))
                    }),
                    license: None,
                })
            }
            ContentSource::GitHub => {
                let (owner, repo) = parse_repo_spec(id).ok_or_else(|| {
                    MubError::Download(format!("Invalid GitHub repository '{id}' (expected owner/repo)"))
                })?;
                let (repo_res, readme_res) =
                    tokio::join!(self.github.get_repo(&owner, &repo), self.github.get_readme(&owner, &repo));
                let gh = repo_res?;
                let readme = readme_res.ok().flatten();
                let desc = gh.description.clone().unwrap_or_default();
                let body = readme.unwrap_or_else(|| desc.clone());
                let issues_url = Some(format!("{}/issues", gh.html_url));
                Ok(CatalogDetail {
                    modrinth_id: None,
                    curseforge_id: None,
                    github_repo: Some(gh.full_name.clone()),
                    sources: vec![ContentSource::GitHub],
                    kind,
                    title: gh.name,
                    slug: gh.full_name,
                    author: gh.owner.login,
                    description: desc,
                    body,
                    downloads: 0,
                    follows: gh.stargazers_count,
                    icon_url: gh.owner.avatar_url,
                    categories: gh.topics,
                    gallery: Vec::new(),
                    source_url: Some(gh.html_url.clone()),
                    wiki_url: None,
                    issues_url,
                    discord_url: None,
                    website_url: Some(gh.html_url),
                    license: gh.license.and_then(|l| l.name.or(l.spdx_id)),
                })
            }
        }
    }

    pub async fn list_versions(
        &self,
        source: ContentSource,
        project_id: &str,
        kind: ContentKind,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
    ) -> Result<Vec<CatalogVersion>> {
        match source {
            ContentSource::Modrinth => {
                let effective_loader = match kind {
                    ContentKind::Mod | ContentKind::Modpack => loader,
                    _ => None,
                };
                let versions = self
                    .modrinth
                    .list_project_versions(project_id, mc_version, effective_loader)
                    .await?;
                Ok(versions
                    .into_iter()
                    .map(modrinth_version_to_catalog)
                    .collect())
            }
            ContentSource::CurseForge => {
                let cf = self.curseforge.as_ref().ok_or_else(|| {
                    MubError::CurseForgeAuth("CurseForge API key is not configured".into())
                })?;
                let mod_id: u64 = project_id.parse().map_err(|_| {
                    MubError::Download(format!("Invalid CurseForge mod ID '{project_id}'"))
                })?;
                let loader_id = match kind {
                    ContentKind::Mod => loader.and_then(|l| l.curseforge_loader_id()),
                    _ => None,
                };
                let files = cf
                    .get_mod_files(mod_id, mc_version, loader_id, 0, 50)
                    .await?;
                Ok(files
                    .into_iter()
                    .map(|f| cf_file_to_catalog_version(f, None))
                    .collect())
            }
            ContentSource::GitHub => {
                let (owner, repo) = parse_repo_spec(project_id).ok_or_else(|| {
                    MubError::Download(format!(
                        "Invalid GitHub repository '{project_id}' (expected owner/repo)"
                    ))
                })?;
                let full_name = format!("{owner}/{repo}");
                let releases = self.github.list_releases(&owner, &repo, 30).await?;
                let all_versions: Vec<CatalogVersion> = releases
                    .into_iter()
                    .filter_map(|rel| {
                        gh_release_to_catalog_version(
                            &self.github,
                            &full_name,
                            rel,
                            kind,
                            mc_version,
                            loader,
                        )
                    })
                    .collect();

                let loader_slug = match kind {
                    ContentKind::Mod | ContentKind::Modpack => loader.and_then(|l| l.slug()),
                    _ => None,
                };
                let filtered: Vec<CatalogVersion> = all_versions
                    .iter()
                    .filter(|v| {
                        let mc_ok = match mc_version {
                            Some(mc) if !v.game_versions.is_empty() => {
                                v.game_versions.iter().any(|gv| gv == mc)
                            }
                            _ => true,
                        };
                        let loader_ok = match loader_slug {
                            Some(ls) if !v.loaders.is_empty() => {
                                v.loaders.iter().any(|l| l.eq_ignore_ascii_case(ls))
                            }
                            _ => true,
                        };
                        mc_ok && loader_ok
                    })
                    .cloned()
                    .collect();

                if filtered.is_empty() {
                    Ok(all_versions)
                } else {
                    Ok(filtered)
                }
            }
        }
    }

    /// Install a `CatalogVersion` AND its required dependencies into the target
    /// content directory (`mods/`, `resourcepacks/`, `shaderpacks/`, `datapacks/`)
    /// for each profile in `profile_ids`.
    #[allow(clippy::too_many_arguments)]
    pub async fn install_with_dependencies(
        &self,
        paths: &AppPaths,
        profile_ids: &[String],
        kind: ContentKind,
        root_version: &CatalogVersion,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
    ) -> Result<Vec<String>> {
        let mut installed_files = Vec::new();
        let mut visited: HashSet<(ContentSource, String)> = HashSet::new();
        let mut queue = vec![root_version.clone()];

        while let Some(ver) = queue.pop() {
            if !visited.insert((ver.source, ver.project_id.clone())) {
                continue;
            }
            let fname = self
                .install_single_version(paths, profile_ids, kind, &ver)
                .await?;
            installed_files.push(fname);

            for dep in &ver.dependencies {
                if !dep.required {
                    continue;
                }
                match dep.source {
                    ContentSource::Modrinth => {
                        if let Some(dep_vid) = &dep.version_id {
                            if let Ok(mr_ver) = self.modrinth.get_version(dep_vid).await
                                && !visited.contains(&(ContentSource::Modrinth, mr_ver.project_id.clone()))
                            {
                                queue.push(modrinth_version_to_catalog(mr_ver));
                            }
                        } else if let Some(dep_pid) = &dep.project_id
                            && !visited.contains(&(ContentSource::Modrinth, dep_pid.clone()))
                            && let Ok(vers) = self
                                .list_versions(
                                    ContentSource::Modrinth,
                                    dep_pid,
                                    kind,
                                    mc_version,
                                    loader,
                                )
                                .await
                            && let Some(latest) = vers.into_iter().next()
                        {
                            queue.push(latest);
                        }
                    }
                    ContentSource::CurseForge => {
                        if let Some(dep_pid) = &dep.project_id
                            && !visited.contains(&(ContentSource::CurseForge, dep_pid.clone()))
                            && let Ok(vers) = self
                                .list_versions(
                                    ContentSource::CurseForge,
                                    dep_pid,
                                    kind,
                                    mc_version,
                                    loader,
                                )
                                .await
                            && let Some(latest) = vers.into_iter().next()
                        {
                            queue.push(latest);
                        }
                    }
                    ContentSource::GitHub => {}
                }
            }
        }

        Ok(installed_files)
    }

    async fn install_single_version(
        &self,
        paths: &AppPaths,
        profile_ids: &[String],
        kind: ContentKind,
        version: &CatalogVersion,
    ) -> Result<String> {
        let file = version
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| version.files.first())
            .ok_or_else(|| {
                MubError::Download(format!(
                    "Version '{}' has no downloadable files",
                    version.version_number
                ))
            })?;

        let download_url = self.resolve_file_download_url(version, file).await?;
        let bytes = match version.source {
            ContentSource::Modrinth => self.modrinth.download_bytes(&download_url).await?,
            ContentSource::CurseForge => {
                let cf = self.curseforge.as_ref().ok_or_else(|| {
                    MubError::CurseForgeAuth("CurseForge API key is not configured".into())
                })?;
                cf.download_bytes(&download_url).await?
            }
            ContentSource::GitHub => self.github.download_bytes(&download_url).await?,
        };

        verify_downloaded_bytes(&bytes, file)?;
        let actual_sha1 = sha1_hex(&bytes);
        let actual_murmur2 = compute_murmur2_fingerprint(&bytes);

        for pid in profile_ids {
            let target_dir = kind.dir_for_profile(paths, pid);
            std::fs::create_dir_all(&target_dir)?;
            std::fs::write(target_dir.join(&file.filename), &bytes)?;

            let meta = InstalledContentMeta {
                source: version.source,
                kind,
                project_id: version.project_id.clone(),
                version_id: version.id.clone(),
                version_number: version.version_number.clone(),
                filename: file.filename.clone(),
                sha1: Some(actual_sha1.clone()),
                murmur2: Some(actual_murmur2),
                download_url: Some(download_url.clone()),
                title: Some(version.name.clone()),
                icon_url: None,
            };
            upsert_content_meta(paths, pid, meta)?;
        }

        Ok(file.filename.clone())
    }

    /// Install a datapack version directly into a specific saved world's `datapacks/` folder.
    pub async fn install_datapack_to_world(
        &self,
        paths: &AppPaths,
        profile_id: &str,
        world_folder: &str,
        version: &CatalogVersion,
    ) -> Result<String> {
        let file = version
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| version.files.first())
            .ok_or_else(|| {
                MubError::Download(format!(
                    "Datapack version '{}' has no files",
                    version.version_number
                ))
            })?;
        let download_url = self.resolve_file_download_url(version, file).await?;
        let bytes = match version.source {
            ContentSource::Modrinth => self.modrinth.download_bytes(&download_url).await?,
            ContentSource::CurseForge => {
                let cf = self.curseforge.as_ref().ok_or_else(|| {
                    MubError::CurseForgeAuth("CurseForge API key is not configured".into())
                })?;
                cf.download_bytes(&download_url).await?
            }
            ContentSource::GitHub => self.github.download_bytes(&download_url).await?,
        };
        verify_downloaded_bytes(&bytes, file)?;
        let dp_dir = paths
            .saves_dir(profile_id)
            .join(world_folder)
            .join("datapacks");
        std::fs::create_dir_all(&dp_dir)?;
        std::fs::write(dp_dir.join(&file.filename), &bytes)?;
        Ok(file.filename.clone())
    }

    async fn resolve_file_download_url(
        &self,
        version: &CatalogVersion,
        file: &CatalogFile,
    ) -> Result<String> {
        if let Some(url) = &file.url
            && !url.trim().is_empty()
        {
            return Ok(url.clone());
        }
        if version.source == ContentSource::CurseForge
            && let Some(cf) = &self.curseforge
            && let Ok(mod_id) = version.project_id.parse::<u64>()
            && let Ok(file_id) = version.id.parse::<u64>()
            && let Some(url) = cf.get_download_url(mod_id, file_id).await?
        {
            return Ok(url);
        }

        let website_url = version.website_url.clone().unwrap_or_else(|| {
            match version.source {
                ContentSource::CurseForge => {
                    format!("https://www.curseforge.com/projects/{}", version.project_id)
                }
                ContentSource::Modrinth => {
                    format!("https://modrinth.com/project/{}", version.project_id)
                }
                ContentSource::GitHub => {
                    format!("https://github.com/{}/releases", version.project_id)
                }
            }
        });
        Err(MubError::CurseForgeDistributionRestricted {
            filename: file.filename.clone(),
            website_url,
        })
    }

    /// Identify installed content files via Modrinth SHA-1 and CurseForge Murmur2
    /// fingerprints, and check for available updates matching `mc_version` + `loader`.
    pub async fn identify_and_check_updates(
        &self,
        paths: &AppPaths,
        profile_id: &str,
        kind: ContentKind,
        mc_version: &str,
        loader: &ModLoader,
    ) -> Result<Vec<ContentUpdateInfo>> {
        let items = list_installed_content(paths, profile_id, kind)?;
        if items.is_empty() {
            return Ok(Vec::new());
        }

        // Compute hashes for all installed files
        let mut sha1_to_item: HashMap<String, &InstalledContentItem> = HashMap::new();
        let mut murmur_to_item: HashMap<u32, &InstalledContentItem> = HashMap::new();
        let mut all_sha1s = Vec::new();
        let mut all_murmurs = Vec::new();

        for item in &items {
            if !item.path.is_file() {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&item.path) {
                let s1 = sha1_hex(&bytes);
                let mm = compute_murmur2_fingerprint(&bytes);
                all_sha1s.push(s1.clone());
                all_murmurs.push(mm);
                sha1_to_item.insert(s1, item);
                murmur_to_item.insert(mm, item);
            }
        }

        let mut updates = Vec::new();
        let mut matched_filenames: HashSet<String> = HashSet::new();

        // 1. Modrinth SHA-1 identification + update check
        if !all_sha1s.is_empty() {
            let mr_loaders = modrinth_loaders_for_kind(kind, loader);
            let current_map = self
                .modrinth
                .get_versions_from_hashes(&all_sha1s)
                .await
                .unwrap_or_default();
            let update_map = self
                .modrinth
                .get_updates_from_hashes(&all_sha1s, mc_version, &mr_loaders)
                .await
                .unwrap_or_default();

            for (hash, latest_mr) in update_map {
                let Some(item) = sha1_to_item.get(&hash) else {
                    continue;
                };
                matched_filenames.insert(item.filename.clone());
                let cur_ver = current_map.get(&hash);
                let cur_id = cur_ver
                    .map(|v| v.id.as_str())
                    .or_else(|| item.meta.as_ref().map(|m| m.version_id.as_str()));
                let cur_num = cur_ver
                    .map(|v| v.version_number.clone())
                    .or_else(|| item.meta.as_ref().map(|m| m.version_number.clone()))
                    .unwrap_or_else(|| "unknown".to_string());

                if cur_id != Some(latest_mr.id.as_str()) {
                    updates.push(ContentUpdateInfo {
                        filename: item.filename.clone(),
                        kind,
                        current_version: cur_num,
                        latest_version: modrinth_version_to_catalog(latest_mr),
                        source: ContentSource::Modrinth,
                    });
                }
            }
        }

        // 2. CurseForge Murmur2 identification + update check for remaining items
        if let Some(cf) = &self.curseforge {
            let remaining_murmurs: Vec<u32> = murmur_to_item
                .iter()
                .filter(|(_, item)| !matched_filenames.contains(&item.filename))
                .map(|(&mm, _)| mm)
                .collect();

            if !remaining_murmurs.is_empty()
                && let Ok(fp_res) = cf.match_fingerprints(&remaining_murmurs).await
            {
                let loader_id = match kind {
                    ContentKind::Mod => loader.curseforge_loader_id(),
                    _ => None,
                };
                for m in fp_res.exact_matches {
                    let mm = m.file.file_fingerprint as u32;
                    let Some(item) = murmur_to_item.get(&mm) else {
                        continue;
                    };
                    if matched_filenames.contains(&item.filename) {
                        continue;
                    }
                    // Find newest compatible file in m.latest_files
                    let candidate = select_latest_cf_file(&m.latest_files, mc_version, loader_id).cloned();
                    if let Some(latest) = candidate
                        && latest.id != m.file.id
                    {
                        matched_filenames.insert(item.filename.clone());
                        updates.push(ContentUpdateInfo {
                            filename: item.filename.clone(),
                            kind,
                            current_version: m.file.display_name.clone(),
                            latest_version: cf_file_to_catalog_version(latest, None),
                            source: ContentSource::CurseForge,
                        });
                    }
                }
            }
        }

        // 3. GitHub release check for items installed from GitHub
        for item in &items {
            if matched_filenames.contains(&item.filename) {
                continue;
            }
            let Some(meta) = &item.meta else {
                continue;
            };
            if meta.source != ContentSource::GitHub {
                continue;
            }
            if let Ok(vers) = self
                .list_versions(
                    ContentSource::GitHub,
                    &meta.project_id,
                    kind,
                    Some(mc_version),
                    Some(loader),
                )
                .await
                && let Some(latest) = vers.into_iter().next()
                && latest.id != meta.version_id
                && latest.version_number != meta.version_number
            {
                updates.push(ContentUpdateInfo {
                    filename: item.filename.clone(),
                    kind,
                    current_version: meta.version_number.clone(),
                    latest_version: latest,
                    source: ContentSource::GitHub,
                });
            }
        }

        Ok(updates)
    }

    pub async fn update_content_item(
        &self,
        paths: &AppPaths,
        profile_id: &str,
        update: &ContentUpdateInfo,
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
    ) -> Result<String> {
        let was_disabled = update.filename.ends_with(".disabled");
        remove_installed_content(paths, profile_id, update.kind, &update.filename)?;
        let installed = self
            .install_with_dependencies(
                paths,
                &[profile_id.to_string()],
                update.kind,
                &update.latest_version,
                mc_version,
                loader,
            )
            .await?;
        let new_name = installed
            .into_iter()
            .next()
            .unwrap_or_else(|| update.filename.clone());
        if was_disabled {
            toggle_installed_content(paths, profile_id, update.kind, &new_name)?;
            return Ok(format!("{new_name}.disabled"));
        }
        Ok(new_name)
    }

    pub async fn update_all_content(
        &self,
        paths: &AppPaths,
        profile_id: &str,
        updates: &[ContentUpdateInfo],
        mc_version: Option<&str>,
        loader: Option<&ModLoader>,
    ) -> Result<Vec<String>> {
        let mut updated = Vec::with_capacity(updates.len());
        for u in updates {
            let fname = self
                .update_content_item(paths, profile_id, u, mc_version, loader)
                .await?;
            updated.push(fname);
        }
        Ok(updated)
    }

    async fn search_github(
        &self,
        kind: ContentKind,
        query: &str,
        offset: u32,
        limit: u32,
    ) -> Result<CatalogSearchResponse> {
        let q_trim = query.trim();
        let q_lower = q_trim.to_ascii_lowercase();

        let mut seen: HashSet<String> = HashSet::new();
        let mut ordered_specs: Vec<(String, String)> = Vec::new();

        // 1. Direct owner/repo, GitHub URL, or featured short name
        if let Some((owner, repo)) = parse_repo_spec(q_trim) {
            let key = format!("{owner}/{repo}").to_ascii_lowercase();
            if seen.insert(key) {
                ordered_specs.push((owner, repo));
            }
        }

        // 2. Priority featured repos (manhattan, vitruvius, heavymatica)
        for &spec in FEATURED_GITHUB_REPOS {
            if let Some((owner, repo)) = spec.split_once('/') {
                let key = spec.to_ascii_lowercase();
                if seen.insert(key) {
                    ordered_specs.push((owner.to_string(), repo.to_string()));
                }
            }
        }

        // 3. User-configured custom GitHub repos
        for spec in self.github.custom_repos() {
            if let Some((owner, repo)) = parse_repo_spec(spec) {
                let key = format!("{owner}/{repo}").to_ascii_lowercase();
                if seen.insert(key) {
                    ordered_specs.push((owner, repo));
                }
            }
        }

        let repo_futs = ordered_specs
            .iter()
            .map(|(owner, repo)| self.github.get_repo(owner, repo));
        let owner_repos_fut = self.github.list_owner_repos(DEFAULT_GITHUB_OWNER, 50);

        let (direct_results, owner_repos_res) =
            tokio::join!(futures_util::future::join_all(repo_futs), owner_repos_fut);

        let mut repos: Vec<GhRepo> = Vec::new();
        let mut added_full: HashSet<String> = HashSet::new();
        let mut last_err: Option<MubError> = None;

        for res in direct_results {
            match res {
                Ok(r) => {
                    let key = r.full_name.to_ascii_lowercase();
                    if added_full.insert(key) {
                        repos.push(r);
                    }
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        match owner_repos_res {
            Ok(list) => {
                for r in list {
                    if r.name.eq_ignore_ascii_case("modsundbots") {
                        continue;
                    }
                    let key = r.full_name.to_ascii_lowercase();
                    if added_full.insert(key) {
                        repos.push(r);
                    }
                }
            }
            Err(e) => {
                if repos.is_empty() {
                    return Err(last_err.unwrap_or(e));
                }
            }
        }

        if repos.is_empty()
            && let Some(err) = last_err
        {
            return Err(err);
        }

        let direct_spec_lower = parse_repo_spec(q_trim)
            .map(|(o, r)| format!("{o}/{r}").to_ascii_lowercase());

        let mut hits: Vec<CatalogHit> = repos
            .into_iter()
            .filter(|r| {
                if q_lower.is_empty() {
                    return true;
                }
                if direct_spec_lower
                    .as_deref()
                    .is_some_and(|ds| r.full_name.eq_ignore_ascii_case(ds))
                {
                    return true;
                }
                let desc_match = r
                    .description
                    .as_deref()
                    .is_some_and(|d| d.to_ascii_lowercase().contains(&q_lower));
                let topic_match = r
                    .topics
                    .iter()
                    .any(|t| t.to_ascii_lowercase().contains(&q_lower));
                r.full_name.to_ascii_lowercase().contains(&q_lower)
                    || r.name.to_ascii_lowercase().contains(&q_lower)
                    || desc_match
                    || topic_match
            })
            .map(|r| gh_repo_to_catalog_hit(r, kind))
            .collect();

        let total_hits = hits.len() as u64;
        let start = (offset as usize).min(hits.len());
        let end = start.saturating_add(limit as usize).min(hits.len());
        hits = hits.drain(start..end).collect();

        Ok(CatalogSearchResponse {
            hits,
            total_hits,
            offset,
            limit,
        })
    }

    async fn search_github_matching_query(&self, kind: ContentKind, query: &str) -> Vec<CatalogHit> {
        let q_trim = query.trim();
        let q_lower = q_trim.to_ascii_lowercase();
        if q_lower.is_empty() {
            return Vec::new();
        }

        let mut specs: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        if let Some((owner, repo)) = parse_repo_spec(q_trim) {
            let key = format!("{owner}/{repo}").to_ascii_lowercase();
            if seen.insert(key) {
                specs.push((owner, repo));
            }
        }

        for spec in FEATURED_GITHUB_REPOS
            .iter()
            .copied()
            .chain(self.github.custom_repos().iter().map(String::as_str))
        {
            if let Some((owner, repo)) = parse_repo_spec(spec)
                && (repo.to_ascii_lowercase().contains(&q_lower)
                    || spec.to_ascii_lowercase().contains(&q_lower))
            {
                let key = format!("{owner}/{repo}").to_ascii_lowercase();
                if seen.insert(key) {
                    specs.push((owner, repo));
                }
            }
        }

        if specs.is_empty() {
            return Vec::new();
        }

        let futs = specs
            .iter()
            .map(|(owner, repo)| self.github.get_repo(owner, repo));
        futures_util::future::join_all(futs)
            .await
            .into_iter()
            .filter_map(|res| res.ok())
            .map(|r| gh_repo_to_catalog_hit(r, kind))
            .collect()
    }
}

pub fn content_manifest_path(paths: &AppPaths, profile_id: &str) -> PathBuf {
    paths.instance_dir(profile_id).join(CONTENT_MANIFEST_FILE)
}

fn load_content_manifest(paths: &AppPaths, profile_id: &str) -> ContentManifest {
    let path = content_manifest_path(paths, profile_id);
    if let Ok(raw) = std::fs::read_to_string(&path)
        && let Ok(m) = serde_json::from_str::<ContentManifest>(&raw)
    {
        return m;
    }
    ContentManifest::default()
}

fn save_content_manifest(
    paths: &AppPaths,
    profile_id: &str,
    manifest: &ContentManifest,
) -> Result<()> {
    let path = content_manifest_path(paths, profile_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(manifest)?)?;
    Ok(())
}

pub fn upsert_content_meta(
    paths: &AppPaths,
    profile_id: &str,
    meta: InstalledContentMeta,
) -> Result<()> {
    let mut manifest = load_content_manifest(paths, profile_id);
    let base_new = base_filename(&meta.filename);
    manifest.items.retain(|existing| {
        !(existing.kind == meta.kind
            && (base_filename(&existing.filename).eq_ignore_ascii_case(base_new)
                || (existing.source == meta.source && existing.project_id == meta.project_id)))
    });
    manifest.items.push(meta);
    save_content_manifest(paths, profile_id, &manifest)
}

fn load_legacy_mod_metas(paths: &AppPaths, profile_id: &str) -> Vec<InstalledContentMeta> {
    let legacy_path = paths.mods_dir(profile_id).join(".mub-mods.json");
    let Ok(raw) = std::fs::read_to_string(&legacy_path) else {
        return Vec::new();
    };
    let Ok(legacy) = serde_json::from_str::<LegacyModManifest>(&raw) else {
        return Vec::new();
    };
    legacy
        .mods
        .into_values()
        .map(|entry| InstalledContentMeta {
            source: ContentSource::Modrinth,
            kind: ContentKind::Mod,
            project_id: entry.project_id,
            version_id: entry.version_id,
            version_number: entry.version_number,
            filename: entry.filename,
            sha1: None,
            murmur2: None,
            download_url: None,
            title: None,
            icon_url: None,
        })
        .collect()
}

pub fn list_installed_content(
    paths: &AppPaths,
    profile_id: &str,
    kind: ContentKind,
) -> Result<Vec<InstalledContentItem>> {
    let dir = kind.dir_for_profile(paths, profile_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let manifest = load_content_manifest(paths, profile_id);
    let mut metas: Vec<InstalledContentMeta> = manifest
        .items
        .into_iter()
        .filter(|m| m.kind == kind)
        .collect();

    if kind == ContentKind::Mod {
        for legacy in load_legacy_mod_metas(paths, profile_id) {
            let base = base_filename(&legacy.filename);
            if !metas
                .iter()
                .any(|m| base_filename(&m.filename).eq_ignore_ascii_case(base))
            {
                metas.push(legacy);
            }
        }
    }

    let mut items = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if filename.starts_with('.') {
            continue;
        }
        let lower = filename.to_lowercase();
        let enabled = match kind {
            ContentKind::Mod => {
                if lower.ends_with(".jar") {
                    true
                } else if lower.ends_with(".jar.disabled") {
                    false
                } else {
                    continue;
                }
            }
            ContentKind::ResourcePack | ContentKind::Shader | ContentKind::DataPack => {
                if lower.ends_with(".zip") {
                    true
                } else if lower.ends_with(".zip.disabled") {
                    false
                } else {
                    continue;
                }
            }
            ContentKind::Modpack => continue,
        };

        let size_bytes = entry.metadata()?.len();
        let base = base_filename(filename);
        let meta = metas
            .iter()
            .find(|m| base_filename(&m.filename).eq_ignore_ascii_case(base))
            .cloned();

        items.push(InstalledContentItem {
            filename: filename.to_string(),
            kind,
            enabled,
            size_bytes,
            path,
            sha1: meta.as_ref().and_then(|m| m.sha1.clone()),
            murmur2: meta.as_ref().and_then(|m| m.murmur2),
            meta,
        });
    }

    items.sort_by_key(|a| a.filename.to_lowercase());
    Ok(items)
}

pub fn toggle_installed_content(
    paths: &AppPaths,
    profile_id: &str,
    kind: ContentKind,
    filename: &str,
) -> Result<()> {
    let dir = kind.dir_for_profile(paths, profile_id);
    let src = dir.join(filename);
    if !src.exists() {
        return Ok(());
    }
    let new_name = if let Some(stripped) = filename.strip_suffix(".disabled") {
        stripped.to_string()
    } else {
        format!("{filename}.disabled")
    };
    std::fs::rename(&src, dir.join(&new_name))?;

    let mut manifest = load_content_manifest(paths, profile_id);
    let base = base_filename(filename);
    let mut changed = false;
    for item in &mut manifest.items {
        if item.kind == kind && base_filename(&item.filename).eq_ignore_ascii_case(base) {
            item.filename = new_name.clone();
            changed = true;
        }
    }
    if changed {
        save_content_manifest(paths, profile_id, &manifest)?;
    }
    Ok(())
}

pub fn remove_installed_content(
    paths: &AppPaths,
    profile_id: &str,
    kind: ContentKind,
    filename: &str,
) -> Result<()> {
    let dir = kind.dir_for_profile(paths, profile_id);
    let target = dir.join(filename);
    if target.exists() {
        std::fs::remove_file(&target)?;
    }

    let mut manifest = load_content_manifest(paths, profile_id);
    let before = manifest.items.len();
    let base = base_filename(filename);
    manifest
        .items
        .retain(|m| !(m.kind == kind && base_filename(&m.filename).eq_ignore_ascii_case(base)));
    if manifest.items.len() != before {
        save_content_manifest(paths, profile_id, &manifest)?;
    }
    Ok(())
}

pub fn list_saved_worlds(paths: &AppPaths, profile_id: &str) -> Result<Vec<WorldInfo>> {
    let saves_dir = paths.saves_dir(profile_id);
    if !saves_dir.exists() {
        return Ok(Vec::new());
    }
    let mut worlds = Vec::new();
    for entry in std::fs::read_dir(&saves_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(folder_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if folder_name.starts_with('.') {
            continue;
        }
        let datapacks_dir = path.join("datapacks");
        let datapack_count = if datapacks_dir.exists() {
            std::fs::read_dir(&datapacks_dir)?
                .flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.eq_ignore_ascii_case("zip"))
                })
                .count()
        } else {
            0
        };
        worlds.push(WorldInfo {
            folder_name: folder_name.to_string(),
            path,
            datapacks_dir,
            datapack_count,
        });
    }
    worlds.sort_by(|a, b| {
        a.folder_name
            .to_lowercase()
            .cmp(&b.folder_name.to_lowercase())
    });
    Ok(worlds)
}

/// Copy all content files of `kind` and their manifest entries from `source_profile_id`
/// into each `target_profile_ids`. Returns the total number of files copied.
pub fn sync_content_to_profiles(
    paths: &AppPaths,
    source_profile_id: &str,
    target_profile_ids: &[String],
    kind: ContentKind,
) -> Result<usize> {
    let items = list_installed_content(paths, source_profile_id, kind)?;
    let mut copied = 0usize;

    for target_pid in target_profile_ids {
        if target_pid == source_profile_id {
            continue;
        }
        let dest_dir = kind.dir_for_profile(paths, target_pid);
        std::fs::create_dir_all(&dest_dir)?;
        for item in &items {
            std::fs::copy(&item.path, dest_dir.join(&item.filename))?;
            if let Some(meta) = &item.meta {
                upsert_content_meta(paths, target_pid, meta.clone())?;
            }
            copied += 1;
        }
    }
    Ok(copied)
}

pub(crate) fn list_all_tracked_meta(
    paths: &AppPaths,
    profile_id: &str,
) -> Vec<InstalledContentMeta> {
    let mut items = load_content_manifest(paths, profile_id).items;
    for legacy in load_legacy_mod_metas(paths, profile_id) {
        let base = base_filename(&legacy.filename);
        if !items
            .iter()
            .any(|m| m.kind == ContentKind::Mod && base_filename(&m.filename).eq_ignore_ascii_case(base))
        {
            items.push(legacy);
        }
    }
    items
}

fn base_filename(filename: &str) -> &str {
    filename.strip_suffix(".disabled").unwrap_or(filename)
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn verify_downloaded_bytes(bytes: &[u8], file: &CatalogFile) -> Result<()> {
    if let Some(expected_sha1) = &file.sha1 {
        let actual = sha1_hex(bytes);
        if !actual.eq_ignore_ascii_case(expected_sha1) {
            return Err(MubError::Download(format!(
                "SHA-1 mismatch for '{}': expected {expected_sha1}, got {actual}",
                file.filename
            )));
        }
    }
    if let Some(expected_mm) = file.murmur2 {
        let actual = compute_murmur2_fingerprint(bytes);
        if actual != expected_mm {
            return Err(MubError::Download(format!(
                "Murmur2 fingerprint mismatch for '{}': expected {expected_mm}, got {actual}",
                file.filename
            )));
        }
    }
    Ok(())
}

fn modrinth_website_url(kind: ContentKind, slug: &str) -> String {
    let seg = match kind {
        ContentKind::Mod => "mod",
        ContentKind::Modpack => "modpack",
        ContentKind::ResourcePack => "resourcepack",
        ContentKind::Shader => "shader",
        ContentKind::DataPack => "datapack",
    };
    format!("https://modrinth.com/{seg}/{slug}")
}

fn modrinth_hit_to_catalog(hit: crate::modrinth::ModrinthHit, kind: ContentKind) -> CatalogHit {
    let slug = hit.slug.clone();
    CatalogHit {
        modrinth_id: Some(hit.project_id),
        curseforge_id: None,
        github_repo: None,
        sources: vec![ContentSource::Modrinth],
        kind,
        title: hit.title,
        slug: slug.clone(),
        author: hit.author,
        description: hit.description,
        downloads: hit.downloads,
        follows: hit.follows,
        icon_url: hit.icon_url,
        categories: hit.categories,
        website_url: Some(modrinth_website_url(kind, &slug)),
    }
}

fn cf_mod_to_catalog_hit(m: CfMod, kind: ContentKind) -> CatalogHit {
    let author = m
        .authors
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    CatalogHit {
        modrinth_id: None,
        curseforge_id: Some(m.id),
        github_repo: None,
        sources: vec![ContentSource::CurseForge],
        kind,
        title: m.name,
        slug: m.slug,
        author,
        description: m.summary,
        downloads: m.download_count,
        follows: m.thumbs_up_count,
        icon_url: m.logo.and_then(|l| l.thumbnail_url.or(l.url)),
        categories: m.categories.into_iter().map(|c| c.name).collect(),
        website_url: m
            .links
            .and_then(|l| l.website_url)
            .or_else(|| Some(format!("https://www.curseforge.com/projects/{}", m.id))),
    }
}

fn gh_repo_to_catalog_hit(r: GhRepo, kind: ContentKind) -> CatalogHit {
    let mut categories = r.topics.clone();
    if r.private && !categories.iter().any(|c| c == "private") {
        categories.push("private".to_string());
    }
    CatalogHit {
        modrinth_id: None,
        curseforge_id: None,
        github_repo: Some(r.full_name.clone()),
        sources: vec![ContentSource::GitHub],
        kind,
        title: r.name,
        slug: r.full_name,
        author: r.owner.login,
        description: r
            .description
            .unwrap_or_else(|| "GitHub repository release".to_string()),
        downloads: 0,
        follows: r.stargazers_count,
        icon_url: r.owner.avatar_url,
        categories,
        website_url: Some(r.html_url),
    }
}

fn gh_release_to_catalog_version(
    gh: &GitHubClient,
    full_name: &str,
    rel: GhRelease,
    kind: ContentKind,
    mc_version: Option<&str>,
    loader: Option<&ModLoader>,
) -> Option<CatalogVersion> {
    let relevant: Vec<_> = rel
        .assets
        .iter()
        .filter(|a| is_relevant_asset(&a.name, kind))
        .collect();
    if relevant.is_empty() {
        return None;
    }

    let loader_slug = match kind {
        ContentKind::Mod | ContentKind::Modpack => loader.and_then(|l| l.slug()),
        _ => None,
    };

    // Score each asset so the one matching mc_version + loader becomes `primary: true`
    let best_idx = relevant
        .iter()
        .enumerate()
        .max_by_key(|(_, asset)| {
            let (asset_mcs, asset_loaders) = extract_mc_versions_and_loaders(&asset.name);
            let mut score = 0i32;
            if let Some(mc) = mc_version
                && asset_mcs.iter().any(|m| m == mc)
            {
                score += 2;
            }
            if let Some(ls) = loader_slug
                && asset_loaders.iter().any(|l| l.eq_ignore_ascii_case(ls))
            {
                score += 2;
            }
            score
        })
        .map(|(i, _)| i)
        .unwrap_or(0);

    let mut combined_text = format!(
        "{} {}",
        rel.tag_name,
        rel.name.as_deref().unwrap_or_default()
    );
    for a in &relevant {
        combined_text.push(' ');
        combined_text.push_str(&a.name);
    }
    let (game_versions, loaders) = extract_mc_versions_and_loaders(&combined_text);

    let total_downloads: u64 = relevant.iter().map(|a| a.download_count).sum();
    let files: Vec<CatalogFile> = relevant
        .into_iter()
        .enumerate()
        .map(|(idx, asset)| CatalogFile {
            url: Some(gh.asset_download_url(asset)),
            filename: asset.name.clone(),
            primary: idx == best_idx,
            size: asset.size,
            sha1: None,
            murmur2: None,
        })
        .collect();

    let display_name = rel
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| rel.tag_name.clone());

    Some(CatalogVersion {
        id: rel.tag_name.clone(),
        project_id: full_name.to_string(),
        source: ContentSource::GitHub,
        name: display_name,
        version_number: rel.tag_name,
        version_type: if rel.prerelease {
            "beta".to_string()
        } else {
            "release".to_string()
        },
        date_published: rel.published_at.unwrap_or_default(),
        downloads: total_downloads,
        changelog: rel.body,
        game_versions,
        loaders,
        dependencies: Vec::new(),
        files,
        website_url: Some(rel.html_url),
    })
}

fn modrinth_version_to_catalog(v: ModrinthVersion) -> CatalogVersion {
    CatalogVersion {
        id: v.id,
        project_id: v.project_id,
        source: ContentSource::Modrinth,
        name: v.name,
        version_number: v.version_number,
        version_type: v.version_type,
        date_published: v.date_published,
        downloads: v.downloads,
        changelog: v.changelog,
        game_versions: v.game_versions,
        loaders: v.loaders,
        dependencies: v
            .dependencies
            .into_iter()
            .map(|d| CatalogDependency {
                project_id: d.project_id,
                version_id: d.version_id,
                required: d.dependency_type == "required",
                source: ContentSource::Modrinth,
            })
            .collect(),
        files: v
            .files
            .into_iter()
            .map(|f| CatalogFile {
                url: Some(f.url),
                filename: f.filename,
                primary: f.primary,
                size: f.size,
                sha1: f.hashes.get("sha1").cloned(),
                murmur2: None,
            })
            .collect(),
        website_url: None,
    }
}

fn cf_file_to_catalog_version(f: CfFile, mod_website_url: Option<&str>) -> CatalogVersion {
    let version_type = f.release_type_str().to_string();
    let sha1 = f.sha1_hash().map(ToString::to_string);
    let murmur2 = u32::try_from(f.file_fingerprint).ok();
    let version_number = f.display_name.clone();
    let website_url = Some(match mod_website_url {
        Some(base) => format!("{}/files/{}", base.trim_end_matches('/'), f.id),
        None => format!("https://www.curseforge.com/projects/{}/files/{}", f.mod_id, f.id),
    });

    CatalogVersion {
        id: f.id.to_string(),
        project_id: f.mod_id.to_string(),
        source: ContentSource::CurseForge,
        name: f.display_name,
        version_number,
        version_type,
        date_published: f.file_date,
        downloads: f.download_count,
        changelog: None,
        game_versions: f.game_versions.clone(),
        loaders: extract_cf_loaders(&f.game_versions),
        dependencies: f
            .dependencies
            .into_iter()
            .map(|d| CatalogDependency {
                project_id: Some(d.mod_id.to_string()),
                version_id: None,
                required: d.relation_type == 3,
                source: ContentSource::CurseForge,
            })
            .collect(),
        files: vec![CatalogFile {
            url: f.download_url,
            filename: f.file_name,
            primary: true,
            size: f.file_length,
            sha1,
            murmur2,
        }],
        website_url,
    }
}

fn extract_cf_loaders(game_versions: &[String]) -> Vec<String> {
    let known = ["Fabric", "NeoForge", "Forge", "Quilt"];
    game_versions
        .iter()
        .filter(|gv| known.iter().any(|k| gv.eq_ignore_ascii_case(k)))
        .map(|gv| gv.to_lowercase())
        .collect()
}

fn select_latest_cf_file<'a>(
    files: &'a [CfFile],
    mc_version: &str,
    loader_id: Option<u32>,
) -> Option<&'a CfFile> {
    let loader_name = match loader_id {
        Some(1) => Some("forge"),
        Some(4) => Some("fabric"),
        Some(5) => Some("quilt"),
        Some(6) => Some("neoforge"),
        _ => None,
    };

    files
        .iter()
        .filter(|f| {
            let mc_ok = f.game_versions.iter().any(|gv| gv == mc_version);
            let loader_ok = match loader_name {
                Some(ln) => f
                    .game_versions
                    .iter()
                    .any(|gv| gv.eq_ignore_ascii_case(ln)),
                None => true,
            };
            mc_ok && loader_ok
        })
        .max_by_key(|f| f.id)
}

fn modrinth_loaders_for_kind(kind: ContentKind, loader: &ModLoader) -> Vec<String> {
    match kind {
        ContentKind::Mod | ContentKind::Modpack => loader
            .slug()
            .map(|s| vec![s.to_string()])
            .unwrap_or_else(|| vec!["minecraft".to_string()]),
        ContentKind::ResourcePack => vec!["minecraft".to_string()],
        ContentKind::Shader => vec![
            "iris".to_string(),
            "optifine".to_string(),
            "canvas".to_string(),
        ],
        ContentKind::DataPack => vec!["datapack".to_string()],
    }
}

fn normalize_title(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn merge_catalog_hits(
    mr_hits: Vec<CatalogHit>,
    cf_hits: Vec<CatalogHit>,
    sort: CatalogSort,
) -> Vec<CatalogHit> {
    let mut merged: Vec<CatalogHit> = mr_hits;
    let mut slug_idx: HashMap<String, usize> = HashMap::new();
    let mut title_idx: HashMap<String, usize> = HashMap::new();

    for (i, hit) in merged.iter().enumerate() {
        slug_idx.insert(hit.slug.to_lowercase(), i);
        let nt = normalize_title(&hit.title);
        if !nt.is_empty() {
            title_idx.insert(nt, i);
        }
    }

    let mut unmatched_cf = Vec::new();
    for cf in cf_hits {
        let cf_slug = cf.slug.to_lowercase();
        let cf_title = normalize_title(&cf.title);
        let existing = slug_idx
            .get(&cf_slug)
            .copied()
            .or_else(|| title_idx.get(&cf_title).copied());

        if let Some(idx) = existing {
            let entry = &mut merged[idx];
            if entry.curseforge_id.is_none() {
                entry.curseforge_id = cf.curseforge_id;
            }
            if !entry.sources.contains(&ContentSource::CurseForge) {
                entry.sources.push(ContentSource::CurseForge);
            }
            entry.downloads = entry.downloads.saturating_add(cf.downloads);
            if entry.icon_url.is_none() {
                entry.icon_url = cf.icon_url;
            }
        } else {
            unmatched_cf.push(cf);
        }
    }

    if sort == CatalogSort::Downloads {
        merged.extend(unmatched_cf);
        merged.sort_by_key(|a| std::cmp::Reverse(a.downloads));
        return merged;
    }

    // Interleave Modrinth and CurseForge hits so both platforms are represented cleanly
    let mut out = Vec::with_capacity(merged.len() + unmatched_cf.len());
    let mut mr_iter = merged.into_iter();
    let mut cf_iter = unmatched_cf.into_iter();
    loop {
        match (mr_iter.next(), cf_iter.next()) {
            (Some(m), Some(c)) => {
                out.push(m);
                out.push(c);
            }
            (Some(m), None) => {
                out.push(m);
                out.extend(mr_iter);
                break;
            }
            (None, Some(c)) => {
                out.push(c);
                out.extend(cf_iter);
                break;
            }
            (None, None) => break,
        }
    }
    out
}

/// Ensure `curseforge` module constants stay cleanly integrated (compile-time sanity).
const _: u32 = curseforge::MC_GAME_ID;
