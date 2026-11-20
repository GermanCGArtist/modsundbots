use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::sync::Semaphore;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

use crate::catalog::{
    CatalogClient, CatalogVersion, ContentKind, ContentSource, InstalledContentMeta,
    list_all_tracked_meta, upsert_content_meta,
};
use crate::curseforge::{CfFile, CurseForgeClient, compute_murmur2_fingerprint};
use crate::error::{MubError, Result};
use crate::paths::AppPaths;
use crate::profile::{ModLoader, Profile, ProfileKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestrictedPackFile {
    pub project_id: u64,
    pub file_id: u64,
    pub filename: String,
    pub website_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModpackImportResult {
    pub profile: Profile,
    pub downloaded_files: usize,
    pub extracted_overrides: usize,
    pub restricted_files: Vec<RestrictedPackFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    pub path: String,
    #[serde(default)]
    pub hashes: HashMap<String, String>,
    #[serde(default)]
    pub env: Option<MrpackEnv>,
    #[serde(default)]
    pub downloads: Vec<String>,
    #[serde(default)]
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrpackEnv {
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeManifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub minecraft: CurseForgeManifestMc,
    #[serde(default)]
    pub files: Vec<CurseForgeManifestFile>,
    #[serde(default = "default_overrides_dir")]
    pub overrides: String,
}

fn default_overrides_dir() -> String {
    "overrides".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeManifestMc {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<CurseForgeManifestLoader>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CurseForgeManifestLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

/// Import a Modrinth `.mrpack` archive into a new `Profile`.
pub async fn import_mrpack<F>(
    paths: &AppPaths,
    mrpack_path: &Path,
    profile_name: Option<&str>,
    kind: ProfileKind,
    max_concurrent: usize,
    on_progress: F,
) -> Result<ModpackImportResult>
where
    F: Fn(String) + Send + Sync + 'static,
{
    paths.ensure_dirs()?;
    on_progress("Reading .mrpack index...".to_string());

    let (index, overrides_entries) = read_mrpack_archive(mrpack_path)?;
    let mc_version = index
        .dependencies
        .get("minecraft")
        .cloned()
        .ok_or_else(|| MubError::Modpack("modrinth.index.json missing 'minecraft' dependency".into()))?;
    let mod_loader = parse_mrpack_loader(&index.dependencies);
    let final_name = profile_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&index.name);

    let mut profile = Profile::new(final_name, kind, mc_version, mod_loader);
    profile.source_pack = Some(index.name.clone());

    let instance_dir = paths.instance_dir(&profile.id);
    std::fs::create_dir_all(&instance_dir)?;

    // Prepare concurrent download tasks with zip-slip protection
    let mut download_tasks = Vec::new();
    for file in &index.files {
        if let Some(env) = &file.env
            && env.client.as_deref() == Some("unsupported")
        {
            continue;
        }
        let rel = sanitize_rel_path(&file.path)?;
        let dest = instance_dir.join(rel);
        let url = file.downloads.first().cloned().ok_or_else(|| {
            MubError::Modpack(format!("No download URL for '{}'", file.path))
        })?;
        let sha1 = file.hashes.get("sha1").cloned();
        download_tasks.push((url, dest, sha1));
    }

    let downloaded_files = download_tasks.len();
    if downloaded_files > 0 {
        on_progress(format!("Downloading {downloaded_files} modpack files..."));
        run_concurrent_downloads(download_tasks, max_concurrent, &on_progress).await?;
    }

    // Write overrides and client-overrides (already ordered overrides first, client-overrides second)
    let extracted_overrides = overrides_entries.len();
    if extracted_overrides > 0 {
        on_progress(format!("Extracting {extracted_overrides} override files..."));
        for (rel, bytes) in overrides_entries {
            let dest = instance_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(dest, bytes)?;
        }
    }

    on_progress(format!("Imported modpack '{}'", profile.name));
    Ok(ModpackImportResult {
        profile,
        downloaded_files,
        extracted_overrides,
        restricted_files: Vec::new(),
    })
}

/// Import a CurseForge modpack `.zip` (containing `manifest.json`) into a new `Profile`.
pub async fn import_curseforge_pack<F>(
    paths: &AppPaths,
    zip_path: &Path,
    cf_client: &CurseForgeClient,
    profile_name: Option<&str>,
    kind: ProfileKind,
    max_concurrent: usize,
    on_progress: F,
) -> Result<ModpackImportResult>
where
    F: Fn(String) + Send + Sync + 'static,
{
    paths.ensure_dirs()?;
    on_progress("Reading CurseForge manifest.json...".to_string());

    let (manifest, overrides_entries) = read_cf_pack_archive(zip_path)?;
    let mod_loader = parse_cf_loader(&manifest.minecraft.mod_loaders);
    let final_name = profile_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&manifest.name);

    let mut profile = Profile::new(
        final_name,
        kind,
        manifest.minecraft.version.clone(),
        mod_loader,
    );
    profile.source_pack = Some(manifest.name.clone());

    let instance_dir = paths.instance_dir(&profile.id);
    let mods_dir = paths.mods_dir(&profile.id);
    std::fs::create_dir_all(&mods_dir)?;

    let file_ids: Vec<u64> = manifest.files.iter().map(|f| f.file_id).collect();
    on_progress(format!("Resolving {} CurseForge pack files...", file_ids.len()));

    let mut resolved_files: Vec<CfFile> = Vec::new();
    for chunk in file_ids.chunks(100) {
        let mut batch = cf_client.get_files_by_ids(chunk).await?;
        resolved_files.append(&mut batch);
    }

    let mut download_tasks = Vec::new();
    let mut restricted_pending: Vec<CfFile> = Vec::new();

    for cf_file in resolved_files {
        if let Some(url) = cf_file.download_url.clone().filter(|u| !u.trim().is_empty()) {
            let safe_name = sanitize_rel_path(&cf_file.file_name)?;
            let dest = mods_dir.join(safe_name);
            let sha1 = cf_file.sha1_hash().map(ToString::to_string);
            download_tasks.push((url, dest, sha1, cf_file));
        } else {
            restricted_pending.push(cf_file);
        }
    }

    // Fetch website URLs for any distribution-restricted mods so the user gets clickable links
    let mut restricted_files = Vec::new();
    if !restricted_pending.is_empty() {
        let mod_ids: Vec<u64> = restricted_pending
            .iter()
            .map(|f| f.mod_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mod_map: HashMap<u64, String> = cf_client
            .get_mods(&mod_ids)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let url = m
                    .links
                    .and_then(|l| l.website_url)
                    .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", m.id));
                (m.id, url)
            })
            .collect();

        for f in restricted_pending {
            let website_url = mod_map
                .get(&f.mod_id)
                .cloned()
                .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", f.mod_id));
            restricted_files.push(RestrictedPackFile {
                project_id: f.mod_id,
                file_id: f.id,
                filename: f.file_name,
                website_url,
            });
        }
    }

    let downloaded_files = download_tasks.len();
    if downloaded_files > 0 {
        on_progress(format!(
            "Downloading {downloaded_files} CurseForge pack files..."
        ));
        let simple_tasks: Vec<(String, PathBuf, Option<String>)> = download_tasks
            .iter()
            .map(|(u, d, s, _)| (u.clone(), d.clone(), s.clone()))
            .collect();
        run_concurrent_downloads(simple_tasks, max_concurrent, &on_progress).await?;

        for (url, dest, sha1, cf_file) in download_tasks {
            let murmur2 = std::fs::read(&dest)
                .ok()
                .map(|b| compute_murmur2_fingerprint(&b));
            let _ = upsert_content_meta(
                paths,
                &profile.id,
                InstalledContentMeta {
                    source: ContentSource::CurseForge,
                    kind: ContentKind::Mod,
                    project_id: cf_file.mod_id.to_string(),
                    version_id: cf_file.id.to_string(),
                    version_number: cf_file.display_name,
                    filename: cf_file.file_name,
                    sha1,
                    murmur2,
                    download_url: Some(url),
                    title: None,
                    icon_url: None,
                },
            );
        }
    }

    let extracted_overrides = overrides_entries.len();
    if extracted_overrides > 0 {
        on_progress(format!("Extracting {extracted_overrides} override files..."));
        for (rel, bytes) in overrides_entries {
            let dest = instance_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(dest, bytes)?;
        }
    }

    on_progress(format!("Imported CurseForge pack '{}'", profile.name));
    Ok(ModpackImportResult {
        profile,
        downloaded_files,
        extracted_overrides,
        restricted_files,
    })
}

/// Export a `Profile`, its tracked downloadable content, and overrides into a `.mrpack` archive.
pub fn export_mrpack(
    paths: &AppPaths,
    profile: &Profile,
    dest_path: &Path,
    pack_version: &str,
    summary: Option<&str>,
) -> Result<()> {
    let instance_dir = paths.instance_dir(&profile.id);
    if !instance_dir.exists() {
        std::fs::create_dir_all(&instance_dir)?;
    }
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut dependencies = HashMap::new();
    dependencies.insert("minecraft".to_string(), profile.mc_version.clone());
    match &profile.mod_loader {
        ModLoader::Vanilla => {}
        ModLoader::Fabric { loader_version } => {
            dependencies.insert("fabric-loader".to_string(), loader_version.clone());
        }
        ModLoader::Quilt { loader_version } => {
            dependencies.insert("quilt-loader".to_string(), loader_version.clone());
        }
        ModLoader::NeoForge { loader_version } => {
            dependencies.insert("neoforge".to_string(), loader_version.clone());
        }
        ModLoader::Forge { loader_version } => {
            dependencies.insert("forge".to_string(), loader_version.clone());
        }
    }

    let tracked = list_all_tracked_meta(paths, &profile.id);
    let mut tracked_paths: HashSet<PathBuf> = HashSet::new();
    let mut mrpack_files = Vec::new();

    for meta in tracked {
        let dir = meta.kind.dir_for_profile(paths, &profile.id);
        let file_path = dir.join(&meta.filename);
        if !file_path.is_file() {
            continue;
        }
        let rel_str = match meta.kind {
            ContentKind::Mod => format!("mods/{}", meta.filename),
            ContentKind::ResourcePack => format!("resourcepacks/{}", meta.filename),
            ContentKind::Shader => format!("shaderpacks/{}", meta.filename),
            ContentKind::DataPack => format!("datapacks/{}", meta.filename),
            ContentKind::Modpack => continue,
        };
        if let Some(url) = meta.download_url.filter(|u| !u.trim().is_empty()) {
            let bytes = std::fs::read(&file_path)?;
            let sha1 = meta.sha1.unwrap_or_else(|| sha1_hex(&bytes));
            let mut hashes = HashMap::new();
            hashes.insert("sha1".to_string(), sha1);
            mrpack_files.push(MrpackFile {
                path: rel_str,
                hashes,
                env: None,
                downloads: vec![url],
                file_size: bytes.len() as u64,
            });
            tracked_paths.insert(file_path);
        }
    }

    let index = MrpackIndex {
        format_version: 1,
        game: "minecraft".to_string(),
        version_id: pack_version.to_string(),
        name: profile.name.clone(),
        summary: summary.map(ToString::to_string),
        files: mrpack_files,
        dependencies,
    };

    let out_file = std::fs::File::create(dest_path)?;
    let mut zip = zip::ZipWriter::new(out_file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("modrinth.index.json", options)?;
    let index_json = serde_json::to_string_pretty(&index)?;
    zip.write_all(index_json.as_bytes())?;

    // Bundle untracked content & config files into overrides/
    let override_subdirs = ["mods", "resourcepacks", "shaderpacks", "datapacks", "config"];
    for sub in &override_subdirs {
        let sub_dir = instance_dir.join(sub);
        if sub_dir.is_dir() {
            write_dir_to_overrides(&mut zip, options, &instance_dir, &sub_dir, &tracked_paths)?;
        }
    }
    let options_txt = instance_dir.join("options.txt");
    if options_txt.is_file() {
        zip.start_file("overrides/options.txt", options)?;
        zip.write_all(&std::fs::read(options_txt)?)?;
    }

    zip.finish()?;
    Ok(())
}

/// Download a modpack from `CatalogVersion` (Modrinth `.mrpack` or CurseForge `.zip`)
/// and import it into a new `Profile`.
pub async fn install_modpack_from_catalog<F>(
    paths: &AppPaths,
    catalog: &CatalogClient,
    version: &CatalogVersion,
    profile_name: Option<&str>,
    kind: ProfileKind,
    max_concurrent: usize,
    on_progress: F,
) -> Result<ModpackImportResult>
where
    F: Fn(String) + Send + Sync + 'static,
{
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| {
            MubError::Modpack(format!(
                "Modpack version '{}' has no files",
                version.version_number
            ))
        })?;

    let download_url = match &file.url {
        Some(u) if !u.trim().is_empty() => u.clone(),
        _ => {
            if version.source == ContentSource::CurseForge
                && let Some(cf) = catalog.curseforge()
                && let Ok(mod_id) = version.project_id.parse::<u64>()
                && let Ok(file_id) = version.id.parse::<u64>()
                && let Some(u) = cf.get_download_url(mod_id, file_id).await?
            {
                u
            } else {
                let website_url = version.website_url.clone().unwrap_or_else(|| {
                    format!("https://www.curseforge.com/projects/{}", version.project_id)
                });
                return Err(MubError::CurseForgeDistributionRestricted {
                    filename: file.filename.clone(),
                    website_url,
                });
            }
        }
    };

    on_progress(format!("Downloading modpack archive '{}'...", file.filename));
    let bytes = match version.source {
        ContentSource::Modrinth => catalog.modrinth().download_bytes(&download_url).await?,
        ContentSource::CurseForge => {
            let cf = catalog.curseforge().ok_or_else(|| {
                MubError::CurseForgeAuth("CurseForge API key is not configured".into())
            })?;
            cf.download_bytes(&download_url).await?
        }
    };

    let tmp_dir = paths.shared_dir().join("cache");
    std::fs::create_dir_all(&tmp_dir)?;
    let tmp_path = tmp_dir.join(format!(
        "pack_{}_{}",
        uuid::Uuid::new_v4(),
        file.filename
    ));
    std::fs::write(&tmp_path, &bytes)?;

    let res = if file.filename.to_lowercase().ends_with(".mrpack")
        || version.source == ContentSource::Modrinth
    {
        import_mrpack(
            paths,
            &tmp_path,
            profile_name,
            kind,
            max_concurrent,
            on_progress,
        )
        .await
    } else {
        let cf = catalog.curseforge().ok_or_else(|| {
            MubError::CurseForgeAuth("CurseForge API key is not configured".into())
        })?;
        import_curseforge_pack(
            paths,
            &tmp_path,
            cf,
            profile_name,
            kind,
            max_concurrent,
            on_progress,
        )
        .await
    };

    let _ = std::fs::remove_file(&tmp_path);
    res
}

fn write_dir_to_overrides<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: SimpleFileOptions,
    instance_dir: &Path,
    dir: &Path,
    tracked_paths: &HashSet<PathBuf>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if fname.starts_with(".mub-") {
            continue;
        }
        if path.is_dir() {
            write_dir_to_overrides(zip, options, instance_dir, &path, tracked_paths)?;
        } else if path.is_file() && !tracked_paths.contains(&path) {
            let rel = path
                .strip_prefix(instance_dir)
                .map_err(|e| MubError::Modpack(e.to_string()))?;
            let zip_name = format!(
                "overrides/{}",
                rel.to_string_lossy().replace('\\', "/")
            );
            zip.start_file(zip_name, options)?;
            zip.write_all(&std::fs::read(&path)?)?;
        }
    }
    Ok(())
}

type OverridesList = Vec<(PathBuf, Vec<u8>)>;

fn read_mrpack_archive(mrpack_path: &Path) -> Result<(MrpackIndex, OverridesList)> {
    let file = std::fs::File::open(mrpack_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut index: Option<MrpackIndex> = None;
    let mut base_overrides = Vec::new();
    let mut client_overrides = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if name == "modrinth.index.json" {
            let mut raw = String::new();
            entry.read_to_string(&mut raw)?;
            index = Some(serde_json::from_str(&raw)?);
        } else if let Some(rest) = name.strip_prefix("overrides/") {
            if !rest.is_empty() {
                let rel = sanitize_rel_path(rest)?;
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                base_overrides.push((rel, buf));
            }
        } else if let Some(rest) = name.strip_prefix("client-overrides/")
            && !rest.is_empty()
        {
            let rel = sanitize_rel_path(rest)?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            client_overrides.push((rel, buf));
        }
    }

    let index = index.ok_or_else(|| {
        MubError::Modpack("Archive is missing modrinth.index.json".into())
    })?;
    base_overrides.extend(client_overrides);
    Ok((index, base_overrides))
}

fn read_cf_pack_archive(zip_path: &Path) -> Result<(CurseForgeManifest, OverridesList)> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    let manifest: CurseForgeManifest = {
        let mut manifest_entry = archive.by_name("manifest.json").map_err(|_| {
            MubError::Modpack("CurseForge pack archive is missing manifest.json".into())
        })?;
        let mut raw = String::new();
        manifest_entry.read_to_string(&mut raw)?;
        serde_json::from_str(&raw)?
    };

    let overrides_prefix = format!(
        "{}/",
        manifest.overrides.trim_matches('/').replace('\\', "/")
    );
    let mut overrides = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if let Some(rest) = name.strip_prefix(&overrides_prefix)
            && !rest.is_empty()
        {
            let rel = sanitize_rel_path(rest)?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            overrides.push((rel, buf));
        }
    }

    Ok((manifest, overrides))
}

fn sanitize_rel_path(raw: &str) -> Result<PathBuf> {
    let normalized = raw.replace('\\', "/");
    let p = Path::new(&normalized);
    if p.is_absolute() {
        return Err(MubError::Modpack(format!(
            "Unsafe absolute path in modpack: '{raw}'"
        )));
    }
    let mut clean = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(seg) => clean.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(MubError::Modpack(format!(
                    "Unsafe path traversal in modpack: '{raw}'"
                )));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(MubError::Modpack("Empty path in modpack".into()));
    }
    Ok(clean)
}

fn parse_mrpack_loader(deps: &HashMap<String, String>) -> ModLoader {
    if let Some(v) = deps.get("fabric-loader") {
        ModLoader::Fabric {
            loader_version: v.clone(),
        }
    } else if let Some(v) = deps.get("quilt-loader") {
        ModLoader::Quilt {
            loader_version: v.clone(),
        }
    } else if let Some(v) = deps.get("neoforge") {
        ModLoader::NeoForge {
            loader_version: v.clone(),
        }
    } else if let Some(v) = deps.get("forge") {
        ModLoader::Forge {
            loader_version: v.clone(),
        }
    } else {
        ModLoader::Vanilla
    }
}

fn parse_cf_loader(loaders: &[CurseForgeManifestLoader]) -> ModLoader {
    let chosen = loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| loaders.first());
    let Some(loader) = chosen else {
        return ModLoader::Vanilla;
    };
    let id = loader.id.trim();
    if let Some(ver) = id.strip_prefix("fabric-") {
        ModLoader::Fabric {
            loader_version: ver.to_string(),
        }
    } else if let Some(ver) = id.strip_prefix("quilt-") {
        ModLoader::Quilt {
            loader_version: ver.to_string(),
        }
    } else if let Some(ver) = id.strip_prefix("neoforge-") {
        ModLoader::NeoForge {
            loader_version: ver.to_string(),
        }
    } else if let Some(ver) = id.strip_prefix("forge-") {
        ModLoader::Forge {
            loader_version: ver.to_string(),
        }
    } else {
        ModLoader::Vanilla
    }
}

async fn run_concurrent_downloads<F>(
    tasks: Vec<(String, PathBuf, Option<String>)>,
    max_concurrent: usize,
    on_progress: &F,
) -> Result<()>
where
    F: Fn(String) + Send + Sync,
{
    let http = Client::builder()
        .user_agent("ModsUndBots/0.2.0 (https://github.com/modsundbots)")
        .pool_max_idle_per_host(16)
        .build()
        .unwrap_or_else(|_| Client::new());

    let total = tasks.len();
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sem = Arc::new(Semaphore::new(max_concurrent.max(1)));

    let results: Vec<Result<()>> = stream::iter(tasks)
        .map(|(url, dest, expected_sha1)| {
            let http = http.clone();
            let sem = Arc::clone(&sem);
            let done = Arc::clone(&done);
            async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|e| MubError::Download(format!("Semaphore error: {e}")))?;

                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let resp = http.get(&url).send().await?;
                if !resp.status().is_success() {
                    return Err(MubError::Download(format!(
                        "GET {url} failed with status {}",
                        resp.status()
                    )));
                }
                let bytes = resp.bytes().await?;
                if let Some(expected) = expected_sha1 {
                    let actual = sha1_hex(&bytes);
                    if !actual.eq_ignore_ascii_case(&expected) {
                        return Err(MubError::Download(format!(
                            "SHA-1 mismatch for {url}: expected {expected}, got {actual}"
                        )));
                    }
                }
                std::fs::write(&dest, &bytes)?;
                let count = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if count.is_multiple_of(10) || count == total {
                    on_progress(format!("Pack files {count}/{total}"));
                }
                Ok(())
            }
        })
        .buffer_unordered(max_concurrent.max(1))
        .collect()
        .await;

    for r in results {
        r?;
    }
    Ok(())
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
