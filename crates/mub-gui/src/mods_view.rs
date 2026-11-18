use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};
use mub_core::{
    AppPaths, ModLoader, ModrinthClient, ModrinthEnvironment, ModrinthHit, ModrinthProject,
    ModrinthSort, ModrinthVersion, Profile, ProfileKind, ProfileStore, list_installed_mods,
    list_installed_project_ids, remove_installed_mod, toggle_installed_mod,
};
use tokio::sync::mpsc;

use crate::theme::{self, ButtonTone, Palette};

const PAGE_SIZE: u32 = 20;

const MODRINTH_CATEGORIES: &[(&str, &str)] = &[
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
pub enum ModsSubTab {
    Discover,
    Installed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    Versions,
}

pub enum ModBgEvent {
    SearchResults {
        hits: Vec<ModrinthHit>,
        total_hits: u64,
        append: bool,
    },
    ProjectDetail {
        project: ModrinthProject,
        versions: Vec<ModrinthVersion>,
    },
    ImageLoaded {
        key: String,
        image: egui::ColorImage,
    },
    Installed(String),
    Error(String),
}

pub struct ModsViewState {
    pub sub_tab: ModsSubTab,
    pub selected_profile_id: Option<String>,
    pub install_all_bots: bool,

    // Search & filters
    pub query: String,
    pub sort: ModrinthSort,
    pub environment: ModrinthEnvironment,
    pub selected_categories: HashSet<String>,
    pub page: u32,
    pub total_hits: u64,
    pub results: Vec<ModrinthHit>,
    pub loading: bool,
    pub loaded_once: bool,

    // Project detail view
    pub active_project: Option<ModrinthProject>,
    pub active_versions: Vec<ModrinthVersion>,
    pub detail_tab: DetailTab,
    pub detail_loading: bool,

    // Texture cache for icons & gallery images
    pub textures: HashMap<String, egui::TextureHandle>,
    pub fetching_images: HashSet<String>,

    pub tx: mpsc::UnboundedSender<ModBgEvent>,
    pub rx: mpsc::UnboundedReceiver<ModBgEvent>,
}

impl ModsViewState {
    pub fn new(first_profile_id: Option<String>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            sub_tab: ModsSubTab::Discover,
            selected_profile_id: first_profile_id,
            install_all_bots: false,
            query: String::new(),
            sort: ModrinthSort::Downloads,
            environment: ModrinthEnvironment::Any,
            selected_categories: HashSet::new(),
            page: 0,
            total_hits: 0,
            results: Vec::new(),
            loading: false,
            loaded_once: false,
            active_project: None,
            active_versions: Vec::new(),
            detail_tab: DetailTab::Overview,
            detail_loading: false,
            textures: HashMap::new(),
            fetching_images: HashSet::new(),
            tx,
            rx,
        }
    }

    pub fn drain_events(
        &mut self,
        ctx: &egui::Context,
        rt: &Arc<tokio::runtime::Runtime>,
        banner_msg: &mut Option<(bool, String)>,
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
                            self.spawn_image_fetch(rt, &h.project_id, url);
                        }
                    }
                    if append {
                        for h in hits {
                            if !self
                                .results
                                .iter()
                                .any(|existing| existing.project_id == h.project_id)
                            {
                                self.results.push(h);
                            }
                        }
                    } else {
                        self.results = hits;
                    }
                }
                ModBgEvent::ProjectDetail { project, versions } => {
                    self.detail_loading = false;
                    if let Some(url) = &project.icon_url {
                        self.spawn_image_fetch(rt, &project.id, url);
                    }
                    for (idx, g) in project.gallery.iter().take(6).enumerate() {
                        let key = format!("gallery_{}_{idx}", project.id);
                        self.spawn_image_fetch(rt, &key, &g.url);
                    }
                    self.active_project = Some(project);
                    self.active_versions = versions;
                }
                ModBgEvent::ImageLoaded { key, image } => {
                    let tex = ctx.load_texture(
                        format!("mod_tex_{key}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.textures.insert(key, tex);
                }
                ModBgEvent::Installed(msg) => {
                    *banner_msg = Some((false, msg));
                }
                ModBgEvent::Error(e) => {
                    self.loading = false;
                    self.detail_loading = false;
                    *banner_msg = Some((true, e));
                }
            }
        }
    }

    fn spawn_image_fetch(&mut self, rt: &Arc<tokio::runtime::Runtime>, key: &str, url: &str) {
        if url.is_empty()
            || self.textures.contains_key(key)
            || self.fetching_images.contains(key)
        {
            return;
        }
        self.fetching_images.insert(key.to_string());
        let key = key.to_string();
        let url = url.to_string();
        let tx = self.tx.clone();
        rt.spawn(async move {
            let Ok(resp) = reqwest::get(&url).await else {
                return;
            };
            if !resp.status().is_success() {
                return;
            }
            let Ok(bytes) = resp.bytes().await else {
                return;
            };
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                let _ = tx.send(ModBgEvent::ImageLoaded {
                    key,
                    image: color_img,
                });
            }
        });
    }

    fn fetch_catalog(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        mc_version: &str,
        loader: &ModLoader,
        append: bool,
    ) {
        self.loading = true;
        self.loaded_once = true;
        let query = self.query.trim().to_string();
        let mc = mc_version.to_string();
        let loader = loader.clone();
        let categories: Vec<String> = self.selected_categories.iter().cloned().collect();
        let env = self.environment;
        let sort = self.sort;
        let offset = self.page * PAGE_SIZE;
        let tx = self.tx.clone();

        rt.spawn(async move {
            let client = ModrinthClient::new();
            match client
                .search_mods_advanced(
                    &query,
                    Some(&mc),
                    Some(&loader),
                    &categories,
                    env,
                    sort,
                    offset,
                    PAGE_SIZE,
                )
                .await
            {
                Ok(resp) => {
                    let _ = tx.send(ModBgEvent::SearchResults {
                        hits: resp.hits,
                        total_hits: resp.total_hits,
                        append,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::Error(format!("Modrinth search failed: {e}")));
                }
            }
        });
    }

    fn open_project_detail(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        project_id: &str,
        mc_version: &str,
        loader: &ModLoader,
    ) {
        self.detail_loading = true;
        self.detail_tab = DetailTab::Overview;
        self.active_project = None;
        self.active_versions.clear();
        let pid = project_id.to_string();
        let mc = mc_version.to_string();
        let loader = loader.clone();
        let tx = self.tx.clone();

        rt.spawn(async move {
            let client = ModrinthClient::new();
            match client.get_project(&pid).await {
                Ok(project) => {
                    let versions = client
                        .list_project_versions(&pid, Some(&mc), Some(&loader))
                        .await
                        .unwrap_or_default();
                    let _ = tx.send(ModBgEvent::ProjectDetail { project, versions });
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::Error(format!(
                        "Failed to load project '{pid}': {e}"
                    )));
                }
            }
        });
    }

    fn install_latest(
        &self,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        selected: &Profile,
        project_id: &str,
        title: &str,
    ) {
        let mut target_ids = vec![selected.id.clone()];
        if self.install_all_bots {
            for p in &profiles.profiles {
                if p.kind == ProfileKind::Bot && !target_ids.contains(&p.id) {
                    target_ids.push(p.id.clone());
                }
            }
        }
        let pid = project_id.to_string();
        let title = title.to_string();
        let mc = selected.mc_version.clone();
        let loader = selected.mod_loader.clone();
        let paths = paths.clone();
        let tx = self.tx.clone();

        rt.spawn(async move {
            let client = ModrinthClient::new();
            match client
                .list_project_versions(&pid, Some(&mc), Some(&loader))
                .await
            {
                Ok(versions) => {
                    let Some(latest) = versions.first() else {
                        let _ = tx.send(ModBgEvent::Error(format!(
                            "No compatible version of '{title}' for MC {mc} ({loader})"
                        )));
                        return;
                    };
                    match client
                        .install_with_dependencies(
                            &paths,
                            &target_ids,
                            latest,
                            Some(&mc),
                            Some(&loader),
                        )
                        .await
                    {
                        Ok(files) => {
                            let extra = if files.len() > 1 {
                                format!(" (+{} dep(s))", files.len() - 1)
                            } else {
                                String::new()
                            };
                            let _ = tx.send(ModBgEvent::Installed(format!(
                                "Installed {title}{extra} to {} profile(s).",
                                target_ids.len()
                            )));
                        }
                        Err(e) => {
                            let _ = tx.send(ModBgEvent::Error(format!(
                                "Install failed for {title}: {e}"
                            )));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::Error(format!(
                        "Failed to fetch versions for {title}: {e}"
                    )));
                }
            }
        });
    }

    fn install_specific_version(
        &self,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        selected: &Profile,
        version: &ModrinthVersion,
        title: &str,
    ) {
        let mut target_ids = vec![selected.id.clone()];
        if self.install_all_bots {
            for p in &profiles.profiles {
                if p.kind == ProfileKind::Bot && !target_ids.contains(&p.id) {
                    target_ids.push(p.id.clone());
                }
            }
        }
        let version = version.clone();
        let title = title.to_string();
        let mc = selected.mc_version.clone();
        let loader = selected.mod_loader.clone();
        let paths = paths.clone();
        let tx = self.tx.clone();

        rt.spawn(async move {
            let client = ModrinthClient::new();
            match client
                .install_with_dependencies(&paths, &target_ids, &version, Some(&mc), Some(&loader))
                .await
            {
                Ok(files) => {
                    let extra = if files.len() > 1 {
                        format!(" (+{} dep(s))", files.len() - 1)
                    } else {
                        String::new()
                    };
                    let _ = tx.send(ModBgEvent::Installed(format!(
                        "Installed {title} {}{extra} to {} profile(s).",
                        version.version_number,
                        target_ids.len()
                    )));
                }
                Err(e) => {
                    let _ = tx.send(ModBgEvent::Error(format!("Install failed: {e}")));
                }
            }
        });
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
    ) {
        if profiles.profiles.is_empty() {
            theme::glass_card(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new("No profiles available")
                            .size(16.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Create a profile in Instances first to browse and install Modrinth mods.",
                        )
                        .color(palette.text_muted),
                    );
                    ui.add_space(24.0);
                });
            });
            return;
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
            return;
        };

        let installed = list_installed_mods(paths, &selected.id).unwrap_or_default();

        // Top Store Header
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Modrinth Store")
                        .size(26.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.label(
                    RichText::new("Discover, install, and sync mods across your player and bot profiles.")
                        .size(12.5)
                        .color(palette.text_muted),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.install_all_bots, "Sync to all Bots");

                let prev = self.selected_profile_id.clone();
                egui::ComboBox::from_id_salt("mod_target_profile")
                    .selected_text(format!(
                        "{} • MC {} • {}",
                        selected.name, selected.mc_version, selected.mod_loader
                    ))
                    .show_ui(ui, |ui| {
                        for p in &profiles.profiles {
                            ui.selectable_value(
                                &mut self.selected_profile_id,
                                Some(p.id.clone()),
                                format!("{} • MC {} • {}", p.name, p.mc_version, p.mod_loader),
                            );
                        }
                    });
                if self.selected_profile_id != prev {
                    self.loaded_once = false;
                    self.page = 0;
                    self.active_project = None;
                }
            });
        });

        ui.add_space(10.0);

        // Segmented Discover / Installed Switcher
        let installed_label = format!("Installed ({})", installed.len());
        ui.horizontal(|ui| {
            theme::segmented_control(
                ui,
                palette,
                &mut self.sub_tab,
                &[
                    (ModsSubTab::Discover, "Discover"),
                    (ModsSubTab::Installed, installed_label.as_str()),
                ],
            );
        });

        ui.add_space(12.0);

        match self.sub_tab {
            ModsSubTab::Installed => {
                self.ui_installed(ui, palette, paths, &selected, &installed);
            }
            ModsSubTab::Discover => {
                if self.detail_loading {
                    theme::glass_card(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("Loading project details from Modrinth...")
                                    .color(palette.text_secondary),
                            );
                        });
                    });
                    return;
                }

                if let Some(project) = self.active_project.clone() {
                    self.ui_project_detail(ui, palette, rt, paths, profiles, &selected, &project);
                    return;
                }

                if !self.loaded_once && !self.loading {
                    self.fetch_catalog(rt, &selected.mc_version, &selected.mod_loader, false);
                }

                self.ui_discover(ui, palette, rt, paths, profiles, &selected);
            }
        }
    }

    fn ui_installed(
        &self,
        ui: &mut egui::Ui,
        palette: &Palette,
        paths: &AppPaths,
        selected: &Profile,
        installed: &[mub_core::InstalledMod],
    ) {
        if installed.is_empty() {
            theme::glass_card(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new("No mods installed")
                            .size(16.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Switch to Discover to browse Modrinth mods for {} ({}).",
                            selected.name, selected.mc_version
                        ))
                        .color(palette.text_muted),
                    );
                    ui.add_space(24.0);
                });
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for m in installed {
                theme::glass_card(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let status_color = if m.enabled {
                            palette.accent_emerald
                        } else {
                            palette.text_muted
                        };
                        theme::status_dot(ui, status_color, m.enabled);
                        ui.label(
                            RichText::new(&m.filename)
                                .monospace()
                                .size(13.0)
                                .color(palette.text_primary),
                        );
                        theme::pill_badge(
                            ui,
                            if m.enabled { "ENABLED" } else { "DISABLED" },
                            status_color,
                        );
                        ui.label(
                            RichText::new(format!("{:.1} KB", m.size_bytes as f64 / 1024.0))
                                .size(11.5)
                                .color(palette.text_muted),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::pill_button(ui, palette, "Remove", ButtonTone::Subtle)
                                .clicked()
                            {
                                let _ = remove_installed_mod(paths, &selected.id, &m.filename);
                            }
                            let toggle_label = if m.enabled { "Disable" } else { "Enable" };
                            if theme::pill_button(ui, palette, toggle_label, ButtonTone::Secondary)
                                .clicked()
                            {
                                let _ = toggle_installed_mod(paths, &selected.id, &m.filename);
                            }
                        });
                    });
                });
                ui.add_space(8.0);
            }
        });
    }

    fn ui_discover(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        selected: &Profile,
    ) {
        let mut trigger_fetch = false;

        ui.horizontal_top(|ui| {
            // Floating Liquid Glass Left Filter Rail
            theme::glass_rail(ui, palette, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    theme::section_eyebrow(ui, palette, "Environment");
                    ui.add_space(4.0);
                    let prev_env = self.environment;
                    ui.radio_value(&mut self.environment, ModrinthEnvironment::Any, "Any");
                    ui.radio_value(&mut self.environment, ModrinthEnvironment::Client, "Client");
                    ui.radio_value(&mut self.environment, ModrinthEnvironment::Server, "Server");
                    if self.environment != prev_env {
                        self.page = 0;
                        trigger_fetch = true;
                    }

                    ui.add_space(10.0);
                    theme::glass_divider(ui, palette);
                    ui.add_space(2.0);
                    theme::section_eyebrow(ui, palette, "Categories");
                    ui.add_space(6.0);

                    for (slug, label) in MODRINTH_CATEGORIES {
                        let mut active = self.selected_categories.contains(*slug);
                        if ui.checkbox(&mut active, *label).changed() {
                            if active {
                                self.selected_categories.insert((*slug).to_string());
                            } else {
                                self.selected_categories.remove(*slug);
                            }
                            self.page = 0;
                            trigger_fetch = true;
                        }
                    }

                    if !self.selected_categories.is_empty()
                        || self.environment != ModrinthEnvironment::Any
                    {
                        ui.add_space(10.0);
                        if theme::pill_button(ui, palette, "Reset Filters", ButtonTone::Secondary)
                            .clicked()
                        {
                            self.selected_categories.clear();
                            self.environment = ModrinthEnvironment::Any;
                            self.page = 0;
                            trigger_fetch = true;
                        }
                    }
                });
            });

            ui.add_space(12.0);

            // Main Storefront Column
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());

                // Floating Glass Search & Sort Toolbar
                theme::glass_toolbar(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let search_w = (ui.available_width() - 275.0).max(170.0);
                        let (_, cleared, enter_pressed) = theme::glass_search_bar(
                            ui,
                            palette,
                            &mut self.query,
                            "Search Modrinth mods...",
                            search_w,
                        );
                        if cleared {
                            self.page = 0;
                            trigger_fetch = true;
                        }

                        let btn_text = if self.loading { "Loading..." } else { "Search" };
                        let search_clicked = ui
                            .add_enabled_ui(!self.loading, |ui| {
                                theme::pill_button(ui, palette, btn_text, ButtonTone::Primary)
                            })
                            .inner
                            .clicked();

                        if search_clicked || enter_pressed {
                            self.page = 0;
                            trigger_fetch = true;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let prev_sort = self.sort;
                            egui::ComboBox::from_id_salt("mod_sort_box")
                                .selected_text(match self.sort {
                                    ModrinthSort::Downloads => "Sort: Downloads",
                                    ModrinthSort::Relevance => "Sort: Relevance",
                                    ModrinthSort::Updated => "Sort: Updated",
                                    ModrinthSort::Newest => "Sort: Newest",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.sort,
                                        ModrinthSort::Downloads,
                                        "Most Downloaded",
                                    );
                                    ui.selectable_value(
                                        &mut self.sort,
                                        ModrinthSort::Relevance,
                                        "Relevance",
                                    );
                                    ui.selectable_value(
                                        &mut self.sort,
                                        ModrinthSort::Updated,
                                        "Recently Updated",
                                    );
                                    ui.selectable_value(
                                        &mut self.sort,
                                        ModrinthSort::Newest,
                                        "Newest",
                                    );
                                });
                            if self.sort != prev_sort {
                                self.page = 0;
                                trigger_fetch = true;
                            }
                        });
                    });
                });

                ui.add_space(10.0);

                // Result count bar
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Showing {} of {} mods",
                            self.results.len(),
                            format_count(self.total_hits)
                        ))
                        .size(12.0)
                        .color(palette.text_muted),
                    );
                    if self.loading {
                        ui.spinner();
                    }
                });

                ui.add_space(6.0);

                // Mod Cards List (Infinite Scroll)
                let installed_project_ids = list_installed_project_ids(paths, &selected.id);
                let mut detail_to_open: Option<String> = None;
                let mut install_target: Option<(String, String)> = None;
                let mut load_more = false;

                let scroll_out = egui::ScrollArea::vertical().show(ui, |ui| {
                    for hit in &self.results {
                        let is_installed = installed_project_ids.contains(&hit.project_id);
                        theme::glass_card(ui, palette, |ui| {
                            let card_w = ui.available_width();
                            ui.set_width(card_w);
                            let right_col_w = 195.0_f32;
                            let icon_col_w = 68.0_f32;
                            let mid_col_w = (card_w - icon_col_w - right_col_w - 16.0).max(160.0);

                            ui.horizontal_top(|ui| {
                                self.render_icon(ui, palette, &hit.project_id, &hit.title, 56.0);
                                ui.add_space(8.0);

                                // Middle info column (strictly bounded width)
                                ui.allocate_ui(egui::vec2(mid_col_w, 0.0), |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(mid_col_w);
                                        ui.horizontal_wrapped(|ui| {
                                            if ui
                                                .link(
                                                    RichText::new(&hit.title)
                                                        .size(15.5)
                                                        .strong()
                                                        .color(palette.text_primary),
                                                )
                                                .clicked()
                                            {
                                                detail_to_open = Some(hit.project_id.clone());
                                            }
                                            ui.label(
                                                RichText::new(format!("by {}", hit.author))
                                                    .size(12.0)
                                                    .color(palette.text_muted),
                                            );
                                            if is_installed {
                                                theme::pill_badge(
                                                    ui,
                                                    "INSTALLED",
                                                    palette.accent_emerald,
                                                );
                                            }
                                        });

                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&hit.description)
                                                    .size(12.0)
                                                    .color(palette.text_secondary),
                                            )
                                            .wrap(),
                                        );

                                        ui.add_space(4.0);
                                        ui.horizontal_wrapped(|ui| {
                                            for cat in hit.categories.iter().take(4) {
                                                theme::pill_badge(ui, cat, palette.accent_blue);
                                            }
                                        });
                                    });
                                });

                                // Right stats & action column
                                ui.allocate_ui(egui::vec2(right_col_w, 0.0), |ui| {
                                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} dl",
                                                    format_count(hit.downloads)
                                                ))
                                                .size(12.0)
                                                .strong()
                                                .color(palette.text_primary),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "• {} fav",
                                                    format_count(hit.follows)
                                                ))
                                                .size(11.5)
                                                .color(palette.text_muted),
                                            );
                                        });
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            let btn_label = if is_installed {
                                                "Reinstall"
                                            } else {
                                                "Install"
                                            };
                                            if theme::pill_button(
                                                ui,
                                                palette,
                                                btn_label,
                                                ButtonTone::Primary,
                                            )
                                            .clicked()
                                            {
                                                install_target = Some((
                                                    hit.project_id.clone(),
                                                    hit.title.clone(),
                                                ));
                                            }
                                            if theme::pill_button(
                                                ui,
                                                palette,
                                                "Details",
                                                ButtonTone::Secondary,
                                            )
                                            .clicked()
                                            {
                                                detail_to_open = Some(hit.project_id.clone());
                                            }
                                        });
                                    });
                                });
                            });
                        });
                        ui.add_space(8.0);
                    }

                    if !self.loading && self.results.len() < self.total_hits as usize {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            if theme::pill_button(
                                ui,
                                palette,
                                "Load More Mods",
                                ButtonTone::Secondary,
                            )
                            .clicked()
                            {
                                load_more = true;
                            }
                        });
                    } else if self.loading && !self.results.is_empty() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("Loading more mods...").color(palette.text_muted),
                            );
                        });
                    }
                });

                // Infinite scroll trigger when near bottom of scroll area
                if !self.loading
                    && self.results.len() < self.total_hits as usize
                    && scroll_out.content_size.y > 0.0
                    && (scroll_out.state.offset.y + scroll_out.inner_rect.height() + 260.0)
                        >= scroll_out.content_size.y
                {
                    load_more = true;
                }

                if let Some(pid) = detail_to_open {
                    self.open_project_detail(
                        rt,
                        &pid,
                        &selected.mc_version,
                        &selected.mod_loader,
                    );
                }
                if let Some((pid, title)) = install_target {
                    self.install_latest(rt, paths, profiles, selected, &pid, &title);
                }
                if load_more && !self.loading {
                    self.page += 1;
                    self.fetch_catalog(rt, &selected.mc_version, &selected.mod_loader, true);
                }
            });
        });

        if trigger_fetch && !self.loading {
            self.fetch_catalog(rt, &selected.mc_version, &selected.mod_loader, false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ui_project_detail(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        profiles: &ProfileStore,
        selected: &Profile,
        project: &ModrinthProject,
    ) {
        if theme::pill_button(ui, palette, "< Back to Discover", ButtonTone::Secondary).clicked() {
            self.active_project = None;
            return;
        }
        ui.add_space(10.0);

        let installed_ids = list_installed_project_ids(paths, &selected.id);
        let is_installed = installed_ids.contains(&project.id);

        // Hero Header Glass Card
        theme::glass_elevated(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                self.render_icon(ui, palette, &project.id, &project.title, 76.0);
                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&project.title)
                                .size(21.0)
                                .strong()
                                .color(palette.text_primary),
                        );
                        if is_installed {
                            theme::pill_badge(ui, "INSTALLED", palette.accent_emerald);
                        }
                    });
                    ui.label(
                        RichText::new(&project.description)
                            .size(13.0)
                            .color(palette.text_secondary),
                    );
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        theme::pill_badge(
                            ui,
                            &format!("{} Downloads", format_count(project.downloads)),
                            palette.accent_emerald,
                        );
                        theme::pill_badge(
                            ui,
                            &format!("{} Followers", format_count(project.followers)),
                            palette.accent_blue,
                        );
                        for cat in project.categories.iter().take(4) {
                            theme::pill_badge(ui, cat, palette.text_muted);
                        }
                    });
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(latest) = self.active_versions.first()
                        && theme::pill_button(
                            ui,
                            palette,
                            "Install Latest",
                            ButtonTone::Primary,
                        )
                        .clicked()
                    {
                        self.install_specific_version(
                            rt,
                            paths,
                            profiles,
                            selected,
                            latest,
                            &project.title,
                        );
                    }
                });
            });
        });

        ui.add_space(10.0);

        // Detail Segmented Sub-tabs
        let versions_label = format!("Versions ({})", self.active_versions.len());
        ui.horizontal(|ui| {
            theme::segmented_control(
                ui,
                palette,
                &mut self.detail_tab,
                &[
                    (DetailTab::Overview, "Overview & Gallery"),
                    (DetailTab::Versions, versions_label.as_str()),
                ],
            );
        });
        ui.add_space(10.0);

        match self.detail_tab {
            DetailTab::Overview => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Links row
                    if project.source_url.is_some()
                        || project.wiki_url.is_some()
                        || project.issues_url.is_some()
                    {
                        ui.horizontal(|ui| {
                            if let Some(url) = &project.source_url {
                                ui.hyperlink_to("Source Code", url);
                            }
                            if let Some(url) = &project.wiki_url {
                                ui.hyperlink_to("Wiki", url);
                            }
                            if let Some(url) = &project.issues_url {
                                ui.hyperlink_to("Issues", url);
                            }
                        });
                        ui.add_space(10.0);
                    }

                    // Gallery Screenshots
                    if !project.gallery.is_empty() {
                        theme::section_eyebrow(ui, palette, "Gallery");
                        ui.add_space(6.0);
                        egui::ScrollArea::horizontal().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (idx, item) in project.gallery.iter().take(6).enumerate() {
                                    let key = format!("gallery_{}_{idx}", project.id);
                                    theme::glass_card(ui, palette, |ui| {
                                        if let Some(tex) = self.textures.get(&key) {
                                            ui.add(
                                                egui::Image::new(tex)
                                                    .fit_to_exact_size(egui::vec2(260.0, 150.0))
                                                    .corner_radius(CornerRadius::same(10)),
                                            );
                                        } else {
                                            ui.set_min_size(egui::vec2(260.0, 150.0));
                                            ui.centered_and_justified(|ui| {
                                                ui.spinner();
                                            });
                                        }
                                        if let Some(t) = &item.title {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(t)
                                                    .size(11.5)
                                                    .strong()
                                                    .color(palette.text_primary),
                                            );
                                        }
                                    });
                                }
                            });
                        });
                        ui.add_space(12.0);
                    }

                    // Project Body
                    theme::glass_card(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        theme::section_eyebrow(ui, palette, "Description");
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(&project.body)
                                .size(12.5)
                                .color(palette.text_primary),
                        );
                    });
                });
            }
            DetailTab::Versions => {
                if self.active_versions.is_empty() {
                    theme::glass_card(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(format!(
                                "No versions available for MC {} ({}).",
                                selected.mc_version, selected.mod_loader
                            ))
                            .color(palette.text_muted),
                        );
                    });
                    return;
                }

                let mut version_to_install = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for ver in &self.active_versions {
                        theme::glass_card(ui, palette, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&ver.name)
                                                .size(14.0)
                                                .strong()
                                                .color(palette.text_primary),
                                        );
                                        let type_color = match ver.version_type.as_str() {
                                            "release" => palette.accent_emerald,
                                            "beta" => palette.accent_amber,
                                            _ => palette.accent_red,
                                        };
                                        theme::pill_badge(
                                            ui,
                                            &ver.version_type.to_uppercase(),
                                            type_color,
                                        );
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  •  MC: {}  •  Loaders: {}",
                                            ver.version_number,
                                            ver.game_versions.join(", "),
                                            ver.loaders.join(", ")
                                        ))
                                        .size(11.5)
                                        .color(palette.text_muted),
                                    );
                                });

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if theme::pill_button(
                                            ui,
                                            palette,
                                            "Install",
                                            ButtonTone::Primary,
                                        )
                                        .clicked()
                                        {
                                            version_to_install = Some(ver.clone());
                                        }
                                    },
                                );
                            });
                        });
                        ui.add_space(8.0);
                    }
                });

                if let Some(ver) = version_to_install {
                    self.install_specific_version(
                        rt,
                        paths,
                        profiles,
                        selected,
                        &ver,
                        &project.title,
                    );
                }
            }
        }
    }

    fn render_icon(
        &self,
        ui: &mut egui::Ui,
        palette: &Palette,
        key: &str,
        title: &str,
        size: f32,
    ) {
        if let Some(tex) = self.textures.get(key) {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(size, size))
                    .corner_radius(CornerRadius::same(14)),
            );
        } else {
            let initial = title.chars().next().unwrap_or('M').to_ascii_uppercase();
            egui::Frame::new()
                .fill(palette.well_fill)
                .stroke(Stroke::new(1.0_f32, palette.border_subtle))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(size, size));
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(initial.to_string())
                                .size(size * 0.42)
                                .strong()
                                .color(palette.accent_blue),
                        );
                    });
                });
        }
    }
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
