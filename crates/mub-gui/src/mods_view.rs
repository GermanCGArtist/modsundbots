use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, CornerRadius, Id, Margin, Order, RichText, Sense, Stroke};
use mub_core::{
    AppPaths, CatalogClient, CatalogDetail, CatalogHit, CatalogSort, CatalogVersion, ContentKind,
    ContentSource, ModLoader, ModpackImportResult, ModrinthEnvironment, MubError, Profile,
    ProfileKind, ProfileStore, SourceFilter, install_modpack_from_catalog, list_installed_content,
};
use tokio::sync::{Semaphore, mpsc};

use crate::ToastTone;
use crate::theme::{self, ButtonTone, Palette};

const PAGE_SIZE: u32 = 20;

const CATALOG_CATEGORIES: &[(&str, &str)] = &[
    ("optimization", "Optimization"),
    ("utility", "Utility"),
    ("technology", "Technology"),
    ("adventure", "Adventure"),
    ("worldgen", "Worldgen"),
    ("magic", "Magic"),
    ("library", "Library"),
    ("management", "Management"),
    ("equipment", "Equipment"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderFilter {
    Profile,
    Fabric,
    NeoForge,
    Quilt,
    Forge,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    Versions,
    Dependencies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionChannelFilter {
    All,
    Release,
    Beta,
    Alpha,
}

pub enum ModsAction {
    OpenConfig,
    ModpackCreated(Box<Profile>),
    Toast(ToastTone, String),
    TaskProgress(Option<(f32, String)>),
}

enum ModBgEvent {
    SearchResults {
        hits: Vec<CatalogHit>,
        total_hits: u64,
        append: bool,
    },
    ProjectDetail {
        detail: CatalogDetail,
        mr_versions: Vec<CatalogVersion>,
        cf_versions: Vec<CatalogVersion>,
        dep_names: HashMap<(ContentSource, String), String>,
    },
    ImageLoaded {
        key: String,
        image: egui::ColorImage,
    },
    ImageFailed {
        key: String,
    },
    Installed {
        key: String,
        message: String,
        source: ContentSource,
    },
    ModpackInstalled {
        key: String,
        result: ModpackImportResult,
    },
    DistributionRestricted {
        key: String,
        filename: String,
        website_url: String,
    },
    InstallError {
        key: String,
        error: String,
    },
    Error(String),
}

pub struct ModsViewState {
    pub selected_profile_id: Option<String>,
    pub install_all_bots: bool,

    pub content_kind: ContentKind,
    pub source_filter: SourceFilter,
    pub loader_filter: LoaderFilter,
    pub environment: ModrinthEnvironment,
    pub selected_categories: HashSet<String>,

    pub query: String,
    pub sort: CatalogSort,
    pub page: u32,
    pub total_hits: u64,
    pub results: Vec<CatalogHit>,
    pub loading: bool,
    pub loaded_once: bool,

    // Project detail view
    pub active_detail: Option<CatalogDetail>,
    pub mr_versions: Vec<CatalogVersion>,
    pub cf_versions: Vec<CatalogVersion>,
    pub dep_names: HashMap<(ContentSource, String), String>,
    pub detail_tab: DetailTab,
    pub detail_loading: bool,
    pub channel_filter: VersionChannelFilter,
    pub detail_source_filter: SourceFilter,

    // Gallery lightbox modal
    lightbox: Option<(String, String)>, // (texture_key, caption)

    // Distribution restricted notice (filename, website_url)
    restricted_notice: Option<(String, String)>,

    // Per-item installing state
    installing_keys: HashSet<String>,

    // Disk-cached texture loader + bounded concurrency
    textures: HashMap<String, egui::TextureHandle>,
    fetching_images: HashSet<String>,
    image_sem: Arc<Semaphore>,

    tx: mpsc::UnboundedSender<ModBgEvent>,
    rx: mpsc::UnboundedReceiver<ModBgEvent>,
}

impl ModsViewState {
    pub fn new(first_profile_id: Option<String>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            selected_profile_id: first_profile_id,
            install_all_bots: false,
            content_kind: ContentKind::Mod,
            source_filter: SourceFilter::All,
            loader_filter: LoaderFilter::Profile,
            environment: ModrinthEnvironment::Any,
            selected_categories: HashSet::new(),
            query: String::new(),
            sort: CatalogSort::Downloads,
            page: 0,
            total_hits: 0,
            results: Vec::new(),
            loading: false,
            loaded_once: false,
            active_detail: None,
            mr_versions: Vec::new(),
            cf_versions: Vec::new(),
            dep_names: HashMap::new(),
            detail_tab: DetailTab::Overview,
            detail_loading: false,
            channel_filter: VersionChannelFilter::All,
            detail_source_filter: SourceFilter::All,
            lightbox: None,
            restricted_notice: None,
            installing_keys: HashSet::new(),
            textures: HashMap::new(),
            fetching_images: HashSet::new(),
            image_sem: Arc::new(Semaphore::new(8)),
            tx,
            rx,
        }
    }

    pub fn lock_to_profile(&mut self, profile_id: String) {
        self.selected_profile_id = Some(profile_id);
        self.loaded_once = false;
        self.page = 0;
        self.active_detail = None;
        self.lightbox = None;
    }

    pub fn invalidate_catalog(&mut self) {
        self.loaded_once = false;
        self.page = 0;
    }

    fn drain_events(
        &mut self,
        ctx: &egui::Context,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        actions: &mut Vec<ModsAction>,
    ) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                ModBgEvent::SearchResults {
                    hits,
                    total_hits,
                    append,
                } => {
                    self.loading = false;
                    self.total_hits = total_hits;
                    for h in &hits {
                        if let Some(url) = &h.icon_url {
                            let key = hit_icon_key(h);
                            self.spawn_image_fetch(rt, ctx, paths, &key, url);
                        }
                    }
                    if append {
                        for h in hits {
                            let dup = self.results.iter().any(|e| {
                                (h.modrinth_id.is_some() && e.modrinth_id == h.modrinth_id)
                                    || (h.curseforge_id.is_some()
                                        && e.curseforge_id == h.curseforge_id)
                            });
                            if !dup {
                                self.results.push(h);
                            }
                        }
                    } else {
                        self.results = hits;
                    }
                }
                ModBgEvent::ProjectDetail {
                    detail,
                    mr_versions,
                    cf_versions,
                    dep_names,
                } => {
                    self.detail_loading = false;
                    if let Some(url) = &detail.icon_url {
                        let key = detail_icon_key(&detail);
                        self.spawn_image_fetch(rt, ctx, paths, &key, url);
                    }
                    for (idx, g) in detail.gallery.iter().take(8).enumerate() {
                        let key = format!("gallery_{}_{idx}", detail.slug);
                        self.spawn_image_fetch(rt, ctx, paths, &key, &g.url);
                    }
                    self.active_detail = Some(detail);
                    self.mr_versions = mr_versions;
                    self.cf_versions = cf_versions;
                    self.dep_names = dep_names;
                }
                ModBgEvent::ImageLoaded { key, image } => {
                    self.fetching_images.remove(&key);
                    let tex = ctx.load_texture(
                        format!("mub_cat_tex_{key}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.textures.insert(key, tex);
                }
                ModBgEvent::ImageFailed { key } => {
                    self.fetching_images.remove(&key);
                }
                ModBgEvent::Installed {
                    key,
                    message,
                    source,
                } => {
                    self.installing_keys.remove(&key);
                    actions.push(ModsAction::TaskProgress(None));
                    let tone = match source {
                        ContentSource::Modrinth => ToastTone::Mr,
                        ContentSource::CurseForge => ToastTone::Cf,
                    };
                    actions.push(ModsAction::Toast(tone, message));
                }
                ModBgEvent::ModpackInstalled { key, result } => {
                    self.installing_keys.remove(&key);
                    actions.push(ModsAction::TaskProgress(None));
                    let restricted_count = result.restricted_files.len();
                    let name = result.profile.name.clone();
                    if let Some(first) = result.restricted_files.first() {
                        self.restricted_notice = Some((
                            format!(
                                "{} restricted file(s) in modpack (e.g. {})",
                                restricted_count, first.filename
                            ),
                            first.website_url.clone(),
                        ));
                    }
                    actions.push(ModsAction::ModpackCreated(Box::new(result.profile)));
                    actions.push(ModsAction::Toast(
                        if restricted_count > 0 {
                            ToastTone::Warn
                        } else {
                            ToastTone::Mr
                        },
                        format!(
                            "Created modpack profile '{name}' ({} files, {} overrides{})",
                            result.downloaded_files,
                            result.extracted_overrides,
                            if restricted_count > 0 {
                                format!(", {restricted_count} restricted")
                            } else {
                                String::new()
                            }
                        ),
                    ));
                }
                ModBgEvent::DistributionRestricted {
                    key,
                    filename,
                    website_url,
                } => {
                    self.installing_keys.remove(&key);
                    actions.push(ModsAction::TaskProgress(None));
                    self.restricted_notice = Some((filename.clone(), website_url));
                    actions.push(ModsAction::Toast(
                        ToastTone::Warn,
                        format!(
                            "'{filename}' has third-party CurseForge distribution disabled — open website link to download manually."
                        ),
                    ));
                }
                ModBgEvent::InstallError { key, error } => {
                    self.installing_keys.remove(&key);
                    actions.push(ModsAction::TaskProgress(None));
                    actions.push(ModsAction::Toast(ToastTone::Danger, error));
                }
                ModBgEvent::Error(e) => {
                    self.loading = false;
                    self.detail_loading = false;
                    actions.push(ModsAction::TaskProgress(None));
                    actions.push(ModsAction::Toast(ToastTone::Danger, e));
                }
            }
        }
    }

    fn spawn_image_fetch(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        paths: &AppPaths,
        key: &str,
        url: &str,
    ) {
        if url.is_empty()
            || self.textures.contains_key(key)
            || self.fetching_images.contains(key)
        {
            return;
        }
        self.fetching_images.insert(key.to_string());
        let key = key.to_string();
        let url = url.to_string();
        let cache_path = icon_cache_path(paths, &url);
        let sem = Arc::clone(&self.image_sem);
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            // Try disk cache first
            if let Ok(cached) = tokio::fs::read(&cache_path).await
                && let Some(img) = decode_color_image(&cached)
            {
                let _ = tx.send(ModBgEvent::ImageLoaded { key, image: img });
                ctx.request_repaint();
                return;
            }

            let Ok(_permit) = sem.acquire().await else {
                let _ = tx.send(ModBgEvent::ImageFailed { key });
                return;
            };

            let Ok(resp) = reqwest::get(&url).await else {
                let _ = tx.send(ModBgEvent::ImageFailed { key });
                return;
            };
            if !resp.status().is_success() {
                let _ = tx.send(ModBgEvent::ImageFailed { key });
                return;
            }
            let Ok(bytes) = resp.bytes().await else {
                let _ = tx.send(ModBgEvent::ImageFailed { key });
                return;
            };

            if let Some(img) = decode_color_image(&bytes) {
                if let Some(parent) = cache_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&cache_path, &bytes).await;
                let _ = tx.send(ModBgEvent::ImageLoaded { key, image: img });
            } else {
                let _ = tx.send(ModBgEvent::ImageFailed { key });
            }
            ctx.request_repaint();
        });
    }

    fn effective_loader(&self, profile: &Profile) -> Option<ModLoader> {
        match self.loader_filter {
            LoaderFilter::Profile => Some(profile.mod_loader.clone()),
            LoaderFilter::Fabric => Some(ModLoader::Fabric {
                loader_version: String::new(),
            }),
            LoaderFilter::NeoForge => Some(ModLoader::NeoForge {
                loader_version: String::new(),
            }),
            LoaderFilter::Quilt => Some(ModLoader::Quilt {
                loader_version: String::new(),
            }),
            LoaderFilter::Forge => Some(ModLoader::Forge {
                loader_version: String::new(),
            }),
            LoaderFilter::Any => None,
        }
    }

    fn fetch_catalog(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        cf_api_key: Option<String>,
        profile: &Profile,
        append: bool,
    ) {
        self.loading = true;
        self.loaded_once = true;
        let catalog = CatalogClient::new(cf_api_key);
        let source_filter = self.source_filter;
        let kind = self.content_kind;
        let query = self.query.trim().to_string();
        let mc_version = profile.mc_version.clone();
        let loader = self.effective_loader(profile);
        let categories: Vec<String> = self.selected_categories.iter().cloned().collect();
        let sort = self.sort;
        let env = self.environment;
        let offset = self.page * PAGE_SIZE;
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            let res = if env != ModrinthEnvironment::Any
                && (source_filter == SourceFilter::Modrinth
                    || (source_filter == SourceFilter::All && !catalog.has_curseforge()))
            {
                let mr_sort = match sort {
                    CatalogSort::Downloads => mub_core::ModrinthSort::Downloads,
                    CatalogSort::Relevance => mub_core::ModrinthSort::Relevance,
                    CatalogSort::Updated => mub_core::ModrinthSort::Updated,
                    CatalogSort::Newest => mub_core::ModrinthSort::Newest,
                };
                let effective_loader = match kind {
                    ContentKind::Mod | ContentKind::Modpack => loader.as_ref(),
                    _ => None,
                };
                catalog
                    .modrinth()
                    .search_projects_advanced(
                        kind.modrinth_project_type(),
                        &query,
                        Some(&mc_version),
                        effective_loader,
                        &categories,
                        env,
                        mr_sort,
                        offset,
                        PAGE_SIZE,
                    )
                    .await
                    .map(|r| {
                        let hits = r
                            .hits
                            .into_iter()
                            .map(|h| CatalogHit {
                                modrinth_id: Some(h.project_id),
                                curseforge_id: None,
                                sources: vec![ContentSource::Modrinth],
                                kind,
                                title: h.title,
                                slug: h.slug.clone(),
                                author: h.author,
                                description: h.description,
                                downloads: h.downloads,
                                follows: h.follows,
                                icon_url: h.icon_url,
                                categories: h.categories,
                                website_url: None,
                            })
                            .collect();
                        mub_core::CatalogSearchResponse {
                            hits,
                            total_hits: r.total_hits,
                            offset: r.offset,
                            limit: r.limit,
                        }
                    })
            } else {
                catalog
                    .search(
                        source_filter,
                        kind,
                        &query,
                        Some(&mc_version),
                        loader.as_ref(),
                        &categories,
                        sort,
                        offset,
                        PAGE_SIZE,
                    )
                    .await
            };

            match res {
                Ok(resp) => {
                    let _ = tx.send(ModBgEvent::SearchResults {
                        hits: resp.hits,
                        total_hits: resp.total_hits,
                        append,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::Error(format!("Catalog search failed: {e}")));
                }
            }
            ctx.request_repaint();
        });
    }

    fn open_project_detail(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        cf_api_key: Option<String>,
        hit: &CatalogHit,
        profile: &Profile,
    ) {
        self.detail_loading = true;
        self.detail_tab = DetailTab::Overview;
        self.channel_filter = VersionChannelFilter::All;
        self.detail_source_filter = SourceFilter::All;
        self.active_detail = None;
        self.mr_versions.clear();
        self.cf_versions.clear();
        self.dep_names.clear();
        self.lightbox = None;

        let catalog = CatalogClient::new(cf_api_key);
        let hit = hit.clone();
        let mc = profile.mc_version.clone();
        let loader = self.effective_loader(profile);
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            match catalog.get_detail(&hit).await {
                Ok(detail) => {
                    let mr_fut = async {
                        if let Some(mr_id) = &detail.modrinth_id {
                            catalog
                                .list_versions(
                                    ContentSource::Modrinth,
                                    mr_id,
                                    detail.kind,
                                    Some(&mc),
                                    loader.as_ref(),
                                )
                                .await
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    };
                    let cf_fut = async {
                        if let Some(cf_id) = detail.curseforge_id
                            && catalog.has_curseforge()
                        {
                            catalog
                                .list_versions(
                                    ContentSource::CurseForge,
                                    &cf_id.to_string(),
                                    detail.kind,
                                    Some(&mc),
                                    loader.as_ref(),
                                )
                                .await
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    };
                    let (mr_versions, cf_versions) = tokio::join!(mr_fut, cf_fut);

                    // Resolve human-readable dependency names for displayed dependencies
                    let mut mr_dep_ids = Vec::new();
                    let mut cf_dep_ids = Vec::new();
                    for v in mr_versions.iter().take(3).chain(cf_versions.iter().take(3)) {
                        for d in &v.dependencies {
                            if let Some(pid) = &d.project_id {
                                match d.source {
                                    ContentSource::Modrinth => {
                                        if !mr_dep_ids.contains(pid) {
                                            mr_dep_ids.push(pid.clone());
                                        }
                                    }
                                    ContentSource::CurseForge => {
                                        if let Ok(num) = pid.parse::<u64>()
                                            && !cf_dep_ids.contains(&num)
                                        {
                                            cf_dep_ids.push(num);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let mut dep_names = HashMap::new();
                    if !mr_dep_ids.is_empty()
                        && let Ok(projs) = catalog.modrinth().get_projects(&mr_dep_ids).await
                    {
                        for p in projs {
                            dep_names.insert((ContentSource::Modrinth, p.id), p.title);
                        }
                    }
                    if !cf_dep_ids.is_empty()
                        && let Some(cf) = catalog.curseforge()
                        && let Ok(mods) = cf.get_mods(&cf_dep_ids).await
                    {
                        for m in mods {
                            dep_names
                                .insert((ContentSource::CurseForge, m.id.to_string()), m.name);
                        }
                    }

                    let _ = tx.send(ModBgEvent::ProjectDetail {
                        detail,
                        mr_versions,
                        cf_versions,
                        dep_names,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::Error(format!(
                        "Failed to load project '{}': {e}",
                        hit.title
                    )));
                }
            }
            ctx.request_repaint();
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn install_from_hit(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        paths: &AppPaths,
        profiles: &ProfileStore,
        cf_api_key: Option<String>,
        selected: &Profile,
        hit: &CatalogHit,
        source: ContentSource,
        actions: &mut Vec<ModsAction>,
    ) {
        let install_key = hit_install_key(hit, source);
        if self.installing_keys.contains(&install_key) {
            return;
        }
        let Some(pid) = (match source {
            ContentSource::Modrinth => hit.modrinth_id.clone(),
            ContentSource::CurseForge => hit.curseforge_id.map(|id| id.to_string()),
        }) else {
            return;
        };

        self.installing_keys.insert(install_key.clone());
        actions.push(ModsAction::TaskProgress(Some((
            0.3,
            format!("Installing {} from {}...", hit.title, source),
        ))));

        let mut target_ids = vec![selected.id.clone()];
        if self.install_all_bots {
            for p in &profiles.profiles {
                if p.kind == ProfileKind::Bot && !target_ids.contains(&p.id) {
                    target_ids.push(p.id.clone());
                }
            }
        }

        let catalog = CatalogClient::new(cf_api_key);
        let paths = paths.clone();
        let title = hit.title.clone();
        let kind = hit.kind;
        let mc = selected.mc_version.clone();
        let loader = self.effective_loader(selected);
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            match catalog
                .list_versions(source, &pid, kind, Some(&mc), loader.as_ref())
                .await
            {
                Ok(versions) => {
                    let Some(latest) = versions.first() else {
                        let _ = tx.send(ModBgEvent::InstallError {
                            key: install_key,
                            error: format!("No compatible {source} version of '{title}' for MC {mc}"),
                        });
                        ctx.request_repaint();
                        return;
                    };
                    if kind == ContentKind::Modpack {
                        let tx_prog = tx.clone();
                        let res = install_modpack_from_catalog(
                            &paths,
                            &catalog,
                            latest,
                            None,
                            ProfileKind::Player,
                            16,
                            move |_msg| {
                                let _ = &tx_prog;
                            },
                        )
                        .await;
                        match res {
                            Ok(result) => {
                                let _ = tx.send(ModBgEvent::ModpackInstalled {
                                    key: install_key,
                                    result,
                                });
                            }
                            Err(MubError::CurseForgeDistributionRestricted {
                                filename,
                                website_url,
                            }) => {
                                let _ = tx.send(ModBgEvent::DistributionRestricted {
                                    key: install_key,
                                    filename,
                                    website_url,
                                });
                            }
                            Err(e) => {
                                let _ = tx.send(ModBgEvent::InstallError {
                                    key: install_key,
                                    error: format!("Modpack install failed: {e}"),
                                });
                            }
                        }
                    } else {
                        match catalog
                            .install_with_dependencies(
                                &paths,
                                &target_ids,
                                kind,
                                latest,
                                Some(&mc),
                                loader.as_ref(),
                            )
                            .await
                        {
                            Ok(files) => {
                                let extra = if files.len() > 1 {
                                    format!(" (+{} dep(s))", files.len() - 1)
                                } else {
                                    String::new()
                                };
                                let _ = tx.send(ModBgEvent::Installed {
                                    key: install_key,
                                    message: format!(
                                        "Installed {title}{extra} via {source} to {} profile(s).",
                                        target_ids.len()
                                    ),
                                    source,
                                });
                            }
                            Err(MubError::CurseForgeDistributionRestricted {
                                filename,
                                website_url,
                            }) => {
                                let _ = tx.send(ModBgEvent::DistributionRestricted {
                                    key: install_key,
                                    filename,
                                    website_url,
                                });
                            }
                            Err(e) => {
                                let _ = tx.send(ModBgEvent::InstallError {
                                    key: install_key,
                                    error: format!("Install failed for {title}: {e}"),
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::InstallError {
                        key: install_key,
                        error: format!("Failed to fetch versions for {title}: {e}"),
                    });
                }
            }
            ctx.request_repaint();
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn install_version(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        paths: &AppPaths,
        profiles: &ProfileStore,
        cf_api_key: Option<String>,
        selected: &Profile,
        kind: ContentKind,
        version: &CatalogVersion,
        title: &str,
        actions: &mut Vec<ModsAction>,
    ) {
        let install_key = format!("ver_{}_{}", version.source, version.id);
        if self.installing_keys.contains(&install_key) {
            return;
        }
        self.installing_keys.insert(install_key.clone());
        actions.push(ModsAction::TaskProgress(Some((
            0.35,
            format!("Installing {} {}...", title, version.version_number),
        ))));

        let mut target_ids = vec![selected.id.clone()];
        if self.install_all_bots {
            for p in &profiles.profiles {
                if p.kind == ProfileKind::Bot && !target_ids.contains(&p.id) {
                    target_ids.push(p.id.clone());
                }
            }
        }

        let catalog = CatalogClient::new(cf_api_key);
        let paths = paths.clone();
        let version = version.clone();
        let title = title.to_string();
        let mc = selected.mc_version.clone();
        let loader = self.effective_loader(selected);
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            if kind == ContentKind::Modpack {
                match install_modpack_from_catalog(
                    &paths,
                    &catalog,
                    &version,
                    None,
                    ProfileKind::Player,
                    16,
                    |_| {},
                )
                .await
                {
                    Ok(result) => {
                        let _ = tx.send(ModBgEvent::ModpackInstalled {
                            key: install_key,
                            result,
                        });
                    }
                    Err(MubError::CurseForgeDistributionRestricted {
                        filename,
                        website_url,
                    }) => {
                        let _ = tx.send(ModBgEvent::DistributionRestricted {
                            key: install_key,
                            filename,
                            website_url,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(ModBgEvent::InstallError {
                            key: install_key,
                            error: format!("Modpack install failed: {e}"),
                        });
                    }
                }
            } else {
                match catalog
                    .install_with_dependencies(
                        &paths,
                        &target_ids,
                        kind,
                        &version,
                        Some(&mc),
                        loader.as_ref(),
                    )
                    .await
                {
                    Ok(files) => {
                        let extra = if files.len() > 1 {
                            format!(" (+{} dep(s))", files.len() - 1)
                        } else {
                            String::new()
                        };
                        let _ = tx.send(ModBgEvent::Installed {
                            key: install_key,
                            message: format!(
                                "Installed {title} {}{extra} to {} profile(s).",
                                version.version_number,
                                target_ids.len()
                            ),
                            source: version.source,
                        });
                    }
                    Err(MubError::CurseForgeDistributionRestricted {
                        filename,
                        website_url,
                    }) => {
                        let _ = tx.send(ModBgEvent::DistributionRestricted {
                            key: install_key,
                            filename,
                            website_url,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(ModBgEvent::InstallError {
                            key: install_key,
                            error: format!("Install failed: {e}"),
                        });
                    }
                }
            }
            ctx.request_repaint();
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        cf_api_key: Option<String>,
    ) -> Vec<ModsAction> {
        let mut actions = Vec::new();
        self.drain_events(ctx, rt, paths, &mut actions);

        if profiles.profiles.is_empty() {
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new("NO PROFILES CONFIGURED")
                            .monospace()
                            .size(14.0)
                            .strong()
                            .color(palette.ink),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Create a profile in 01 INSTANCES first (or install a Modpack directly once a target exists).",
                        )
                        .monospace()
                        .size(12.0)
                        .color(palette.ink_muted),
                    );
                    ui.add_space(24.0);
                });
            });
            return actions;
        }

        if self
            .selected_profile_id
            .as_deref()
            .and_then(|id| profiles.get(id))
            .is_none()
        {
            self.selected_profile_id = profiles.profiles.first().map(|p| p.id.clone());
        }

        let Some(selected) = self
            .selected_profile_id
            .as_deref()
            .and_then(|id| profiles.get(id))
            .cloned()
        else {
            return actions;
        };

        // Top Catalog Header
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 410.0).max(200.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("DISCOVER CATALOG")
                            .monospace()
                            .size(16.0)
                            .strong()
                            .color(palette.ink),
                    );
                    theme::term_badge(ui, "MR", palette.mr, palette.mr_bg);
                    ui.label(RichText::new("+").monospace().color(palette.ink_muted));
                    theme::term_badge(ui, "CF", palette.cf, palette.cf_bg);
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Unified Modrinth & CurseForge search with automatic dependency resolution & deduplication.",
                        )
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                    )
                    .truncate(),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::term_toggle(
                    ui,
                    palette,
                    &mut self.install_all_bots,
                    "SYNC BOTS",
                    palette.bot,
                );

                let prev = self.selected_profile_id.clone();
                egui::ComboBox::from_id_salt("discover_target_profile")
                    .width(210.0)
                    .selected_text(format!(
                        "{} · {} · {}",
                        selected.name, selected.mc_version, selected.mod_loader
                    ))
                    .show_ui(ui, |ui| {
                        for p in &profiles.profiles {
                            ui.selectable_value(
                                &mut self.selected_profile_id,
                                Some(p.id.clone()),
                                format!("{} · {} · {}", p.name, p.mc_version, p.mod_loader),
                            );
                        }
                    });
                ui.label(
                    RichText::new("TARGET:")
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                );
                if self.selected_profile_id != prev {
                    self.loaded_once = false;
                    self.page = 0;
                    self.active_detail = None;
                }
            });
        });

        ui.add_space(6.0);

        // Restricted CurseForge distribution notice if triggered
        if let Some((filename, url)) = self.restricted_notice.clone() {
            theme::term_box(
                ui,
                palette.warn_bg,
                palette.warn,
                Margin::symmetric(10, 6),
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "! CurseForge distribution restricted for '{filename}'."
                            ))
                            .monospace()
                            .size(11.5)
                            .strong()
                            .color(palette.warn),
                        );
                        ui.hyperlink_to(
                            RichText::new("OPEN WEBSITE TO DOWNLOAD ->")
                                .monospace()
                                .size(11.5)
                                .strong()
                                .color(palette.player),
                            &url,
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::term_button(ui, palette, "DISMISS", ButtonTone::Subtle)
                                .clicked()
                            {
                                self.restricted_notice = None;
                            }
                        });
                    });
                },
            );
            ui.add_space(6.0);
        }

        // Content kind tabs + Source switcher
        let mut trigger_fetch = false;
        ui.horizontal(|ui| {
            for (kind, label) in [
                (ContentKind::Mod, "MODS"),
                (ContentKind::Modpack, "MODPACKS"),
                (ContentKind::ResourcePack, "RESOURCE PACKS"),
                (ContentKind::Shader, "SHADERS"),
                (ContentKind::DataPack, "DATA PACKS"),
            ] {
                if theme::term_tab(ui, palette, label, self.content_kind == kind).clicked()
                    && self.content_kind != kind
                {
                    self.content_kind = kind;
                    self.page = 0;
                    self.active_detail = None;
                    trigger_fetch = true;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (sf, label, tone) in [
                    (SourceFilter::CurseForge, "CURSEFORGE", ButtonTone::CurseForge),
                    (SourceFilter::Modrinth, "MODRINTH", ButtonTone::Modrinth),
                    (SourceFilter::All, "UNIFIED", ButtonTone::Focus),
                ] {
                    let active = self.source_filter == sf;
                    if theme::term_button(
                        ui,
                        palette,
                        label,
                        if active { tone } else { ButtonTone::Subtle },
                    )
                    .clicked()
                        && self.source_filter != sf
                    {
                        self.source_filter = sf;
                        self.page = 0;
                        trigger_fetch = true;
                    }
                }
            });
        });
        theme::term_divider(ui, palette);

        // Missing CurseForge API key notice
        if cf_api_key.is_none() && self.source_filter == SourceFilter::CurseForge {
            theme::term_box(
                ui,
                palette.warn_bg,
                palette.warn,
                Margin::symmetric(12, 8),
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(
                                "! CurseForge API v1 key is not set. Configure your key in 05 CONFIG (or set MUB_CURSEFORGE_API_KEY) to search CurseForge.",
                            )
                            .monospace()
                            .size(12.0)
                            .color(palette.warn),
                        );
                        if theme::term_button(ui, palette, "OPEN 05 CONFIG ->", ButtonTone::Warn)
                            .clicked()
                        {
                            actions.push(ModsAction::OpenConfig);
                        }
                    });
                },
            );
            ui.add_space(6.0);
        } else if cf_api_key.is_none() && self.source_filter == SourceFilter::All {
            theme::term_box(
                ui,
                palette.surface,
                palette.border_hi,
                Margin::symmetric(10, 5),
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(
                                "CurseForge API key not configured — showing Modrinth results only.",
                            )
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                        );
                        if theme::term_button(ui, palette, "SET CF KEY", ButtonTone::CurseForge)
                            .clicked()
                        {
                            actions.push(ModsAction::OpenConfig);
                        }
                    });
                },
            );
            ui.add_space(4.0);
        }

        // Detail loading / Project Detail
        if self.detail_loading {
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Loading project details & dependencies...")
                            .monospace()
                            .color(palette.ink_dim),
                    );
                });
            });
            return actions;
        }

        if let Some(detail) = self.active_detail.clone() {
            self.ui_project_detail(
                ui,
                ctx,
                palette,
                rt,
                paths,
                profiles,
                cf_api_key,
                &selected,
                &detail,
                &mut actions,
            );
            return actions;
        }

        if (!self.loaded_once || trigger_fetch)
            && !self.loading
            && !(self.source_filter == SourceFilter::CurseForge && cf_api_key.is_none())
        {
            self.fetch_catalog(rt, ctx, cf_api_key.clone(), &selected, false);
        }

        self.ui_discover(
            ui,
            ctx,
            palette,
            rt,
            paths,
            profiles,
            cf_api_key,
            &selected,
            &mut actions,
        );

        actions
    }

    #[allow(clippy::too_many_arguments)]
    fn ui_discover(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        cf_api_key: Option<String>,
        selected: &Profile,
        actions: &mut Vec<ModsAction>,
    ) {
        let mut trigger_fetch = false;
        let rail_w = 196.0_f32;
        let avail_h = ui.available_height();

        ui.horizontal_top(|ui| {
            // Left Filter Rail (explicitly bounded vertical column)
            ui.allocate_ui_with_layout(
                egui::vec2(rail_w, avail_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    theme::term_surface(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .id_salt("discover_filter_rail_scroll")
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                theme::section_header(ui, palette, "LOADER", palette.meta);
                                ui.add_space(4.0);
                                let prof_lbl = format!("Profile ({})", selected.mod_loader);
                                let prev_loader = self.loader_filter;
                                for (lf, lbl) in [
                                    (LoaderFilter::Profile, prof_lbl.as_str()),
                                    (LoaderFilter::Fabric, "Fabric"),
                                    (LoaderFilter::NeoForge, "NeoForge"),
                                    (LoaderFilter::Quilt, "Quilt"),
                                    (LoaderFilter::Forge, "Forge"),
                                    (LoaderFilter::Any, "Any Loader"),
                                ] {
                                    let active = self.loader_filter == lf;
                                    let prefix = if active { "[×]" } else { "[ ]" };
                                    if theme::term_rail_option(
                                        ui,
                                        palette,
                                        prefix,
                                        lbl,
                                        active,
                                        palette.focus,
                                    )
                                    .clicked()
                                    {
                                        self.loader_filter = lf;
                                    }
                                }
                                if self.loader_filter != prev_loader {
                                    self.page = 0;
                                    trigger_fetch = true;
                                }

                                ui.add_space(8.0);
                                theme::term_divider(ui, palette);
                                theme::section_header(ui, palette, "ENVIRONMENT", palette.meta);
                                ui.add_space(4.0);
                                let prev_env = self.environment;
                                for (env, lbl) in [
                                    (ModrinthEnvironment::Any, "Any"),
                                    (ModrinthEnvironment::Client, "Client-side"),
                                    (ModrinthEnvironment::Server, "Server-side"),
                                ] {
                                    let active = self.environment == env;
                                    let prefix = if active { "(●)" } else { "( )" };
                                    if theme::term_rail_option(
                                        ui,
                                        palette,
                                        prefix,
                                        lbl,
                                        active,
                                        palette.player,
                                    )
                                    .clicked()
                                    {
                                        self.environment = env;
                                    }
                                }
                                if self.environment != prev_env {
                                    self.page = 0;
                                    trigger_fetch = true;
                                }

                                ui.add_space(8.0);
                                theme::term_divider(ui, palette);
                                theme::section_header(ui, palette, "CATEGORIES", palette.meta);
                                ui.add_space(4.0);
                                for (slug, label) in CATALOG_CATEGORIES {
                                    let active = self.selected_categories.contains(*slug);
                                    let prefix = if active { "[×]" } else { "[ ]" };
                                    if theme::term_rail_option(
                                        ui,
                                        palette,
                                        prefix,
                                        label,
                                        active,
                                        palette.focus,
                                    )
                                    .clicked()
                                    {
                                        if active {
                                            self.selected_categories.remove(*slug);
                                        } else {
                                            self.selected_categories.insert((*slug).to_string());
                                        }
                                        self.page = 0;
                                        trigger_fetch = true;
                                    }
                                }

                                if self.loader_filter != LoaderFilter::Profile
                                    || self.environment != ModrinthEnvironment::Any
                                    || !self.selected_categories.is_empty()
                                {
                                    ui.add_space(8.0);
                                    if theme::term_button(
                                        ui,
                                        palette,
                                        "RESET FILTERS",
                                        ButtonTone::Secondary,
                                    )
                                    .clicked()
                                    {
                                        self.loader_filter = LoaderFilter::Profile;
                                        self.environment = ModrinthEnvironment::Any;
                                        self.selected_categories.clear();
                                        self.page = 0;
                                        trigger_fetch = true;
                                    }
                                }
                            });
                    });
                },
            );

            ui.add_space(10.0);

            // Main Catalog Results Column (explicitly bounded vertical column)
            let main_w = ui.available_width().max(260.0);
            ui.allocate_ui_with_layout(
                egui::vec2(main_w, avail_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(main_w);

                    // Search & Sort bar
                    theme::term_surface(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let search_w = (ui.available_width() - 280.0).max(160.0);
                            let hint = format!(
                                "search {} across Modrinth & CurseForge...",
                                self.content_kind.label().to_lowercase()
                            );
                            let (_, cleared, enter) = theme::term_search_bar(
                                ui,
                                palette,
                                &mut self.query,
                                "/",
                                &hint,
                                search_w,
                            );
                            if cleared {
                                self.page = 0;
                                trigger_fetch = true;
                            }

                            let btn_lbl = if self.loading { "LOADING..." } else { "SEARCH" };
                            let search_clicked = ui
                                .add_enabled_ui(!self.loading, |ui| {
                                    theme::term_button(ui, palette, btn_lbl, ButtonTone::Cta)
                                })
                                .inner
                                .clicked();
                            if search_clicked || enter {
                                self.page = 0;
                                trigger_fetch = true;
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let prev_sort = self.sort;
                                egui::ComboBox::from_id_salt("discover_sort_box")
                                    .width(165.0)
                                    .selected_text(match self.sort {
                                        CatalogSort::Downloads => "SORT: DOWNLOADS",
                                        CatalogSort::Relevance => "SORT: RELEVANCE",
                                        CatalogSort::Updated => "SORT: UPDATED",
                                        CatalogSort::Newest => "SORT: NEWEST",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.sort,
                                            CatalogSort::Downloads,
                                            "SORT: DOWNLOADS",
                                        );
                                        ui.selectable_value(
                                            &mut self.sort,
                                            CatalogSort::Relevance,
                                            "SORT: RELEVANCE",
                                        );
                                        ui.selectable_value(
                                            &mut self.sort,
                                            CatalogSort::Updated,
                                            "SORT: UPDATED",
                                        );
                                        ui.selectable_value(
                                            &mut self.sort,
                                            CatalogSort::Newest,
                                            "SORT: NEWEST",
                                        );
                                    });
                                if self.sort != prev_sort {
                                    self.page = 0;
                                    trigger_fetch = true;
                                }
                            });
                        });
                    });

                    ui.add_space(6.0);

                    // Count + target info bar
                    ui.horizontal(|ui| {
                        let loader_desc = match self.effective_loader(selected) {
                            Some(l) => l.to_string(),
                            None => "ANY LOADER".to_string(),
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "SHOWING {} OF {} {} · MC {} + {}",
                                    self.results.len(),
                                    format_count(self.total_hits),
                                    self.content_kind.label().to_uppercase(),
                                    selected.mc_version,
                                    loader_desc.to_uppercase(),
                                ))
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                            )
                            .truncate(),
                        );
                        if self.loading {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new("DEDUPLICATED MR + CF")
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.focus),
                            );
                        });
                    });

                    ui.add_space(6.0);

                    let installed_ids =
                        installed_project_ids_for_kind(paths, &selected.id, self.content_kind);
                    let mut detail_to_open: Option<CatalogHit> = None;
                    let mut install_target: Option<(CatalogHit, ContentSource)> = None;
                    let mut load_more = false;

                    let scroll_out = egui::ScrollArea::vertical().show(ui, |ui| {
                        for hit in &self.results {
                            let is_installed = hit
                                .modrinth_id
                                .as_ref()
                                .is_some_and(|id| installed_ids.contains(id))
                                || hit
                                    .curseforge_id
                                    .is_some_and(|id| installed_ids.contains(&id.to_string()));

                            theme::term_row(ui, palette, |ui| {
                                let card_w = ui.available_width();
                                ui.set_width(card_w);
                                let icon_w = 48.0_f32;
                                let right_w = 228.0_f32;
                                let mid_w = (card_w - icon_w - right_w - 22.0).max(150.0);

                                ui.horizontal_top(|ui| {
                                    let icon_key = hit_icon_key(hit);
                                    let accent = if hit.sources.contains(&ContentSource::Modrinth) {
                                        palette.mr
                                    } else {
                                        palette.cf
                                    };
                                    self.render_icon(ui, palette, &icon_key, &hit.title, accent, icon_w);
                                    ui.add_space(6.0);

                                    ui.allocate_ui_with_layout(
                                        egui::vec2(mid_w, 0.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_width(mid_w);
                                            ui.horizontal_wrapped(|ui| {
                                                if ui
                                                    .link(
                                                        RichText::new(&hit.title)
                                                            .monospace()
                                                            .size(14.0)
                                                            .strong()
                                                            .color(palette.ink),
                                                    )
                                                    .clicked()
                                                {
                                                    detail_to_open = Some(hit.clone());
                                                }
                                                if !hit.author.is_empty() {
                                                    ui.label(
                                                        RichText::new(format!("by {}", hit.author))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(palette.ink_muted),
                                                    );
                                                }
                                                for src in &hit.sources {
                                                    match src {
                                                        ContentSource::Modrinth => {
                                                            theme::term_badge(
                                                                ui,
                                                                "MR",
                                                                palette.mr,
                                                                palette.mr_bg,
                                                            );
                                                        }
                                                        ContentSource::CurseForge => {
                                                            theme::term_badge(
                                                                ui,
                                                                "CF",
                                                                palette.cf,
                                                                palette.cf_bg,
                                                            );
                                                        }
                                                    }
                                                }
                                                if is_installed {
                                                    theme::term_badge(
                                                        ui,
                                                        "INSTALLED",
                                                        palette.focus,
                                                        palette.focus_bg,
                                                    );
                                                }
                                            });

                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&hit.description)
                                                        .monospace()
                                                        .size(11.5)
                                                        .color(palette.ink_dim),
                                                )
                                                .wrap(),
                                            );

                                            ui.add_space(3.0);
                                            ui.horizontal_wrapped(|ui| {
                                                for cat in hit.categories.iter().take(4) {
                                                    theme::term_badge_bordered(
                                                        ui,
                                                        &format!("#{cat}"),
                                                        palette.meta,
                                                        Color32::TRANSPARENT,
                                                        palette.border,
                                                    );
                                                }
                                            });
                                        },
                                    );

                                    ui.add_space(6.0);

                                    ui.allocate_ui_with_layout(
                                        egui::vec2(right_w, 0.0),
                                        egui::Layout::top_down(egui::Align::Max),
                                        |ui| {
                                            ui.set_width(right_w);

                                            // Single right-aligned stats LayoutJob: `↓ X · ★ Y`
                                            let mut stats_job = egui::text::LayoutJob {
                                                halign: egui::Align::Max,
                                                ..Default::default()
                                            };
                                            stats_job.append(
                                                &format!("↓ {}", format_count(hit.downloads)),
                                                0.0,
                                                egui::TextFormat {
                                                    font_id: egui::FontId::monospace(11.5),
                                                    color: palette.ink,
                                                    ..Default::default()
                                                },
                                            );
                                            stats_job.append(
                                                " · ",
                                                0.0,
                                                egui::TextFormat {
                                                    font_id: egui::FontId::monospace(11.5),
                                                    color: palette.ink_muted,
                                                    ..Default::default()
                                                },
                                            );
                                            stats_job.append(
                                                &format!("★ {}", format_count(hit.follows)),
                                                0.0,
                                                egui::TextFormat {
                                                    font_id: egui::FontId::monospace(11.5),
                                                    color: palette.warn,
                                                    ..Default::default()
                                                },
                                            );
                                            ui.label(stats_job);

                                            ui.add_space(6.0);

                                            let has_mr = hit.modrinth_id.is_some();
                                            let has_cf = hit.curseforge_id.is_some()
                                                && cf_api_key.is_some();
                                            let compact = has_mr && has_cf;

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if has_cf {
                                                        let ikey = hit_install_key(
                                                            hit,
                                                            ContentSource::CurseForge,
                                                        );
                                                        let busy =
                                                            self.installing_keys.contains(&ikey);
                                                        let lbl = if busy {
                                                            "..."
                                                        } else if hit.kind == ContentKind::Modpack
                                                        {
                                                            if compact {
                                                                "[+] CF"
                                                            } else {
                                                                "[+] CF PACK"
                                                            }
                                                        } else if compact {
                                                            "[↓] CF"
                                                        } else {
                                                            "[↓] CF INSTALL"
                                                        };
                                                        if ui
                                                            .add_enabled_ui(!busy, |ui| {
                                                                theme::term_button(
                                                                    ui,
                                                                    palette,
                                                                    lbl,
                                                                    ButtonTone::CurseForge,
                                                                )
                                                            })
                                                            .inner
                                                            .clicked()
                                                        {
                                                            install_target = Some((
                                                                hit.clone(),
                                                                ContentSource::CurseForge,
                                                            ));
                                                        }
                                                    }

                                                    if has_mr {
                                                        let ikey = hit_install_key(
                                                            hit,
                                                            ContentSource::Modrinth,
                                                        );
                                                        let busy =
                                                            self.installing_keys.contains(&ikey);
                                                        let lbl = if busy {
                                                            "..."
                                                        } else if hit.kind == ContentKind::Modpack
                                                        {
                                                            if compact {
                                                                "[+] MR"
                                                            } else {
                                                                "[+] MR PACK"
                                                            }
                                                        } else if compact {
                                                            "[↓] MR"
                                                        } else {
                                                            "[↓] MR INSTALL"
                                                        };
                                                        if ui
                                                            .add_enabled_ui(!busy, |ui| {
                                                                theme::term_button(
                                                                    ui,
                                                                    palette,
                                                                    lbl,
                                                                    ButtonTone::Modrinth,
                                                                )
                                                            })
                                                            .inner
                                                            .clicked()
                                                        {
                                                            install_target = Some((
                                                                hit.clone(),
                                                                ContentSource::Modrinth,
                                                            ));
                                                        }
                                                    }

                                                    if theme::term_button(
                                                        ui,
                                                        palette,
                                                        "INSPECT",
                                                        ButtonTone::Secondary,
                                                    )
                                                    .clicked()
                                                    {
                                                        detail_to_open = Some(hit.clone());
                                                    }
                                                },
                                            );
                                        },
                                    );
                                });
                            });
                            ui.add_space(5.0);
                        }

                        if !self.loading && self.results.len() < self.total_hits as usize {
                            ui.horizontal(|ui| {
                                if theme::term_button(ui, palette, "LOAD MORE", ButtonTone::Secondary)
                                    .clicked()
                                {
                                    load_more = true;
                                }
                            });
                        } else if self.loading && !self.results.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Loading more results...")
                                        .monospace()
                                        .color(palette.ink_muted),
                                );
                            });
                        }
                    });

                    // Infinite scroll near bottom
                    if !self.loading
                        && self.results.len() < self.total_hits as usize
                        && scroll_out.content_size.y > 0.0
                        && (scroll_out.state.offset.y + scroll_out.inner_rect.height() + 240.0)
                            >= scroll_out.content_size.y
                    {
                        load_more = true;
                    }

                    if let Some(hit) = detail_to_open {
                        self.open_project_detail(rt, ctx, cf_api_key.clone(), &hit, selected);
                    }
                    if let Some((hit, src)) = install_target {
                        self.install_from_hit(
                            rt,
                            ctx,
                            paths,
                            profiles,
                            cf_api_key.clone(),
                            selected,
                            &hit,
                            src,
                            actions,
                        );
                    }
                    if load_more && !self.loading {
                        self.page += 1;
                        self.fetch_catalog(rt, ctx, cf_api_key.clone(), selected, true);
                    }
                },
            );
        });

        if trigger_fetch
            && !self.loading
            && !(self.source_filter == SourceFilter::CurseForge && cf_api_key.is_none())
        {
            self.fetch_catalog(rt, ctx, cf_api_key, selected, false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ui_project_detail(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        cf_api_key: Option<String>,
        selected: &Profile,
        detail: &CatalogDetail,
        actions: &mut Vec<ModsAction>,
    ) {
        ui.horizontal(|ui| {
            if theme::term_button(ui, palette, "<- BACK TO CATALOG", ButtonTone::Secondary)
                .clicked()
            {
                self.active_detail = None;
                self.lightbox = None;
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for src in detail.sources.iter().rev() {
                    match src {
                        ContentSource::Modrinth => {
                            theme::term_badge(ui, "MR", palette.mr, palette.mr_bg);
                        }
                        ContentSource::CurseForge => {
                            theme::term_badge(ui, "CF", palette.cf, palette.cf_bg);
                        }
                    }
                }
                ui.label(
                    RichText::new("SOURCES:")
                        .monospace()
                        .size(11.0)
                        .color(palette.ink_muted),
                );
            });
        });
        ui.add_space(8.0);

        let installed_ids = installed_project_ids_for_kind(paths, &selected.id, detail.kind);
        let is_installed = detail
            .modrinth_id
            .as_ref()
            .is_some_and(|id| installed_ids.contains(id))
            || detail
                .curseforge_id
                .is_some_and(|id| installed_ids.contains(&id.to_string()));

        // Hero Header
        let mut version_to_install: Option<CatalogVersion> = None;
        theme::term_raised(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            let detail_right_w = 230.0_f32;
            let detail_mid_w = (ui.available_width() - 64.0 - detail_right_w - 24.0).max(180.0);

            ui.horizontal_top(|ui| {
                let icon_key = detail_icon_key(detail);
                let accent = if detail.sources.contains(&ContentSource::Modrinth) {
                    palette.mr
                } else {
                    palette.cf
                };
                self.render_icon(ui, palette, &icon_key, &detail.title, accent, 64.0);
                ui.add_space(10.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(detail_mid_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(detail_mid_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&detail.title)
                                    .monospace()
                                    .size(17.0)
                                    .strong()
                                    .color(palette.ink),
                            );
                            if !detail.author.is_empty() {
                                ui.label(
                                    RichText::new(format!("by {}", detail.author))
                                        .monospace()
                                        .size(11.5)
                                        .color(palette.ink_muted),
                                );
                            }
                            if is_installed {
                                theme::term_badge(ui, "INSTALLED", palette.focus, palette.focus_bg);
                            }
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new(&detail.description)
                                    .monospace()
                                    .size(12.0)
                                    .color(palette.ink_dim),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("↓ {}", format_count(detail.downloads)))
                                    .monospace()
                                    .size(11.5)
                                    .strong()
                                    .color(palette.ink),
                            );
                            ui.label(
                                RichText::new(format!("★ {}", format_count(detail.follows)))
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.warn),
                            );
                            if let Some(lic) = &detail.license {
                                ui.label(
                                    RichText::new(format!("LICENSE: {lic}"))
                                        .monospace()
                                        .size(11.5)
                                        .color(palette.meta),
                                );
                            }
                        });
                    },
                );

                ui.add_space(8.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(detail_right_w, 0.0),
                    egui::Layout::top_down(egui::Align::Max),
                    |ui| {
                        ui.set_width(detail_right_w);
                        if let Some(latest_mr) = self.mr_versions.first() {
                            let key = format!("ver_{}_{}", latest_mr.source, latest_mr.id);
                            let busy = self.installing_keys.contains(&key);
                            let lbl = if busy {
                                "INSTALLING MR...".to_string()
                            } else if detail.kind == ContentKind::Modpack {
                                format!("[+] PACK MR ({})", latest_mr.version_number)
                            } else {
                                format!("[↓] INSTALL MR ({})", latest_mr.version_number)
                            };
                            if ui
                                .add_enabled_ui(!busy, |ui| {
                                    theme::term_button(ui, palette, &lbl, ButtonTone::Modrinth)
                                })
                                .inner
                                .clicked()
                            {
                                version_to_install = Some(latest_mr.clone());
                            }
                        }
                        if let Some(latest_cf) = self.cf_versions.first() {
                            let key = format!("ver_{}_{}", latest_cf.source, latest_cf.id);
                            let busy = self.installing_keys.contains(&key);
                            let lbl = if busy {
                                "INSTALLING CF...".to_string()
                            } else if detail.kind == ContentKind::Modpack {
                                format!("[+] PACK CF ({})", latest_cf.version_number)
                            } else {
                                format!("[↓] INSTALL CF ({})", latest_cf.version_number)
                            };
                            if ui
                                .add_enabled_ui(!busy, |ui| {
                                    theme::term_button(ui, palette, &lbl, ButtonTone::CurseForge)
                                })
                                .inner
                                .clicked()
                            {
                                version_to_install = Some(latest_cf.clone());
                            }
                        }
                    },
                );
            });
        });

        ui.add_space(8.0);

        // Detail sub-tabs
        let total_vers = self.mr_versions.len() + self.cf_versions.len();
        let vers_lbl = format!("VERSIONS & CHANGELOG ({total_vers})");
        ui.horizontal(|ui| {
            if theme::term_tab(
                ui,
                palette,
                "OVERVIEW",
                self.detail_tab == DetailTab::Overview,
            )
            .clicked()
            {
                self.detail_tab = DetailTab::Overview;
            }
            if theme::term_tab(
                ui,
                palette,
                &vers_lbl,
                self.detail_tab == DetailTab::Versions,
            )
            .clicked()
            {
                self.detail_tab = DetailTab::Versions;
            }
            if theme::term_tab(
                ui,
                palette,
                "DEPENDENCIES",
                self.detail_tab == DetailTab::Dependencies,
            )
            .clicked()
            {
                self.detail_tab = DetailTab::Dependencies;
            }
        });
        theme::term_divider(ui, palette);

        match self.detail_tab {
            DetailTab::Overview => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Links row
                    let has_links = detail.source_url.is_some()
                        || detail.discord_url.is_some()
                        || detail.wiki_url.is_some()
                        || detail.issues_url.is_some()
                        || detail.website_url.is_some();
                    if has_links {
                        ui.horizontal_wrapped(|ui| {
                            if let Some(u) = &detail.source_url {
                                ui.hyperlink_to(
                                    RichText::new("[SOURCE]").monospace().color(palette.player),
                                    u,
                                );
                            }
                            if let Some(u) = &detail.discord_url {
                                ui.hyperlink_to(
                                    RichText::new("[DISCORD]").monospace().color(palette.focus),
                                    u,
                                );
                            }
                            if let Some(u) = &detail.wiki_url {
                                ui.hyperlink_to(
                                    RichText::new("[WIKI]").monospace().color(palette.meta),
                                    u,
                                );
                            }
                            if let Some(u) = &detail.issues_url {
                                ui.hyperlink_to(
                                    RichText::new("[ISSUES]").monospace().color(palette.warn),
                                    u,
                                );
                            }
                            if let Some(u) = &detail.website_url {
                                ui.hyperlink_to(
                                    RichText::new("[WEBSITE]").monospace().color(palette.ink_dim),
                                    u,
                                );
                            }
                        });
                        ui.add_space(8.0);
                    }

                    // Gallery with click-to-enlarge lightbox
                    if !detail.gallery.is_empty() {
                        theme::section_header(
                            ui,
                            palette,
                            "GALLERY (CLICK TO ENLARGE)",
                            palette.focus,
                        );
                        ui.add_space(4.0);
                        egui::ScrollArea::horizontal().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (idx, item) in detail.gallery.iter().take(8).enumerate() {
                                    let key = format!("gallery_{}_{idx}", detail.slug);
                                    theme::term_surface(ui, palette, |ui| {
                                        if let Some(tex) = self.textures.get(&key) {
                                            let img_resp = ui.add(
                                                egui::Image::new(tex)
                                                    .fit_to_exact_size(egui::vec2(240.0, 140.0))
                                                    .sense(Sense::click()),
                                            );
                                            if img_resp.clicked() {
                                                let caption = item
                                                    .title
                                                    .clone()
                                                    .or_else(|| item.description.clone())
                                                    .unwrap_or_else(|| {
                                                        format!("{} — Screenshot #{}", detail.title, idx + 1)
                                                    });
                                                self.lightbox = Some((key.clone(), caption));
                                            }
                                        } else {
                                            ui.allocate_ui(egui::vec2(240.0, 140.0), |ui| {
                                                ui.centered_and_justified(|ui| {
                                                    ui.spinner();
                                                });
                                            });
                                        }
                                        if let Some(t) = &item.title {
                                            ui.add_space(3.0);
                                            ui.label(
                                                RichText::new(t)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(palette.ink_dim),
                                            );
                                        }
                                    });
                                }
                            });
                        });
                        ui.add_space(10.0);
                    }

                    // Rendered body
                    theme::term_surface(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        theme::section_header(ui, palette, "DESCRIPTION", palette.meta);
                        ui.add_space(6.0);
                        crate::markdown::render_markdown(ui, palette, &detail.body);
                    });
                });
            }
            DetailTab::Versions => {
                // Channel filter + Source filter
                ui.horizontal(|ui| {
                    for (ch, lbl, tone) in [
                        (VersionChannelFilter::All, "ALL", ButtonTone::Focus),
                        (VersionChannelFilter::Release, "RELEASE", ButtonTone::Modrinth),
                        (VersionChannelFilter::Beta, "BETA", ButtonTone::Warn),
                        (VersionChannelFilter::Alpha, "ALPHA", ButtonTone::Danger),
                    ] {
                        let active = self.channel_filter == ch;
                        if theme::term_button(
                            ui,
                            palette,
                            lbl,
                            if active { tone } else { ButtonTone::Subtle },
                        )
                        .clicked()
                        {
                            self.channel_filter = ch;
                        }
                    }

                    if !self.mr_versions.is_empty() && !self.cf_versions.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for (sf, lbl, tone) in [
                                (SourceFilter::CurseForge, "CF ONLY", ButtonTone::CurseForge),
                                (SourceFilter::Modrinth, "MR ONLY", ButtonTone::Modrinth),
                                (SourceFilter::All, "BOTH", ButtonTone::Focus),
                            ] {
                                let active = self.detail_source_filter == sf;
                                if theme::term_button(
                                    ui,
                                    palette,
                                    lbl,
                                    if active { tone } else { ButtonTone::Subtle },
                                )
                                .clicked()
                                {
                                    self.detail_source_filter = sf;
                                }
                            }
                        });
                    }
                });
                ui.add_space(6.0);

                let mut combined: Vec<&CatalogVersion> = Vec::new();
                if self.detail_source_filter != SourceFilter::CurseForge {
                    combined.extend(self.mr_versions.iter());
                }
                if self.detail_source_filter != SourceFilter::Modrinth {
                    combined.extend(self.cf_versions.iter());
                }
                combined.sort_by(|a, b| b.date_published.cmp(&a.date_published));

                let filtered: Vec<&CatalogVersion> = combined
                    .into_iter()
                    .filter(|v| match self.channel_filter {
                        VersionChannelFilter::All => true,
                        VersionChannelFilter::Release => {
                            v.version_type.eq_ignore_ascii_case("release")
                        }
                        VersionChannelFilter::Beta => v.version_type.eq_ignore_ascii_case("beta"),
                        VersionChannelFilter::Alpha => v.version_type.eq_ignore_ascii_case("alpha"),
                    })
                    .collect();

                if filtered.is_empty() {
                    theme::term_surface(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(format!(
                                "No matching versions for MC {} ({}).",
                                selected.mc_version, selected.mod_loader
                            ))
                            .monospace()
                            .color(palette.ink_muted),
                        );
                    });
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for ver in filtered {
                            theme::term_row(ui, palette, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width(
                                            (ui.available_width() - 140.0).max(180.0),
                                        );
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new(&ver.version_number)
                                                    .monospace()
                                                    .strong()
                                                    .size(12.5)
                                                    .color(palette.ink),
                                            );
                                            match ver.source {
                                                ContentSource::Modrinth => {
                                                    theme::term_badge(
                                                        ui,
                                                        "MR",
                                                        palette.mr,
                                                        palette.mr_bg,
                                                    );
                                                }
                                                ContentSource::CurseForge => {
                                                    theme::term_badge(
                                                        ui,
                                                        "CF",
                                                        palette.cf,
                                                        palette.cf_bg,
                                                    );
                                                }
                                            }
                                            let (ch_fg, ch_bg) =
                                                match ver.version_type.to_lowercase().as_str() {
                                                    "release" => (palette.mr, palette.mr_bg),
                                                    "beta" => (palette.warn, palette.warn_bg),
                                                    _ => (palette.danger, palette.danger_bg),
                                                };
                                            theme::term_badge(
                                                ui,
                                                &ver.version_type.to_uppercase(),
                                                ch_fg,
                                                ch_bg,
                                            );
                                            let date_short =
                                                ver.date_published.get(..10).unwrap_or(&ver.date_published);
                                            ui.label(
                                                RichText::new(date_short)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(palette.meta),
                                            );
                                        });
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · MC: {} · Loaders: {}",
                                                ver.name,
                                                ver.game_versions.join(", "),
                                                if ver.loaders.is_empty() {
                                                    "any".to_string()
                                                } else {
                                                    ver.loaders.join(", ")
                                                }
                                            ))
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.ink_muted),
                                        );
                                        if let Some(cl) =
                                            ver.changelog.as_deref().filter(|c| !c.trim().is_empty())
                                        {
                                            let preview: String =
                                                cl.trim().chars().take(240).collect();
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(preview)
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(palette.ink_dim),
                                                )
                                                .wrap(),
                                            );
                                        }
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let key = format!("ver_{}_{}", ver.source, ver.id);
                                            let busy = self.installing_keys.contains(&key);
                                            let lbl = if busy { "..." } else { "[↓] INSTALL" };
                                            if ui
                                                .add_enabled_ui(!busy, |ui| {
                                                    theme::term_button(
                                                        ui,
                                                        palette,
                                                        lbl,
                                                        ButtonTone::Cta,
                                                    )
                                                })
                                                .inner
                                                .clicked()
                                            {
                                                version_to_install = Some(ver.clone());
                                            }
                                        },
                                    );
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
            }
            DetailTab::Dependencies => {
                theme::term_surface(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_header(
                        ui,
                        palette,
                        &format!("AUTO-RESOLVED DEPENDENCIES FOR MC {}", selected.mc_version),
                        palette.meta,
                    );
                    ui.add_space(6.0);

                    let mut seen = HashSet::new();
                    let mut deps = Vec::new();
                    for v in self.mr_versions.first().into_iter().chain(self.cf_versions.first()) {
                        for d in &v.dependencies {
                            let id_key = d
                                .project_id
                                .clone()
                                .or_else(|| d.version_id.clone())
                                .unwrap_or_default();
                            if seen.insert((d.source, id_key.clone())) {
                                deps.push((d.clone(), id_key));
                            }
                        }
                    }

                    if deps.is_empty() {
                        ui.label(
                            RichText::new("✓ Zero dependencies declared for the latest compatible version.")
                                .monospace()
                                .size(12.0)
                                .color(palette.mr),
                        );
                    } else {
                        for (dep, id_key) in deps {
                            let resolved_name = dep
                                .project_id
                                .as_ref()
                                .and_then(|pid| self.dep_names.get(&(dep.source, pid.clone())))
                                .cloned()
                                .unwrap_or_else(|| id_key.clone());

                            theme::term_row(ui, palette, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&resolved_name)
                                            .monospace()
                                            .strong()
                                            .size(12.5)
                                            .color(palette.ink),
                                    );
                                    if resolved_name != id_key {
                                        ui.label(
                                            RichText::new(format!("({id_key})"))
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.ink_muted),
                                        );
                                    }
                                    match dep.source {
                                        ContentSource::Modrinth => {
                                            theme::term_badge(ui, "MR", palette.mr, palette.mr_bg);
                                        }
                                        ContentSource::CurseForge => {
                                            theme::term_badge(ui, "CF", palette.cf, palette.cf_bg);
                                        }
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if dep.required {
                                                theme::term_badge(
                                                    ui,
                                                    "REQUIRED",
                                                    palette.warn,
                                                    palette.warn_bg,
                                                );
                                            } else {
                                                theme::term_badge(
                                                    ui,
                                                    "OPTIONAL",
                                                    palette.player,
                                                    palette.player_bg,
                                                );
                                            }
                                        },
                                    );
                                });
                            });
                            ui.add_space(4.0);
                        }
                    }
                });
            }
        }

        if let Some(ver) = version_to_install {
            self.install_version(
                rt,
                ctx,
                paths,
                profiles,
                cf_api_key,
                selected,
                detail.kind,
                &ver,
                &detail.title,
                actions,
            );
        }

        // Gallery Lightbox Modal
        if let Some((tex_key, caption)) = self.lightbox.clone() {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.lightbox = None;
            } else {
                let screen = ctx.screen_rect();
                egui::Area::new(Id::new("discover_gallery_lightbox"))
                    .order(Order::Foreground)
                    .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.painter().rect_filled(
                            screen,
                            CornerRadius::ZERO,
                            Color32::from_rgba_unmultiplied(0, 0, 0, 170),
                        );

                        let max_w = (screen.width() * 0.82).min(960.0);
                        let max_h = (screen.height() * 0.78).min(640.0);
                        egui::Frame::new()
                            .fill(palette.raised)
                            .stroke(Stroke::new(1.0_f32, palette.border_hi))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_max_width(max_w);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&caption)
                                            .monospace()
                                            .strong()
                                            .size(12.5)
                                            .color(palette.ink),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if theme::term_button(
                                                ui,
                                                palette,
                                                "CLOSE [ESC]",
                                                ButtonTone::Secondary,
                                            )
                                            .clicked()
                                            {
                                                self.lightbox = None;
                                            }
                                        },
                                    );
                                });
                                ui.add_space(8.0);
                                if let Some(tex) = self.textures.get(&tex_key) {
                                    ui.add(
                                        egui::Image::new(tex)
                                            .fit_to_exact_size(egui::vec2(max_w - 24.0, max_h - 60.0)),
                                    );
                                }
                            });
                    });
            }
        }
    }

    fn render_icon(
        &self,
        ui: &mut egui::Ui,
        palette: &Palette,
        key: &str,
        title: &str,
        accent: Color32,
        size: f32,
    ) {
        if let Some(tex) = self.textures.get(key) {
            ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(size, size)));
        } else {
            theme::paint_icon_placeholder(ui, palette, title, accent, size);
        }
    }
}

fn hit_icon_key(hit: &CatalogHit) -> String {
    if let Some(mr) = &hit.modrinth_id {
        format!("mr_{mr}")
    } else if let Some(cf) = hit.curseforge_id {
        format!("cf_{cf}")
    } else {
        format!("slug_{}", hit.slug)
    }
}

fn detail_icon_key(detail: &CatalogDetail) -> String {
    if let Some(mr) = &detail.modrinth_id {
        format!("mr_{mr}")
    } else if let Some(cf) = detail.curseforge_id {
        format!("cf_{cf}")
    } else {
        format!("slug_{}", detail.slug)
    }
}

fn hit_install_key(hit: &CatalogHit, source: ContentSource) -> String {
    match source {
        ContentSource::Modrinth => format!("mr_{}", hit.modrinth_id.as_deref().unwrap_or(&hit.slug)),
        ContentSource::CurseForge => format!(
            "cf_{}",
            hit.curseforge_id.map(|i| i.to_string()).unwrap_or_else(|| hit.slug.clone())
        ),
    }
}

fn installed_project_ids_for_kind(
    paths: &AppPaths,
    profile_id: &str,
    kind: ContentKind,
) -> HashSet<String> {
    let mut ids = HashSet::new();
    if let Ok(items) = list_installed_content(paths, profile_id, kind) {
        for item in items {
            if let Some(meta) = item.meta {
                ids.insert(meta.project_id);
            }
        }
    }
    ids
}

fn icon_cache_path(paths: &AppPaths, url: &str) -> PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    paths.icon_cache_dir().join(format!("{hash:016x}.img"))
}

fn decode_color_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
