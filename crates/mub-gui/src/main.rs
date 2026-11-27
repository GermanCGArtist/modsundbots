#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod instance_view;
mod markdown;
mod mods_view;
mod theme;
mod wall_view;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Align2, Color32, CornerRadius, Id, Margin, Order, RichText, Stroke};
use instance_view::{
    InstanceAction, InstanceDetailState, LogLevelFilter, format_playtime, log_line_color,
    matches_log_filter, render_blinking_cursor,
};
use mods_view::{ModsAction, ModsViewState};
use mub_core::mojang::VersionEntry;
use mub_core::{
    APP_VERSION, Account, AccountRole, AccountStore, AppPaths, BotCollection, BotCollectionStore,
    CatalogClient, ContentKind, CurseForgeClient, DeviceCodeResponse, InstalledUpdateResult,
    InstanceManager, InstanceState, InstanceStatus, JavaInstallation, LauncherSettings,
    LoaderVersionInfo, ModLoader, ModpackImportResult, MsaAuthClient, Profile, ProfileKind,
    ProfileStore, UpdateCheckResult, VersionManager, check_for_updates, cleanup_old_update_files,
    detect_java_installations, download_and_install_update, import_curseforge_pack, import_mrpack,
    list_installed_content, open_in_file_manager, spawn_updated_relaunch,
};
use theme::{ButtonTone, NavIcon, Palette, ThemePreset};
use wall_view::{WallAction, WallState};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastTone {
    Focus,
    Mr,
    Cf,
    Bot,
    Player,
    Warn,
    Meta,
    Danger,
}

impl ToastTone {
    fn color(self, p: &Palette) -> Color32 {
        match self {
            ToastTone::Focus => p.focus,
            ToastTone::Mr => p.mr,
            ToastTone::Cf => p.cf,
            ToastTone::Bot => p.bot,
            ToastTone::Player => p.player,
            ToastTone::Warn => p.warn,
            ToastTone::Meta => p.meta,
            ToastTone::Danger => p.danger,
        }
    }

    fn is_sticky(self) -> bool {
        matches!(self, ToastTone::Danger)
    }
}

struct ToastItem {
    id: u64,
    tone: ToastTone,
    message: String,
    created_at: Instant,
    sticky: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoaderChoice {
    Vanilla,
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

impl LoaderChoice {
    pub fn label(self) -> &'static str {
        match self {
            LoaderChoice::Vanilla => "Vanilla",
            LoaderChoice::Fabric => "Fabric",
            LoaderChoice::Quilt => "Quilt",
            LoaderChoice::NeoForge => "NeoForge",
            LoaderChoice::Forge => "Forge",
        }
    }

    pub fn to_mod_loader(self, version: &str) -> ModLoader {
        let v = version.trim().to_string();
        match self {
            LoaderChoice::Vanilla => ModLoader::Vanilla,
            LoaderChoice::Fabric => ModLoader::Fabric { loader_version: v },
            LoaderChoice::Quilt => ModLoader::Quilt { loader_version: v },
            LoaderChoice::NeoForge => ModLoader::NeoForge { loader_version: v },
            LoaderChoice::Forge => ModLoader::Forge { loader_version: v },
        }
    }
}

enum VersionBgEvent {
    McVersionsLoaded(Vec<VersionEntry>),
    McVersionsError(String),
    LoaderVersionsLoaded {
        mc_version: String,
        choice: LoaderChoice,
        versions: Vec<LoaderVersionInfo>,
    },
    LoaderVersionsError {
        mc_version: String,
        choice: LoaderChoice,
        error: String,
    },
}

pub struct VersionPickerCache {
    mc_versions: Vec<VersionEntry>,
    mc_loading: bool,
    mc_loaded: bool,
    mc_error: Option<String>,

    loader_cache: HashMap<(String, LoaderChoice), Vec<LoaderVersionInfo>>,
    loader_loading: HashSet<(String, LoaderChoice)>,
    loader_errors: HashMap<(String, LoaderChoice), String>,

    tx: mpsc::UnboundedSender<VersionBgEvent>,
    rx: mpsc::UnboundedReceiver<VersionBgEvent>,
}

impl VersionPickerCache {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            mc_versions: Vec::new(),
            mc_loading: false,
            mc_loaded: false,
            mc_error: None,
            loader_cache: HashMap::new(),
            loader_loading: HashSet::new(),
            loader_errors: HashMap::new(),
            tx,
            rx,
        }
    }

    pub fn drain(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                VersionBgEvent::McVersionsLoaded(list) => {
                    self.mc_loading = false;
                    self.mc_loaded = true;
                    self.mc_error = None;
                    self.mc_versions = list;
                }
                VersionBgEvent::McVersionsError(e) => {
                    self.mc_loading = false;
                    self.mc_error = Some(e);
                }
                VersionBgEvent::LoaderVersionsLoaded {
                    mc_version,
                    choice,
                    versions,
                } => {
                    let key = (mc_version, choice);
                    self.loader_loading.remove(&key);
                    self.loader_errors.remove(&key);
                    self.loader_cache.insert(key, versions);
                }
                VersionBgEvent::LoaderVersionsError {
                    mc_version,
                    choice,
                    error,
                } => {
                    let key = (mc_version, choice);
                    self.loader_loading.remove(&key);
                    self.loader_errors.insert(key, error);
                }
            }
        }
    }

    pub fn ensure_mc_loaded(&mut self, rt: &Arc<tokio::runtime::Runtime>, ctx: &egui::Context) {
        if self.mc_loaded || self.mc_loading || self.mc_error.is_some() {
            return;
        }
        self.fetch_mc_versions(rt, ctx);
    }

    fn fetch_mc_versions(&mut self, rt: &Arc<tokio::runtime::Runtime>, ctx: &egui::Context) {
        self.mc_loading = true;
        self.mc_error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        rt.spawn(async move {
            let vm = VersionManager::new();
            match vm.fetch_manifest().await {
                Ok(manifest) => {
                    let _ = tx.send(VersionBgEvent::McVersionsLoaded(manifest.versions));
                }
                Err(e) => {
                    let _ = tx.send(VersionBgEvent::McVersionsError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn ensure_loader_loaded(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        mc_version: &str,
        choice: LoaderChoice,
    ) {
        if matches!(choice, LoaderChoice::Vanilla | LoaderChoice::Forge) {
            return;
        }
        let mc = mc_version.trim();
        if mc.is_empty() {
            return;
        }
        let key = (mc.to_string(), choice);
        if self.loader_cache.contains_key(&key)
            || self.loader_loading.contains(&key)
            || self.loader_errors.contains_key(&key)
        {
            return;
        }
        self.fetch_loader_versions(rt, ctx, mc, choice);
    }

    fn fetch_loader_versions(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        mc_version: &str,
        choice: LoaderChoice,
    ) {
        let key = (mc_version.to_string(), choice);
        self.loader_loading.insert(key.clone());
        self.loader_errors.remove(&key);
        let mc = mc_version.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            let vm = VersionManager::new();
            let res = match choice {
                LoaderChoice::Fabric => vm.fetch_fabric_loader_versions(&mc).await,
                LoaderChoice::Quilt => vm.fetch_quilt_loaders(&mc).await,
                LoaderChoice::NeoForge => vm.fetch_neoforge_versions(&mc).await,
                LoaderChoice::Vanilla | LoaderChoice::Forge => Ok(Vec::new()),
            };
            match res {
                Ok(versions) => {
                    let _ = tx.send(VersionBgEvent::LoaderVersionsLoaded {
                        mc_version: mc,
                        choice,
                        versions,
                    });
                }
                Err(e) => {
                    let _ = tx.send(VersionBgEvent::LoaderVersionsError {
                        mc_version: mc,
                        choice,
                        error: e.to_string(),
                    });
                }
            }
            ctx.request_repaint();
        });
    }

    /// Renders the searchable live Mojang version picker with Release/Snapshot filter.
    /// Returns `true` when `mc_version` changed.
    #[allow(clippy::too_many_arguments)]
    pub fn render_mc_picker(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        id_salt: &str,
        mc_version: &mut String,
        show_snapshots: &mut bool,
        search: &mut String,
    ) -> bool {
        let mut changed = false;
        let ctx = ui.ctx().clone();
        self.ensure_mc_loaded(rt, &ctx);

        ui.horizontal_wrapped(|ui| {
            let prev = mc_version.clone();
            let display_lbl = if mc_version.is_empty() {
                "<Select MC>".to_string()
            } else {
                mc_version.clone()
            };

            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(display_lbl)
                .width(160.0)
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(search)
                            .hint_text("filter version...")
                            .font(egui::FontId::monospace(11.5))
                            .desired_width(145.0),
                    );
                    ui.separator();
                    let q = search.trim().to_lowercase();
                    let mut count = 0usize;
                    for entry in &self.mc_versions {
                        if !*show_snapshots && entry.kind != "release" {
                            continue;
                        }
                        if !q.is_empty() && !entry.id.to_lowercase().contains(&q) {
                            continue;
                        }
                        let tag = if entry.kind == "release" {
                            entry.id.clone()
                        } else {
                            format!("{} ({})", entry.id, entry.kind)
                        };
                        ui.selectable_value(mc_version, entry.id.clone(), tag);
                        count += 1;
                        if count >= 80 {
                            break;
                        }
                    }
                    if count == 0 {
                        ui.label(
                            RichText::new("No matching versions")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                    }
                });

            theme::term_toggle(ui, palette, show_snapshots, "SNAPSHOTS", palette.meta);

            if self.mc_loading {
                ui.spinner();
            } else if let Some(err) = &self.mc_error {
                ui.label(
                    RichText::new(format!("Err: {err}"))
                        .monospace()
                        .size(10.5)
                        .color(palette.danger),
                );
                if theme::term_button(ui, palette, "RETRY", ButtonTone::Secondary).clicked() {
                    self.fetch_mc_versions(rt, &ctx);
                }
            }

            if *mc_version != prev {
                changed = true;
            }
        });

        changed
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_loader_picker(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        id_salt: &str,
        mc_version: &str,
        choice: &mut LoaderChoice,
        loader_version: &mut String,
        allow_forge_preserve: bool,
    ) {
        let ctx = ui.ctx().clone();
        let prev_choice = *choice;

        ui.horizontal_wrapped(|ui| {
            let mut choices = vec![
                LoaderChoice::Vanilla,
                LoaderChoice::Fabric,
                LoaderChoice::Quilt,
                LoaderChoice::NeoForge,
            ];
            if allow_forge_preserve && *choice == LoaderChoice::Forge {
                choices.push(LoaderChoice::Forge);
            }
            for c in choices {
                let active = *choice == c;
                if theme::term_button(
                    ui,
                    palette,
                    c.label(),
                    if active {
                        ButtonTone::Focus
                    } else {
                        ButtonTone::Subtle
                    },
                )
                .clicked()
                {
                    *choice = c;
                }
            }
        });

        if *choice != prev_choice {
            loader_version.clear();
            self.ensure_loader_loaded(rt, &ctx, mc_version, *choice);
        }

        match *choice {
            LoaderChoice::Vanilla => {
                ui.label(
                    RichText::new("No mod loader (pure Vanilla client).")
                        .monospace()
                        .size(11.0)
                        .color(palette.ink_muted),
                );
            }
            LoaderChoice::Forge => {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("FORGE VER:")
                            .monospace()
                            .size(11.0)
                            .color(palette.meta),
                    );
                    ui.add(
                        egui::TextEdit::singleline(loader_version)
                            .font(egui::FontId::monospace(11.5))
                            .desired_width(180.0),
                    );
                });
            }
            LoaderChoice::Fabric | LoaderChoice::Quilt | LoaderChoice::NeoForge => {
                self.ensure_loader_loaded(rt, &ctx, mc_version, *choice);
                let key = (mc_version.trim().to_string(), *choice);
                let loading = self.loader_loading.contains(&key);
                let error = self.loader_errors.get(&key).cloned();
                let versions = self.loader_cache.get(&key).cloned();

                ui.add_space(4.0);
                if loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!(
                                "Fetching {} versions for MC {}...",
                                choice.label(),
                                mc_version
                            ))
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_dim),
                        );
                    });
                } else if let Some(err) = error {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("{} fetch failed: {err}", choice.label()))
                                .monospace()
                                .size(11.0)
                                .color(palette.danger),
                        );
                        if theme::term_button(ui, palette, "RETRY", ButtonTone::Warn).clicked() {
                            self.fetch_loader_versions(rt, &ctx, mc_version, *choice);
                        }
                    });
                } else if let Some(vers) = versions {
                    if vers.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "No {} builds available for MC {}. Pick another version or loader.",
                                choice.label(),
                                mc_version
                            ))
                            .monospace()
                            .size(11.0)
                            .color(palette.warn),
                        );
                    } else {
                        // Auto-select first stable (or first) version if current selection is empty/absent
                        if loader_version.is_empty()
                            || !vers.iter().any(|v| v.version == *loader_version)
                        {
                            let preferred = vers
                                .iter()
                                .find(|v| v.stable)
                                .or_else(|| vers.first())
                                .map(|v| v.version.clone())
                                .unwrap_or_default();
                            *loader_version = preferred;
                        }

                        egui::ComboBox::from_id_salt(id_salt)
                            .selected_text(loader_version.as_str())
                            .width(230.0)
                            .show_ui(ui, |ui| {
                                for v in vers.iter().take(60) {
                                    let lbl = if v.stable {
                                        format!("{} (stable)", v.version)
                                    } else {
                                        v.version.clone()
                                    };
                                    ui.selectable_value(loader_version, v.version.clone(), lbl);
                                }
                            });
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Instances,
    InstanceDetail,
    Collections,
    Discover,
    Accounts,
    Console,
    Wall,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileFilter {
    All,
    Players,
    Bots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CfKeyStatus {
    Unset,
    Untested,
    Testing,
    Valid,
    Invalid,
}

enum BgEvent {
    DeviceCodeReady(DeviceCodeResponse, AccountRole),
    AccountAdded(Account),
    AccountRefreshed(Account),
    StatusSnapshot(Vec<InstanceStatus>),
    PackImported(ModpackImportResult),
    ProfileUpdatesChecked(HashMap<String, usize>),
    CfKeyTested(Result<bool, String>),
    AppUpdateChecked {
        silent: bool,
        res: Result<UpdateCheckResult, String>,
    },
    AppUpdateInstalled(Result<InstalledUpdateResult, String>),
    JavasDetected(Vec<JavaInstallation>),
    TaskProgress(Option<(f32, String)>),
    Toast(ToastTone, String),
}

struct MubApp {
    rt: Arc<tokio::runtime::Runtime>,
    paths: AppPaths,
    settings: LauncherSettings,
    accounts: AccountStore,
    profiles: ProfileStore,
    collections: BotCollectionStore,
    manager: InstanceManager,

    // External-change sync (e.g. from MCP server)
    last_store_check: Instant,
    accounts_mtime: Option<SystemTime>,
    profiles_mtime: Option<SystemTime>,
    collections_mtime: Option<SystemTime>,

    // Bot Collections UI state
    show_new_collection: bool,
    new_col_name: String,
    new_col_desc: String,
    new_col_stagger: String,
    new_col_include_all_bots: bool,
    editing_collection_id: Option<String>,
    edit_col_name: String,
    edit_col_desc: String,
    edit_col_stagger: String,

    theme_preset: ThemePreset,
    active_view: ActiveView,
    target_profile_id: Option<String>,
    profile_filter: ProfileFilter,
    search_query: String,
    selected_profiles: HashSet<String>,

    // Live statuses + playtime tracking
    statuses: HashMap<String, InstanceStatus>,
    running_sessions: HashSet<String>,
    profile_update_counts: HashMap<String, usize>,
    checking_all_updates: bool,

    // Toast queue & background progress
    toasts: VecDeque<ToastItem>,
    next_toast_id: u64,
    task_progress: Option<(f32, String)>,

    // Command Palette overlay (Ctrl+K)
    show_cmd_palette: bool,
    cmd_query: String,

    // Shared Mojang & Loader live picker cache
    version_cache: VersionPickerCache,

    // Inline New Profile drawer
    show_new_profile: bool,
    new_prof_name: String,
    new_prof_kind: ProfileKind,
    new_prof_group: String,
    new_prof_mc_version: String,
    new_prof_show_snapshots: bool,
    new_prof_mc_search: String,
    new_prof_loader_choice: LoaderChoice,
    new_prof_loader_ver: String,
    new_prof_account_id: Option<String>,
    new_prof_server: String,
    new_prof_ram_max: u32,

    // Inline Import Pack drawer
    show_import_pack: bool,
    import_pack_path: String,
    import_pack_name: String,
    import_pack_kind: ProfileKind,
    importing_pack: bool,

    // Instance detail view
    instance_detail: InstanceDetailState,

    // Discover view
    mods_view: ModsViewState,

    // Accounts state
    new_offline_name: String,
    new_offline_role: AccountRole,
    pending_device_code: Option<(DeviceCodeResponse, AccountRole)>,
    pending_live_auth: Option<AccountRole>,
    live_auth_code_input: String,
    confirm_delete_account: Option<String>,

    // Bot Wall (live thumbnails of running instances)
    wall: WallState,

    // Console state
    console_profile_id: Option<String>,
    console_filter: LogLevelFilter,
    console_search: String,

    // Config state
    show_cf_key: bool,
    cf_key_status: CfKeyStatus,
    show_gh_token: bool,
    app_update_check: Option<UpdateCheckResult>,
    app_update_error: Option<String>,
    checking_app_update: bool,
    installing_app_update: bool,
    app_update_installed: Option<InstalledUpdateResult>,
    detected_javas: Vec<JavaInstallation>,
    detecting_javas: bool,

    tx: mpsc::UnboundedSender<BgEvent>,
    rx: mpsc::UnboundedReceiver<BgEvent>,
}

impl MubApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rt = Arc::new(
            tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime"),
        );
        let paths = AppPaths::default_root().expect("Failed to resolve app data dir");
        let _ = paths.ensure_dirs();
        cleanup_old_update_files();
        let settings = LauncherSettings::load(&paths.settings_file()).unwrap_or_default();
        let accounts = AccountStore::load(&paths.accounts_file()).unwrap_or_default();
        let profiles = ProfileStore::load(&paths.profiles_file()).unwrap_or_default();
        let collections = BotCollectionStore::load(&paths.collections_file()).unwrap_or_default();
        let accounts_mtime = file_mtime(&paths.accounts_file());
        let profiles_mtime = file_mtime(&paths.profiles_file());
        let collections_mtime = file_mtime(&paths.collections_file());

        let theme_preset = ThemePreset::from_id(&settings.theme_palette);
        theme::apply_theme(&cc.egui_ctx, theme_preset);

        let first_profile_id = profiles.profiles.first().map(|p| p.id.clone());
        let manager = InstanceManager::new();
        let (tx, rx) = mpsc::unbounded_channel();

        // Background status poller
        let mgr_clone = manager.clone();
        let tx_clone = tx.clone();
        let ctx_clone = cc.egui_ctx.clone();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(350)).await;
                let list = mgr_clone.list_statuses().await;
                if tx_clone.send(BgEvent::StatusSnapshot(list)).is_err() {
                    break;
                }
                ctx_clone.request_repaint();
            }
        });

        // Re-attach to games started earlier (mub closed or crashed while they ran) and pick up
        // games started by the CLI or MCP server, so they appear on the Wall too.
        let mgr_adopt = manager.clone();
        let paths_adopt = paths.clone();
        let tx_adopt = tx.clone();
        let ctx_adopt = cc.egui_ctx.clone();
        rt.spawn(async move {
            let mut first = true;
            loop {
                let n = mgr_adopt.adopt_running(&paths_adopt).await;
                if n > 0 {
                    let msg = if first {
                        format!("Re-attached to {n} running instance(s)")
                    } else {
                        format!("Picked up {n} instance(s) started outside the GUI")
                    };
                    let _ = tx_adopt.send(BgEvent::Toast(ToastTone::Bot, msg));
                    ctx_adopt.request_repaint();
                }
                first = false;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        // Initial background Java detection so dropdowns are ready
        let tx_java = tx.clone();
        let ctx_java = cc.egui_ctx.clone();
        rt.spawn(async move {
            let javas = tokio::task::spawn_blocking(detect_java_installations)
                .await
                .unwrap_or_default();
            let _ = tx_java.send(BgEvent::JavasDetected(javas));
            ctx_java.request_repaint();
        });

        let cf_key_status = if settings.effective_curseforge_api_key().is_some() {
            CfKeyStatus::Untested
        } else {
            CfKeyStatus::Unset
        };

        // Silent startup update check (only notifies if a newer version is available)
        let tx_upd = tx.clone();
        let ctx_upd = cc.egui_ctx.clone();
        let gh_tok = settings.effective_github_update_token();
        rt.spawn(async move {
            let res = check_for_updates(gh_tok.as_deref())
                .await
                .map_err(|e| e.to_string());
            let _ = tx_upd.send(BgEvent::AppUpdateChecked { silent: true, res });
            ctx_upd.request_repaint();
        });

        Self {
            rt,
            paths,
            settings,
            accounts,
            profiles,
            collections,
            manager,
            last_store_check: Instant::now(),
            accounts_mtime,
            profiles_mtime,
            collections_mtime,
            show_new_collection: false,
            new_col_name: String::new(),
            new_col_desc: String::new(),
            new_col_stagger: String::new(),
            new_col_include_all_bots: true,
            editing_collection_id: None,
            edit_col_name: String::new(),
            edit_col_desc: String::new(),
            edit_col_stagger: String::new(),
            theme_preset,
            // MUB_START_VIEW=wall opens the GUI on the Wall (handy for a bot PC / screenshots).
            active_view: match std::env::var("MUB_START_VIEW").as_deref() {
                Ok("wall") => ActiveView::Wall,
                Ok("console") => ActiveView::Console,
                _ => ActiveView::Instances,
            },
            target_profile_id: first_profile_id.clone(),
            profile_filter: ProfileFilter::All,
            search_query: String::new(),
            selected_profiles: HashSet::new(),
            statuses: HashMap::new(),
            running_sessions: HashSet::new(),
            profile_update_counts: HashMap::new(),
            checking_all_updates: false,
            toasts: VecDeque::new(),
            next_toast_id: 1,
            task_progress: None,
            show_cmd_palette: false,
            cmd_query: String::new(),
            version_cache: VersionPickerCache::new(),
            show_new_profile: false,
            new_prof_name: String::new(),
            new_prof_kind: ProfileKind::Bot,
            new_prof_group: String::new(),
            new_prof_mc_version: "1.21.4".to_string(),
            new_prof_show_snapshots: false,
            new_prof_mc_search: String::new(),
            new_prof_loader_choice: LoaderChoice::Fabric,
            new_prof_loader_ver: String::new(),
            new_prof_account_id: None,
            new_prof_server: String::new(),
            new_prof_ram_max: 1024,
            show_import_pack: false,
            import_pack_path: String::new(),
            import_pack_name: String::new(),
            import_pack_kind: ProfileKind::Player,
            importing_pack: false,
            instance_detail: InstanceDetailState::new(first_profile_id.clone().unwrap_or_default()),
            mods_view: ModsViewState::new(first_profile_id.clone()),
            new_offline_name: String::new(),
            new_offline_role: AccountRole::Bot,
            pending_device_code: None,
            pending_live_auth: None,
            live_auth_code_input: String::new(),
            confirm_delete_account: None,
            wall: WallState::new(&cc.egui_ctx),
            console_profile_id: first_profile_id,
            console_filter: LogLevelFilter::All,
            console_search: String::new(),
            show_cf_key: false,
            cf_key_status,
            show_gh_token: false,
            app_update_check: None,
            app_update_error: None,
            checking_app_update: false,
            installing_app_update: false,
            app_update_installed: None,
            detected_javas: Vec::new(),
            detecting_javas: true,
            tx,
            rx,
        }
    }

    fn push_toast(&mut self, tone: ToastTone, message: impl Into<String>) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push_back(ToastItem {
            id,
            tone,
            message: message.into(),
            created_at: Instant::now(),
            sticky: tone.is_sticky(),
        });
        while self.toasts.len() > 8 {
            self.toasts.pop_front();
        }
    }

    fn prune_expired_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts
            .retain(|t| t.sticky || now.duration_since(t.created_at) < Duration::from_secs(6));
        if self.toasts.iter().any(|t| !t.sticky) {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn set_theme_preset(&mut self, ctx: &egui::Context, preset: ThemePreset) {
        self.theme_preset = preset;
        theme::apply_theme(ctx, preset);
        self.settings.theme_palette = preset.id().to_string();
        if let Err(e) = self.settings.save(&self.paths.settings_file()) {
            self.push_toast(
                ToastTone::Danger,
                format!("Failed to save theme to settings.json: {e}"),
            );
        } else {
            self.push_toast(ToastTone::Focus, format!("Palette switched -> {}", preset.name()));
        }
    }

    fn open_instance_detail(&mut self, profile_id: &str) {
        self.target_profile_id = Some(profile_id.to_string());
        if let Some(p) = self.profiles.get(profile_id).cloned() {
            self.instance_detail.open_profile(&p, &self.paths);
        } else {
            self.instance_detail.profile_id = profile_id.to_string();
        }
        self.active_view = ActiveView::InstanceDetail;
    }

    fn drain_events(&mut self) {
        self.version_cache.drain();

        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                BgEvent::DeviceCodeReady(dev, role) => {
                    self.pending_device_code = Some((dev, role));
                }
                BgEvent::AccountAdded(acc) => {
                    self.pending_device_code = None;
                    let tone = if acc.role == AccountRole::Bot {
                        ToastTone::Bot
                    } else {
                        ToastTone::Player
                    };
                    self.push_toast(tone, format!("Signed in as {}", acc.username));
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                }
                BgEvent::AccountRefreshed(acc) => {
                    self.push_toast(ToastTone::Mr, format!("Refreshed token for {}", acc.username));
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                }
                BgEvent::StatusSnapshot(list) => {
                    self.statuses.clear();
                    let mut profiles_dirty = false;

                    for s in list {
                        match &s.state {
                            InstanceState::Running => {
                                if self.running_sessions.insert(s.profile_id.clone())
                                    && let Some(p) = self.profiles.get_mut(&s.profile_id)
                                {
                                    p.last_launched_at = Some(s.started_at);
                                    profiles_dirty = true;
                                }
                            }
                            InstanceState::Stopped { .. } | InstanceState::Failed { .. } => {
                                if self.running_sessions.remove(&s.profile_id)
                                    && let Some(p) = self.profiles.get_mut(&s.profile_id)
                                {
                                    let secs = s.elapsed_secs().max(0) as u64;
                                    p.playtime_secs = p.playtime_secs.saturating_add(secs);
                                    profiles_dirty = true;
                                }
                            }
                            InstanceState::Preparing => {}
                        }
                        self.statuses.insert(s.profile_id.clone(), s);
                    }

                    if profiles_dirty {
                        let _ = self.profiles.save(&self.paths.profiles_file());
                    }
                }
                BgEvent::PackImported(res) => {
                    self.importing_pack = false;
                    self.task_progress = None;
                    let pid = res.profile.id.clone();
                    let pname = res.profile.name.clone();
                    let restricted = res.restricted_files.len();
                    self.profiles.upsert(res.profile);
                    let _ = self.profiles.save(&self.paths.profiles_file());
                    self.target_profile_id = Some(pid);
                    self.show_import_pack = false;
                    self.import_pack_path.clear();
                    self.import_pack_name.clear();
                    self.push_toast(
                        if restricted > 0 {
                            ToastTone::Warn
                        } else {
                            ToastTone::Mr
                        },
                        format!(
                            "Imported pack '{pname}' ({} files, {} overrides{})",
                            res.downloaded_files,
                            res.extracted_overrides,
                            if restricted > 0 {
                                format!(", {restricted} restricted")
                            } else {
                                String::new()
                            }
                        ),
                    );
                }
                BgEvent::ProfileUpdatesChecked(counts) => {
                    self.checking_all_updates = false;
                    self.task_progress = None;
                    let total: usize = counts.values().sum();
                    self.profile_update_counts = counts;
                    self.push_toast(
                        if total > 0 {
                            ToastTone::Warn
                        } else {
                            ToastTone::Mr
                        },
                        if total > 0 {
                            format!("Checked MR/CF hashes across profiles — {total} update(s) available")
                        } else {
                            "All profiles are up to date".to_string()
                        },
                    );
                }
                BgEvent::CfKeyTested(res) => match res {
                    Ok(true) => {
                        self.cf_key_status = CfKeyStatus::Valid;
                        self.mods_view.invalidate_catalog();
                        self.push_toast(
                            ToastTone::Cf,
                            "CurseForge API v1 key verified (Minecraft gameId=432 OK)",
                        );
                    }
                    Ok(false) => {
                        self.cf_key_status = CfKeyStatus::Invalid;
                        self.push_toast(
                            ToastTone::Danger,
                            "CurseForge API key rejected (401/403 Unauthorized)",
                        );
                    }
                    Err(e) => {
                        self.cf_key_status = CfKeyStatus::Invalid;
                        self.push_toast(ToastTone::Danger, format!("CurseForge test failed: {e}"));
                    }
                },
                BgEvent::AppUpdateChecked { silent, res } => {
                    self.checking_app_update = false;
                    match res {
                        Ok(info) => {
                            self.app_update_error = None;
                            let avail = info.update_available;
                            let tag = info.tag_name.clone();
                            self.app_update_check = Some(info);
                            if avail {
                                self.push_toast(
                                    ToastTone::Warn,
                                    format!("Update available: {tag} — open 05 CONFIG to install."),
                                );
                            } else if !silent {
                                self.push_toast(
                                    ToastTone::Focus,
                                    format!("ModsUndBots v{APP_VERSION} is up to date."),
                                );
                            }
                        }
                        Err(e) => {
                            self.app_update_error = Some(e.clone());
                            if !silent {
                                self.push_toast(
                                    ToastTone::Danger,
                                    format!("Update check failed: {e}"),
                                );
                            }
                        }
                    }
                }
                BgEvent::AppUpdateInstalled(res) => {
                    self.installing_app_update = false;
                    self.task_progress = None;
                    match res {
                        Ok(installed) => {
                            let ver = installed.new_version.clone();
                            self.app_update_installed = Some(installed);
                            self.app_update_error = None;
                            self.push_toast(
                                ToastTone::Focus,
                                format!("Updated to v{ver}! Click RESTART NOW in 05 CONFIG."),
                            );
                        }
                        Err(e) => {
                            self.app_update_error = Some(e.clone());
                            self.push_toast(
                                ToastTone::Danger,
                                format!("Update install failed: {e}"),
                            );
                        }
                    }
                }
                BgEvent::JavasDetected(list) => {
                    self.detecting_javas = false;
                    self.detected_javas = list;
                }
                BgEvent::TaskProgress(prog) => {
                    self.task_progress = prog;
                }
                BgEvent::Toast(tone, msg) => {
                    self.importing_pack = false;
                    self.checking_all_updates = false;
                    self.task_progress = None;
                    self.push_toast(tone, msg);
                }
            }
        }
    }

    fn launch_profiles(&mut self, profiles: Vec<Profile>) {
        self.launch_profiles_with_stagger(profiles, None);
    }

    fn launch_profiles_with_stagger(&mut self, profiles: Vec<Profile>, stagger_ms_override: Option<u64>) {
        let mut items = Vec::new();
        for p in profiles {
            let Some(acc_id) = &p.account_id else {
                self.push_toast(
                    ToastTone::Danger,
                    format!("Profile '{}' has no assigned account", p.name),
                );
                continue;
            };
            let Some(acc) = self.accounts.get(acc_id).cloned() else {
                self.push_toast(
                    ToastTone::Danger,
                    format!("Assigned account for '{}' not found", p.name),
                );
                continue;
            };
            items.push((p, acc));
        }

        if items.is_empty() {
            return;
        }

        let count = items.len();
        self.push_toast(
            ToastTone::Focus,
            format!("Preparing launch for {count} profile(s)..."),
        );

        let mgr = self.manager.clone();
        let paths = self.paths.clone();
        let settings = self.settings.clone();
        let tx = self.tx.clone();

        self.rt.spawn(async move {
            let mut refresh_set = tokio::task::JoinSet::new();
            for (idx, (p, acc)) in items.into_iter().enumerate() {
                let msa_client_id = settings.msa_client_id.clone();
                let tx_ref = tx.clone();
                refresh_set.spawn(async move {
                    if acc.is_msa() && !acc.is_token_valid() {
                        let client = MsaAuthClient::new(&msa_client_id);
                        match client.refresh_account(&acc).await {
                            Ok(refreshed) => {
                                let _ = tx_ref.send(BgEvent::AccountRefreshed(refreshed.clone()));
                                Some((idx, (p, refreshed)))
                            }
                            Err(e) => {
                                let _ = tx_ref.send(BgEvent::Toast(
                                    ToastTone::Danger,
                                    format!("Token refresh failed for {}: {e}", acc.username),
                                ));
                                None
                            }
                        }
                    } else {
                        Some((idx, (p, acc)))
                    }
                });
            }
            let mut indexed = Vec::new();
            while let Some(res) = refresh_set.join_next().await {
                if let Ok(Some(pair)) = res {
                    indexed.push(pair);
                }
            }
            indexed.sort_by_key(|(idx, _)| *idx);
            let prepared_items: Vec<_> = indexed.into_iter().map(|(_, item)| item).collect();
            let results = mgr
                .launch_batch_with_stagger(&paths, &settings, prepared_items, stagger_ms_override)
                .await;
            for (id, res) in results {
                if let Err(e) = res {
                    let _ = tx.send(BgEvent::Toast(
                        ToastTone::Danger,
                        format!("Launch failed ({id}): {e}"),
                    ));
                }
            }
        });
    }

    fn launch_collection(&mut self, collection_id: &str) {
        let Some(col) = self.collections.get(collection_id).cloned() else {
            return;
        };
        let to_launch: Vec<Profile> = col
            .profile_ids
            .iter()
            .filter_map(|pid| self.profiles.get(pid))
            .filter(|p| {
                !matches!(
                    self.statuses.get(&p.id).map(|s| &s.state),
                    Some(InstanceState::Running | InstanceState::Preparing)
                )
            })
            .cloned()
            .collect();

        if to_launch.is_empty() {
            self.push_toast(
                ToastTone::Bot,
                format!("No idle bot instances in '{}' to start.", col.name),
            );
            return;
        }

        if let Some(c) = self.collections.get_mut(collection_id) {
            c.mark_started();
        }
        let _ = self.collections.save(&self.paths.collections_file());
        self.collections_mtime = file_mtime(&self.paths.collections_file());

        self.push_toast(
            ToastTone::Bot,
            format!("Starting {} bot(s) in '{}'...", to_launch.len(), col.name),
        );
        self.launch_profiles_with_stagger(to_launch, col.stagger_ms);
    }

    fn stop_collection(&self, collection_id: &str) {
        let Some(col) = self.collections.get(collection_id) else {
            return;
        };
        let ids: Vec<String> = col
            .profile_ids
            .iter()
            .filter(|pid| {
                matches!(
                    self.statuses.get(*pid).map(|s| &s.state),
                    Some(InstanceState::Running | InstanceState::Preparing)
                )
            })
            .cloned()
            .collect();
        let mgr = self.manager.clone();
        self.rt.spawn(async move {
            for id in ids {
                let _ = mgr.stop_profile(&id).await;
            }
        });
    }

    fn sync_external_stores_if_changed(&mut self) {
        if self.last_store_check.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_store_check = Instant::now();

        let acc_m = file_mtime(&self.paths.accounts_file());
        if acc_m != self.accounts_mtime {
            self.accounts_mtime = acc_m;
            if let Ok(loaded) = AccountStore::load(&self.paths.accounts_file()) {
                self.accounts = loaded;
            }
        }
        let prof_m = file_mtime(&self.paths.profiles_file());
        if prof_m != self.profiles_mtime {
            self.profiles_mtime = prof_m;
            if let Ok(loaded) = ProfileStore::load(&self.paths.profiles_file()) {
                self.profiles = loaded;
            }
        }
        let col_m = file_mtime(&self.paths.collections_file());
        if col_m != self.collections_mtime {
            self.collections_mtime = col_m;
            if let Ok(loaded) = BotCollectionStore::load(&self.paths.collections_file()) {
                self.collections = loaded;
            }
        }
    }

    fn launch_all_bots(&mut self) {
        let bots: Vec<Profile> = self
            .profiles
            .profiles
            .iter()
            .filter(|p| {
                p.kind == ProfileKind::Bot
                    && !matches!(
                        self.statuses.get(&p.id).map(|s| &s.state),
                        Some(InstanceState::Running | InstanceState::Preparing)
                    )
            })
            .cloned()
            .collect();
        if bots.is_empty() {
            self.push_toast(ToastTone::Bot, "No idle bot profiles to launch.");
        } else {
            self.launch_profiles(bots);
        }
    }

    fn stop_profile(&self, profile_id: String) {
        let mgr = self.manager.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            if let Err(e) = mgr.stop_profile(&profile_id).await {
                let _ = tx.send(BgEvent::Toast(
                    ToastTone::Danger,
                    format!("Failed to stop {profile_id}: {e}"),
                ));
            } else {
                let _ = tx.send(BgEvent::Toast(
                    ToastTone::Danger,
                    format!("Stopped instance {profile_id}"),
                ));
            }
        });
    }

    fn stop_all(&self) {
        let mgr = self.manager.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let _ = mgr.stop_all().await;
            let _ = tx.send(BgEvent::Toast(
                ToastTone::Danger,
                "Sent stop signal to all active instances.".to_string(),
            ));
        });
    }

    fn spawn_check_all_updates(&mut self, ctx: &egui::Context) {
        if self.checking_all_updates || self.profiles.profiles.is_empty() {
            return;
        }
        self.checking_all_updates = true;
        let profiles = self.profiles.profiles.clone();
        let total_profiles = profiles.len();
        let paths = self.paths.clone();
        let cf_key = self.settings.effective_curseforge_api_key();
        let gh_token = self.settings.effective_github_update_token();
        let gh_repos = self.settings.github_mod_repos.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        let _ = tx.send(BgEvent::TaskProgress(Some((
            0.05,
            format!("Checking updates across {total_profiles} profile(s)..."),
        ))));

        self.rt.spawn(async move {
            let catalog = CatalogClient::with_github(cf_key, gh_token, gh_repos);
            let mut map = HashMap::new();
            let kinds = [
                ContentKind::Mod,
                ContentKind::ResourcePack,
                ContentKind::Shader,
                ContentKind::DataPack,
            ];
            for (idx, p) in profiles.iter().enumerate() {
                let frac = (idx as f32 + 0.5) / total_profiles.max(1) as f32;
                let _ = tx.send(BgEvent::TaskProgress(Some((
                    frac,
                    format!("Checking {} ({}/{})", p.name, idx + 1, total_profiles),
                ))));
                let mut count = 0usize;
                for k in kinds {
                    if let Ok(list) = catalog
                        .identify_and_check_updates(&paths, &p.id, k, &p.mc_version, &p.mod_loader)
                        .await
                    {
                        count += list.len();
                    }
                }
                map.insert(p.id.clone(), count);
            }
            let _ = tx.send(BgEvent::ProfileUpdatesChecked(map));
            ctx.request_repaint();
        });
    }

    fn spawn_import_pack(
        &mut self,
        ctx: &egui::Context,
        pack_path: PathBuf,
        custom_name: Option<String>,
        kind: ProfileKind,
    ) {
        if !pack_path.is_file() {
            self.push_toast(
                ToastTone::Danger,
                format!("Pack file not found: {}", pack_path.display()),
            );
            return;
        }
        let ext = pack_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        let paths = self.paths.clone();
        let max_concurrent = self.settings.max_concurrent_downloads;
        let cf_key = self.settings.effective_curseforge_api_key();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.importing_pack = true;
        self.task_progress = Some((0.15, format!("Importing {}...", pack_path.display())));

        self.rt.spawn(async move {
            let tx_prog = tx.clone();
            let ctx_prog = ctx.clone();
            let on_prog = move |msg: String| {
                let _ = tx_prog.send(BgEvent::TaskProgress(Some((0.55, msg))));
                ctx_prog.request_repaint();
            };

            let res = if ext == "mrpack" {
                import_mrpack(
                    &paths,
                    &pack_path,
                    custom_name.as_deref(),
                    kind,
                    max_concurrent,
                    on_prog,
                )
                .await
            } else if ext == "zip" {
                let Some(key) = cf_key else {
                    let _ = tx.send(BgEvent::Toast(
                        ToastTone::Warn,
                        "Importing CurseForge .zip packs requires a CurseForge API key in 05 CONFIG."
                            .to_string(),
                    ));
                    ctx.request_repaint();
                    return;
                };
                let cf = CurseForgeClient::new(key);
                import_curseforge_pack(
                    &paths,
                    &pack_path,
                    &cf,
                    custom_name.as_deref(),
                    kind,
                    max_concurrent,
                    on_prog,
                )
                .await
            } else {
                let _ = tx.send(BgEvent::Toast(
                    ToastTone::Danger,
                    "Unsupported pack format. Expected .mrpack or CurseForge .zip.".to_string(),
                ));
                ctx.request_repaint();
                return;
            };

            match res {
                Ok(imported) => {
                    let _ = tx.send(BgEvent::PackImported(imported));
                }
                Err(e) => {
                    let _ = tx.send(BgEvent::Toast(
                        ToastTone::Danger,
                        format!("Pack import failed: {e}"),
                    ));
                }
            }
            ctx.request_repaint();
        });
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        for file in dropped {
            let Some(path) = file.path else {
                continue;
            };
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();

            if ext == "mrpack" || ext == "zip" {
                self.spawn_import_pack(ctx, path, None, ProfileKind::Player);
            } else if ext == "jar" {
                let target_id = self
                    .target_profile_id
                    .clone()
                    .or_else(|| self.profiles.profiles.first().map(|p| p.id.clone()));
                let Some(pid) = target_id else {
                    self.push_toast(
                        ToastTone::Warn,
                        "Create a profile first before dropping .jar mod files.",
                    );
                    continue;
                };
                let Some(fname) = path.file_name() else {
                    continue;
                };
                let mods_dir = self.paths.mods_dir(&pid);
                if let Err(e) = std::fs::create_dir_all(&mods_dir) {
                    self.push_toast(ToastTone::Danger, format!("Failed to create mods/: {e}"));
                    continue;
                }
                let dest = mods_dir.join(fname);
                match std::fs::copy(&path, &dest) {
                    Ok(_) => {
                        let pname = self
                            .profiles
                            .get(&pid)
                            .map(|p| p.name.as_str())
                            .unwrap_or(&pid);
                        self.push_toast(
                            ToastTone::Mr,
                            format!("Copied {} -> {}/mods/", fname.to_string_lossy(), pname),
                        );
                    }
                    Err(e) => {
                        self.push_toast(ToastTone::Danger, format!("Failed to copy .jar: {e}"));
                    }
                }
            } else {
                self.push_toast(
                    ToastTone::Warn,
                    format!(
                        "Unsupported dropped file '{}'. Drop .mrpack, .zip, or .jar.",
                        path.display()
                    ),
                );
            }
        }
    }

    fn start_msa_login(&mut self, role: AccountRole) {
        let client_id = self.settings.msa_client_id.clone();
        let client = MsaAuthClient::new(client_id);
        if client.uses_live_flow() {
            self.pending_live_auth = Some(role);
            self.live_auth_code_input.clear();
            return;
        }
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            match client.request_device_code().await {
                Ok(dev) => {
                    let _ = tx.send(BgEvent::DeviceCodeReady(dev.clone(), role));
                    match client.poll_device_code(&dev, role).await {
                        Ok(acc) => {
                            let _ = tx.send(BgEvent::AccountAdded(acc));
                        }
                        Err(e) => {
                            let _ = tx.send(BgEvent::Toast(
                                ToastTone::Danger,
                                format!("MSA sign-in failed: {e}"),
                            ));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgEvent::Toast(
                        ToastTone::Danger,
                        format!("Device code request failed: {e}"),
                    ));
                }
            }
        });
    }

    fn complete_live_login(&mut self, role: AccountRole) {
        let input = self.live_auth_code_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        let client_id = self.settings.msa_client_id.clone();
        let tx = self.tx.clone();
        self.pending_live_auth = None;
        self.live_auth_code_input.clear();
        self.rt.spawn(async move {
            let client = MsaAuthClient::new(client_id);
            match client.exchange_live_code(&input, role).await {
                Ok(acc) => {
                    let _ = tx.send(BgEvent::AccountAdded(acc));
                }
                Err(e) => {
                    let _ = tx.send(BgEvent::Toast(
                        ToastTone::Danger,
                        format!("Microsoft sign-in failed: {e}"),
                    ));
                }
            }
        });
    }

    fn current_breadcrumb(&self) -> String {
        match self.active_view {
            ActiveView::Instances => "~/instances".to_string(),
            ActiveView::Collections => "~/collections".to_string(),
            ActiveView::InstanceDetail => {
                let name = self
                    .profiles
                    .get(&self.instance_detail.profile_id)
                    .map(|p| p.name.to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let sub = match self.instance_detail.sub_tab {
                    instance_view::InstanceSubTab::Content => "content",
                    instance_view::InstanceSubTab::WorldsFiles => "worlds",
                    instance_view::InstanceSubTab::Logs => "logs",
                    instance_view::InstanceSubTab::Settings => "settings",
                };
                format!("~/instances/{name}/{sub}")
            }
            ActiveView::Discover => {
                if let Some(d) = &self.mods_view.active_detail {
                    format!("~/discover/{}/{}", self.mods_view.content_kind.label().to_lowercase(), d.slug)
                } else {
                    format!(
                        "~/discover/{}",
                        self.mods_view
                            .content_kind
                            .label()
                            .to_lowercase()
                            .replace(' ', "-")
                    )
                }
            }
            ActiveView::Accounts => "~/accounts".to_string(),
            ActiveView::Console => {
                let name = self
                    .console_profile_id
                    .as_deref()
                    .and_then(|id| self.profiles.get(id))
                    .map(|p| p.name.to_lowercase())
                    .unwrap_or_else(|| "all".to_string());
                format!("~/console/{name}")
            }
            ActiveView::Wall => "~/wall".to_string(),
            ActiveView::Config => "~/config".to_string(),
        }
    }

    fn fleet_stats(&self) -> (usize, usize, usize, usize, usize, u32, u32, usize) {
        let mut running_count = 0usize;
        let mut players_run = 0usize;
        let mut players_tot = 0usize;
        let mut bots_run = 0usize;
        let mut bots_tot = 0usize;
        let mut player_ram_mb = 0u32;
        let mut bot_ram_mb = 0u32;

        for p in &self.profiles.profiles {
            let is_run = matches!(
                self.statuses.get(&p.id).map(|s| &s.state),
                Some(InstanceState::Running | InstanceState::Preparing)
            );
            match p.kind {
                ProfileKind::Player => {
                    players_tot += 1;
                    if is_run {
                        running_count += 1;
                        players_run += 1;
                        player_ram_mb = player_ram_mb.saturating_add(p.memory_max_mb);
                    }
                }
                ProfileKind::Bot => {
                    bots_tot += 1;
                    if is_run {
                        running_count += 1;
                        bots_run += 1;
                        bot_ram_mb = bot_ram_mb.saturating_add(p.memory_max_mb);
                    }
                }
            }
        }
        let upd_tot: usize = self.profile_update_counts.values().sum();
        (
            running_count,
            players_run,
            players_tot,
            bots_run,
            bots_tot,
            player_ram_mb,
            bot_ram_mb,
            upd_tot,
        )
    }
}

impl eframe::App for MubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.sync_external_stores_if_changed();
        self.prune_expired_toasts(ctx);
        self.handle_dropped_files(ctx);

        // Global shortcuts: Ctrl+K Command Palette, Esc close
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.show_cmd_palette = !self.show_cmd_palette;
            self.cmd_query.clear();
        } else if self.show_cmd_palette && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_cmd_palette = false;
        }

        let palette = Palette::for_preset(self.theme_preset);
        theme::paint_dot_grid_backdrop(ctx, &palette);

        let (running_count, players_run, players_tot, bots_run, bots_tot, p_ram_mb, b_ram_mb, upd_tot) =
            self.fleet_stats();
        let total_mem_gb = (p_ram_mb + b_ram_mb) as f32 / 1024.0;

        // 1. Top Command Header
        egui::TopBottomPanel::top("mub_top_header")
            .frame(
                egui::Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .inner_margin(Margin::symmetric(12, 7)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    theme::paint_pixel_emblem(ui, &palette, 20.0);
                    ui.label(
                        RichText::new("MODSUNDBOTS")
                            .monospace()
                            .size(13.0)
                            .strong()
                            .color(palette.ink),
                    );
                    ui.label(
                        RichText::new("v0.2.0")
                            .monospace()
                            .size(12.0)
                            .strong()
                            .color(palette.focus),
                    );
                    ui.label(RichText::new("|").monospace().color(palette.border_hi));
                    ui.label(
                        RichText::new(self.current_breadcrumb())
                            .monospace()
                            .size(12.0)
                            .color(palette.meta),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("UPD:{upd_tot}"))
                                .monospace()
                                .size(11.5)
                                .strong()
                                .color(if upd_tot > 0 {
                                    palette.warn
                                } else {
                                    palette.ink_muted
                                }),
                        );
                        ui.label(
                            RichText::new(format!("MEM:{total_mem_gb:.1}G"))
                                .monospace()
                                .size(11.5)
                                .color(palette.player),
                        );
                        ui.label(
                            RichText::new(format!("● RUN:{running_count}"))
                                .monospace()
                                .size(11.5)
                                .strong()
                                .color(if running_count > 0 {
                                    palette.mr
                                } else {
                                    palette.ink_muted
                                }),
                        );
                        if theme::term_button(ui, &palette, "CTRL+K CMD", ButtonTone::Secondary)
                            .clicked()
                        {
                            self.show_cmd_palette = !self.show_cmd_palette;
                            self.cmd_query.clear();
                        }
                    });
                });
            });

        // 2. Bottom Tmux Status Bar
        egui::TopBottomPanel::bottom("mub_bottom_status")
            .frame(
                egui::Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .inner_margin(Margin::symmetric(12, 6)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // [MUB] focus badge
                    theme::term_badge(ui, "MUB", palette.on_focus, palette.focus);

                    let target_prof = self
                        .target_profile_id
                        .as_deref()
                        .and_then(|id| self.profiles.get(id))
                        .or_else(|| self.profiles.profiles.first());
                    let target_text = if let Some(tp) = target_prof {
                        format!("TARGET: {} [{} · {}]", tp.name, tp.mc_version, tp.mod_loader)
                    } else {
                        "TARGET: <none>".to_string()
                    };
                    let target_col = if target_prof.is_some() {
                        palette.ink_dim
                    } else {
                        palette.ink_muted
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(target_text)
                                .monospace()
                                .size(11.5)
                                .color(target_col),
                        )
                        .truncate(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.theme_preset.name())
                                .monospace()
                                .size(11.5)
                                .color(palette.meta),
                        );
                        if self.settings.effective_curseforge_api_key().is_some() {
                            ui.label(
                                RichText::new("CF:OK")
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.cf),
                            );
                        } else {
                            ui.label(
                                RichText::new("CF:KEY?")
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.warn),
                            );
                        }
                        ui.label(
                            RichText::new("MR:OK")
                                .monospace()
                                .size(11.5)
                                .color(palette.mr),
                        );

                        // Center/Right-fill: Live Task Progress or Latest Toast
                        ui.separator();
                        if let Some((frac, msg)) = &self.task_progress {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(msg)
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_dim),
                                )
                                .truncate(),
                            );
                            ui.label(
                                RichText::new(format!("{:.0}%", (frac.clamp(0.0, 1.0)) * 100.0))
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.focus),
                            );
                            theme::term_progress_bar(ui, &palette, *frac, 88.0);
                            ui.label(
                                RichText::new("TASK:")
                                    .monospace()
                                    .size(11.0)
                                    .strong()
                                    .color(palette.focus),
                            );
                        } else if let Some(toast) = self.toasts.back() {
                            let tid = toast.id;
                            let col = toast.tone.color(&palette);
                            let msg = toast.message.clone();
                            if theme::term_button(ui, &palette, "×", ButtonTone::Subtle).clicked()
                            {
                                self.toasts.retain(|t| t.id != tid);
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("» {msg}"))
                                        .monospace()
                                        .size(11.5)
                                        .color(col),
                                )
                                .truncate(),
                            );
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new("READY · DROP .MRPACK / .ZIP / .JAR")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                )
                                .truncate(),
                            );
                        }
                    });
                });
            });

        // 3. Left Terminal Rail (220px)
        egui::SidePanel::left("mub_left_rail")
            .resizable(false)
            .exact_width(220.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .inner_margin(Margin::same(0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.set_height(ui.available_height());

                ui.add_space(8.0);
                egui::Frame::NONE
                    .inner_margin(Margin::symmetric(12, 4))
                    .show(ui, |ui| {
                        theme::section_header(ui, &palette, "NAVIGATION", palette.ink_muted);
                    });

                let inst_badge = format!("[{:02}]", self.profiles.profiles.len());
                let col_badge = format!("[{:02}]", self.collections.collections.len());
                let acct_badge = format!("[{:02}]", self.accounts.accounts.len());
                let wall_badge = format!(
                    "[{:02}]",
                    self.statuses
                        .values()
                        .filter(|s| matches!(s.state, InstanceState::Running))
                        .count()
                );
                let active_group = match self.active_view {
                    ActiveView::InstanceDetail => ActiveView::Instances,
                    other => other,
                };

                    let config_badge = if self
                        .app_update_check
                        .as_ref()
                        .is_some_and(|u| u.update_available)
                        || self.app_update_installed.is_some()
                    {
                        Some("UPD")
                    } else {
                        None
                    };
                    let nav_items = [
                    (
                        ActiveView::Instances,
                        "01",
                        NavIcon::Instances,
                        "INSTANCES",
                        Some(inst_badge.as_str()),
                        palette.focus,
                    ),
                    (
                        ActiveView::Collections,
                        "02",
                        NavIcon::Collections,
                        "COLLECTIONS",
                        Some(col_badge.as_str()),
                        palette.bot,
                    ),
                    (
                        ActiveView::Discover,
                        "03",
                        NavIcon::Discover,
                        "DISCOVER",
                        Some("MR+CF"),
                        palette.cf,
                    ),
                    (
                        ActiveView::Accounts,
                        "04",
                        NavIcon::Accounts,
                        "ACCOUNTS",
                        Some(acct_badge.as_str()),
                        palette.player,
                    ),
                    (
                        ActiveView::Console,
                        "05",
                        NavIcon::Console,
                        "CONSOLE",
                        Some("LIVE"),
                        palette.mr,
                    ),
                    (
                        ActiveView::Wall,
                        "06",
                        NavIcon::Wall,
                        "WALL",
                        Some(wall_badge.as_str()),
                        palette.bot,
                    ),
                    (
                        ActiveView::Config,
                        "07",
                        NavIcon::Config,
                        "CONFIG",
                        config_badge,
                        if config_badge.is_some() {
                            palette.warn
                        } else {
                            palette.meta
                        },
                    ),
                ];

                for (view, num, icon, label, badge, badge_col) in nav_items {
                    if theme::term_nav_item(
                        ui,
                        &palette,
                        num,
                        icon,
                        label,
                        badge,
                        badge_col,
                        active_group == view,
                    )
                    .clicked()
                    {
                        self.active_view = view;
                    }
                }

                ui.add_space(10.0);
                egui::Frame::NONE
                    .inner_margin(Margin::symmetric(12, 4))
                    .show(ui, |ui| {
                        theme::section_header(ui, &palette, "QUICK LAUNCH", palette.ink_muted);
                    });

                // Top 4 profiles mini-list
                let quick_profiles: Vec<Profile> =
                    self.profiles.profiles.iter().take(4).cloned().collect();
                let mut quick_launch: Option<Profile> = None;
                let mut quick_stop: Option<String> = None;
                let mut quick_manage: Option<String> = None;

                egui::Frame::NONE
                    .inner_margin(Margin::symmetric(10, 2))
                    .show(ui, |ui| {
                        if quick_profiles.is_empty() {
                            ui.label(
                                RichText::new("No profiles yet.")
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.ink_muted),
                            );
                        }
                        for qp in &quick_profiles {
                            let is_run = matches!(
                                self.statuses.get(&qp.id).map(|s| &s.state),
                                Some(InstanceState::Running | InstanceState::Preparing)
                            );
                            let dot_col = if is_run {
                                palette.mr
                            } else if qp.kind == ProfileKind::Bot {
                                palette.bot
                            } else {
                                palette.player
                            };

                            theme::term_box(
                                ui,
                                palette.bg,
                                palette.border,
                                Margin::symmetric(6, 4),
                                |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let (dot_r, _) = ui.allocate_exact_size(
                                            egui::vec2(7.0, 7.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            dot_r,
                                            CornerRadius::ZERO,
                                            dot_col,
                                        );

                                        let name_w = (ui.available_width() - 34.0).max(60.0);
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(name_w, 18.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Label::new(
                                                            RichText::new(&qp.name)
                                                                .monospace()
                                                                .size(11.5)
                                                                .color(palette.ink),
                                                        )
                                                        .truncate()
                                                        .sense(egui::Sense::click()),
                                                    )
                                                    .clicked()
                                                {
                                                    quick_manage = Some(qp.id.clone());
                                                }
                                            },
                                        );

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let (sym, col) = if is_run {
                                                    ("■", palette.danger)
                                                } else {
                                                    ("▶", palette.ink_dim)
                                                };
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(sym)
                                                                .monospace()
                                                                .size(11.5)
                                                                .color(col),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .clicked()
                                                {
                                                    if is_run {
                                                        quick_stop = Some(qp.id.clone());
                                                    } else {
                                                        quick_launch = Some(qp.clone());
                                                    }
                                                }
                                            },
                                        );
                                    });
                                },
                            );
                            ui.add_space(3.0);
                        }
                    });

                if let Some(pid) = quick_manage {
                    self.open_instance_detail(&pid);
                }
                if let Some(p) = quick_launch {
                    self.launch_profiles(vec![p]);
                }
                if let Some(pid) = quick_stop {
                    self.stop_profile(pid);
                }

                // Bottom Fleet Status & Kill Switch
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    egui::Frame::NONE
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            let stop_lbl = format!("[■] STOP ALL ({running_count})");
                            let stop_tone = if running_count > 0 {
                                ButtonTone::Danger
                            } else {
                                ButtonTone::Secondary
                            };
                            if ui
                                .add_sized(
                                    [ui.available_width(), 28.0],
                                    |ui: &mut egui::Ui| {
                                        theme::term_button(ui, &palette, &stop_lbl, stop_tone)
                                    },
                                )
                                .clicked()
                            {
                                self.stop_all();
                            }
                            ui.add_space(8.0);

                            theme::term_surface(ui, &palette, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("FLEET")
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.ink_muted),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(if running_count > 0 {
                                                    "● ONLINE"
                                                } else {
                                                    "○ IDLE"
                                                })
                                                .monospace()
                                                .size(11.0)
                                                .strong()
                                                .color(if running_count > 0 {
                                                    palette.mr
                                                } else {
                                                    palette.ink_muted
                                                }),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("P:{players_run}/{players_tot}"))
                                            .monospace()
                                            .size(11.5)
                                            .strong()
                                            .color(palette.player),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!("B:{bots_run}/{bots_tot}"))
                                                    .monospace()
                                                    .size(11.5)
                                                    .strong()
                                                    .color(palette.bot),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("RAM")
                                            .monospace()
                                            .size(10.5)
                                            .color(palette.ink_muted),
                                    );
                                    let cap_mb = 10240.0_f32;
                                    let p_frac = (p_ram_mb as f32 / cap_mb).min(1.0);
                                    let b_frac = (b_ram_mb as f32 / cap_mb).min(1.0 - p_frac);
                                    let bar_w = (ui.available_width() - 42.0).max(48.0);
                                    theme::term_pixel_ram_bar(ui, &palette, p_frac, b_frac, bar_w);
                                    ui.label(
                                        RichText::new(format!("{total_mem_gb:.1}G"))
                                            .monospace()
                                            .size(10.5)
                                            .color(palette.ink_dim),
                                    );
                                });
                            });
                        });
                });
            });

        // 4. Main Workspace
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(Margin::same(16)))
            .show(ctx, |ui| match self.active_view {
                ActiveView::Instances => self.ui_instances(ui, ctx, &palette),
                ActiveView::Collections => self.ui_collections(ui, ctx, &palette),
                ActiveView::InstanceDetail => {
                    let actions = self.instance_detail.ui(
                        ui,
                        ctx,
                        &palette,
                        &self.rt,
                        &self.paths,
                        &self.settings,
                        &self.profiles,
                        &self.accounts,
                        &self.statuses,
                        &self.manager,
                        &mut self.version_cache,
                        &self.detected_javas,
                    );
                    self.handle_instance_actions(actions);
                }
                ActiveView::Discover => {
                    let actions = self.mods_view.ui(
                        ui,
                        ctx,
                        &palette,
                        &self.rt,
                        &self.paths,
                        &self.profiles,
                        &self.settings,
                    );
                    self.handle_mods_actions(actions);
                }
                ActiveView::Accounts => self.ui_accounts(ui, &palette),
                ActiveView::Console => self.ui_console(ui, &palette),
                ActiveView::Wall => {
                    let actions = self.wall.ui(ui, &palette, &self.statuses, &self.profiles);
                    self.handle_wall_actions(actions);
                }
                ActiveView::Config => self.ui_config(ui, ctx, &palette),
            });

        // 5. Drag-and-Drop hover overlay
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.screen_rect();
            egui::Area::new(Id::new("mub_dnd_overlay"))
                .order(Order::Foreground)
                .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        screen,
                        CornerRadius::ZERO,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                    );
                    theme::term_box(
                        ui,
                        palette.raised,
                        palette.focus,
                        Margin::same(24),
                        |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("DROP FILES TO IMPORT / INSTALL")
                                        .monospace()
                                        .size(15.0)
                                        .strong()
                                        .color(palette.focus),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        ".mrpack / .zip -> Import Modpack as new Profile\n.jar -> Install Mod to target profile's mods/",
                                    )
                                    .monospace()
                                    .size(12.0)
                                    .color(palette.ink_dim),
                                );
                            });
                        },
                    );
                });
        }

        // 6. Command Palette Modal (Ctrl+K)
        if self.show_cmd_palette {
            self.ui_command_palette(ctx, &palette);
        }
    }
}

impl MubApp {
    fn handle_wall_actions(&mut self, actions: Vec<WallAction>) {
        for action in actions {
            match action {
                WallAction::Show(id) => {
                    let mgr = self.manager.clone();
                    self.rt.spawn(async move {
                        mgr.set_window_hidden(&id, false, true).await;
                    });
                }
                WallAction::Hide(id) => {
                    let mgr = self.manager.clone();
                    self.rt.spawn(async move {
                        mgr.set_window_hidden(&id, true, false).await;
                    });
                }
                WallAction::HideAllBots | WallAction::ShowAll => {
                    let hide = matches!(action, WallAction::HideAllBots);
                    let ids: Vec<String> = self
                        .statuses
                        .values()
                        .filter(|s| matches!(s.state, InstanceState::Running))
                        .filter(|s| {
                            !hide
                                || self
                                    .profiles
                                    .get(&s.profile_id)
                                    .map(|p| p.kind == ProfileKind::Bot)
                                    .unwrap_or(true)
                        })
                        .map(|s| s.profile_id.clone())
                        .collect();
                    let mgr = self.manager.clone();
                    self.rt.spawn(async move {
                        for id in ids {
                            mgr.set_window_hidden(&id, hide, false).await;
                        }
                    });
                }
                WallAction::UnhideOrphans => {
                    let tx = self.tx.clone();
                    self.rt.spawn(async move {
                        let n = tokio::task::spawn_blocking(mub_core::window::unhide_all_java)
                            .await
                            .unwrap_or(0);
                        let _ = tx.send(BgEvent::Toast(
                            ToastTone::Focus,
                            format!("Showed {n} hidden game window(s)"),
                        ));
                    });
                }
                WallAction::OpenConsole(id) => {
                    self.console_profile_id = Some(id);
                    self.active_view = ActiveView::Console;
                }
            }
        }
    }

    fn handle_instance_actions(&mut self, actions: Vec<InstanceAction>) {
        for action in actions {
            match action {
                InstanceAction::Back => {
                    self.active_view = ActiveView::Instances;
                }
                InstanceAction::AddContent(pid) => {
                    self.target_profile_id = Some(pid.clone());
                    self.mods_view.lock_to_profile(pid);
                    self.active_view = ActiveView::Discover;
                }
                InstanceAction::Launch(p) => {
                    self.launch_profiles(vec![p]);
                }
                InstanceAction::Stop(pid) => {
                    self.stop_profile(pid);
                }
                InstanceAction::ProfileUpdated(updated) => {
                    self.profiles.upsert(updated);
                    let _ = self.profiles.save(&self.paths.profiles_file());
                }
                InstanceAction::ProfileCreated(created) => {
                    let cid = created.id.clone();
                    self.profiles.upsert(created);
                    let _ = self.profiles.save(&self.paths.profiles_file());
                    self.open_instance_detail(&cid);
                }
                InstanceAction::ProfileDeleted { id, delete_files } => {
                    self.profiles.remove(&id);
                    self.selected_profiles.remove(&id);
                    self.profile_update_counts.remove(&id);
                    let _ = self.profiles.save(&self.paths.profiles_file());
                    if delete_files {
                        let dir = self.paths.instance_dir(&id);
                        if dir.exists() {
                            let _ = std::fs::remove_dir_all(&dir);
                        }
                    }
                    if self.target_profile_id.as_deref() == Some(&id) {
                        self.target_profile_id =
                            self.profiles.profiles.first().map(|p| p.id.clone());
                    }
                    self.active_view = ActiveView::Instances;
                    self.push_toast(
                        ToastTone::Danger,
                        if delete_files {
                            "Deleted profile and removed instance folder from disk."
                        } else {
                            "Deleted profile (instance files kept on disk)."
                        },
                    );
                }
                InstanceAction::UpdateCountChanged { profile_id, count } => {
                    self.profile_update_counts.insert(profile_id, count);
                }
                InstanceAction::Toast(tone, msg) => {
                    self.push_toast(tone, msg);
                }
                InstanceAction::TaskProgress(prog) => {
                    self.task_progress = prog;
                }
            }
        }
    }

    fn handle_mods_actions(&mut self, actions: Vec<ModsAction>) {
        for action in actions {
            match action {
                ModsAction::OpenConfig => {
                    self.active_view = ActiveView::Config;
                }
                ModsAction::ModpackCreated(profile) => {
                    let pid = profile.id.clone();
                    self.profiles.upsert(*profile);
                    let _ = self.profiles.save(&self.paths.profiles_file());
                    self.target_profile_id = Some(pid);
                }
                ModsAction::Toast(tone, msg) => {
                    self.push_toast(tone, msg);
                }
                ModsAction::TaskProgress(prog) => {
                    self.task_progress = prog;
                }
            }
        }
    }

    fn ui_command_palette(&mut self, ctx: &egui::Context, palette: &Palette) {
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("mub_command_palette"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
            .show(ctx, |ui| {
                ui.painter().rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 145),
                );

                theme::term_raised(ui, palette, |ui| {
                    ui.set_width(520.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("COMMAND PALETTE [CTRL+K]")
                                .monospace()
                                .strong()
                                .size(12.0)
                                .color(palette.focus),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::term_button(ui, palette, "ESC", ButtonTone::Secondary)
                                .clicked()
                            {
                                self.show_cmd_palette = false;
                            }
                        });
                    });
                    theme::term_divider(ui, palette);

                    let (resp, _, _) = theme::term_search_bar(
                        ui,
                        palette,
                        &mut self.cmd_query,
                        ">",
                        "type command or profile name...",
                        496.0,
                    );
                    resp.request_focus();

                    ui.add_space(4.0);
                    let q = self.cmd_query.trim().to_lowercase();

                    let static_cmds: Vec<(&str, &str, Color32, u8)> = vec![
                        ("> go instances", "01 INSTANCES", palette.focus, 1),
                        ("> go bot collections", "02 COLLECTIONS", palette.bot, 8),
                        (
                            "> discover modrinth + curseforge",
                            "03 DISCOVER",
                            palette.meta,
                            2,
                        ),
                        ("> go accounts", "04 ACCOUNTS", palette.player, 3),
                        ("> go console", "05 CONSOLE", palette.mr, 4),
                        ("> go wall (bot thumbnails)", "06 WALL", palette.bot, 9),
                        ("> go config", "07 CONFIG", palette.meta, 5),
                        ("> launch all bots (staggered)", "SWARM", palette.bot, 6),
                        ("> stop all active instances", "SIGKILL", palette.danger, 7),
                    ];

                    for (cmd, tag, tag_col, code) in static_cmds {
                        if !q.is_empty() && !cmd.contains(&q) && !tag.to_lowercase().contains(&q) {
                            continue;
                        }
                        if cmd_row(ui, palette, cmd, tag, tag_col) {
                            self.show_cmd_palette = false;
                            match code {
                                1 => self.active_view = ActiveView::Instances,
                                2 => self.active_view = ActiveView::Discover,
                                3 => self.active_view = ActiveView::Accounts,
                                4 => self.active_view = ActiveView::Console,
                                5 => self.active_view = ActiveView::Config,
                                6 => self.launch_all_bots(),
                                7 => self.stop_all(),
                                8 => self.active_view = ActiveView::Collections,
                                9 => self.active_view = ActiveView::Wall,
                                _ => {}
                            }
                        }
                    }

                    let matching_profiles: Vec<Profile> = self
                        .profiles
                        .profiles
                        .iter()
                        .filter(|p| {
                            q.is_empty()
                                || p.name.to_lowercase().contains(&q)
                                || p.mc_version.to_lowercase().contains(&q)
                        })
                        .take(5)
                        .cloned()
                        .collect();

                    for p in matching_profiles {
                        let lbl = format!("> manage profile {}", p.name);
                        let tag = format!("MC {}", p.mc_version);
                        let col = if p.kind == ProfileKind::Bot {
                            palette.bot
                        } else {
                            palette.player
                        };
                        if cmd_row(ui, palette, &lbl, &tag, col) {
                            self.show_cmd_palette = false;
                            self.open_instance_detail(&p.id);
                        }
                    }
                });
            });
    }

    fn ui_instances(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, palette: &Palette) {
        let total_count = self.profiles.profiles.len();
        let players_tot = self
            .profiles
            .profiles
            .iter()
            .filter(|p| p.kind == ProfileKind::Player)
            .count();
        let bots_tot = total_count.saturating_sub(players_tot);

        // Header row
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 400.0).max(180.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("INSTANCES")
                            .monospace()
                            .size(16.0)
                            .strong()
                            .color(palette.ink),
                    );
                    ui.label(
                        RichText::new(format!("[{total_count}]"))
                            .monospace()
                            .size(14.0)
                            .strong()
                            .color(palette.focus),
                    );
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Isolated player & bot profiles, staggered swarm launches, and .mrpack / CurseForge pack import.",
                        )
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                    )
                    .truncate(),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::term_button(ui, palette, "[+] NEW PROFILE", ButtonTone::Cta).clicked() {
                    self.show_new_profile = !self.show_new_profile;
                    if self.show_new_profile {
                        self.show_import_pack = false;
                    }
                }
                if theme::term_button(ui, palette, "[▶▶] LAUNCH BOTS", ButtonTone::Bot).clicked() {
                    self.launch_all_bots();
                }
                if theme::term_button(ui, palette, "[↓] IMPORT PACK", ButtonTone::Secondary)
                    .clicked()
                {
                    self.show_import_pack = !self.show_import_pack;
                    if self.show_import_pack {
                        self.show_new_profile = false;
                    }
                }
                if !self.selected_profiles.is_empty()
                    && theme::term_button(
                        ui,
                        palette,
                        &format!("[▶] LAUNCH SELECTED ({})", self.selected_profiles.len()),
                        ButtonTone::Focus,
                    )
                    .clicked()
                {
                    let selected: Vec<Profile> = self
                        .profiles
                        .profiles
                        .iter()
                        .filter(|p| self.selected_profiles.contains(&p.id))
                        .cloned()
                        .collect();
                    self.launch_profiles(selected);
                }
            });
        });

        theme::term_divider(ui, palette);
        ui.add_space(4.0);

        // Inline Import Pack drawer
        if self.show_import_pack {
            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::section_header(
                        ui,
                        palette,
                        "IMPORT .MRPACK OR CURSEFORGE .ZIP PACK",
                        palette.cf,
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::term_button(ui, palette, "CLOSE", ButtonTone::Subtle).clicked() {
                            self.show_import_pack = false;
                        }
                    });
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Paste a path to a .mrpack / CurseForge .zip below, or drag-and-drop the pack file directly onto this window.",
                    )
                    .monospace()
                    .size(11.0)
                    .color(palette.ink_muted),
                );
                ui.add_space(6.0);
                egui::Grid::new("import_pack_grid")
                    .num_columns(2)
                    .spacing([14.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("PACK FILE PATH")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.import_pack_path)
                                .hint_text(r"C:\path\to\modpack.mrpack or modpack.zip")
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(420.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("PROFILE NAME (OPT)")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.import_pack_name)
                                .hint_text("leave empty to use pack manifest name")
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(260.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("PROFILE KIND")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.horizontal(|ui| {
                            if theme::term_button(
                                ui,
                                palette,
                                "PLAYER",
                                if self.import_pack_kind == ProfileKind::Player {
                                    ButtonTone::Player
                                } else {
                                    ButtonTone::Subtle
                                },
                            )
                            .clicked()
                            {
                                self.import_pack_kind = ProfileKind::Player;
                            }
                            if theme::term_button(
                                ui,
                                palette,
                                "BOT",
                                if self.import_pack_kind == ProfileKind::Bot {
                                    ButtonTone::Bot
                                } else {
                                    ButtonTone::Subtle
                                },
                            )
                            .clicked()
                            {
                                self.import_pack_kind = ProfileKind::Bot;
                            }
                        });
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let can_import =
                        !self.importing_pack && !self.import_pack_path.trim().is_empty();
                    let lbl = if self.importing_pack {
                        "IMPORTING..."
                    } else {
                        "IMPORT PACK"
                    };
                    if ui
                        .add_enabled_ui(can_import, |ui| {
                            theme::term_button(ui, palette, lbl, ButtonTone::Cta)
                        })
                        .inner
                        .clicked()
                    {
                        let p = PathBuf::from(self.import_pack_path.trim().trim_matches('"'));
                        let name = {
                            let n = self.import_pack_name.trim();
                            if n.is_empty() {
                                None
                            } else {
                                Some(n.to_string())
                            }
                        };
                        self.spawn_import_pack(ctx, p, name, self.import_pack_kind);
                    }
                    if theme::term_button(ui, palette, "CANCEL", ButtonTone::Subtle).clicked() {
                        self.show_import_pack = false;
                    }
                });
            });
            ui.add_space(8.0);
        }

        // Inline New Profile drawer (live Mojang + Loader picker)
        if self.show_new_profile {
            self.version_cache.ensure_mc_loaded(&self.rt, ctx);
            self.version_cache.ensure_loader_loaded(
                &self.rt,
                ctx,
                &self.new_prof_mc_version,
                self.new_prof_loader_choice,
            );

            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::section_header(
                        ui,
                        palette,
                        "CREATE NEW PROFILE (LIVE MOJANG + LOADER PICKER)",
                        palette.focus,
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::term_button(ui, palette, "CLOSE", ButtonTone::Subtle).clicked() {
                            self.show_new_profile = false;
                        }
                    });
                });
                ui.add_space(6.0);

                ui.columns(2, |cols| {
                    cols[0].vertical(|ui| {
                        ui.label(
                            RichText::new("PROFILE NAME")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_name)
                                .hint_text("e.g. FarmBot-04")
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(ui.available_width()),
                        );
                        ui.add_space(6.0);

                        ui.label(
                            RichText::new("MINECRAFT VERSION (LIVE MOJANG)")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        if self.version_cache.render_mc_picker(
                            ui,
                            palette,
                            &self.rt,
                            "new_prof_mc_combo",
                            &mut self.new_prof_mc_version,
                            &mut self.new_prof_show_snapshots,
                            &mut self.new_prof_mc_search,
                        ) {
                            self.new_prof_loader_ver.clear();
                            self.version_cache.ensure_loader_loaded(
                                &self.rt,
                                ctx,
                                &self.new_prof_mc_version,
                                self.new_prof_loader_choice,
                            );
                        }
                        ui.add_space(6.0);

                        ui.label(
                            RichText::new("MOD LOADER")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        self.version_cache.render_loader_picker(
                            ui,
                            palette,
                            &self.rt,
                            "new_prof_loader_combo",
                            &self.new_prof_mc_version,
                            &mut self.new_prof_loader_choice,
                            &mut self.new_prof_loader_ver,
                            false,
                        );
                    });

                    cols[1].vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("KIND")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.horizontal(|ui| {
                                    if theme::term_button(
                                        ui,
                                        palette,
                                        "PLAYER",
                                        if self.new_prof_kind == ProfileKind::Player {
                                            ButtonTone::Player
                                        } else {
                                            ButtonTone::Subtle
                                        },
                                    )
                                    .clicked()
                                    {
                                        self.new_prof_kind = ProfileKind::Player;
                                        self.new_prof_ram_max = 4096;
                                    }
                                    if theme::term_button(
                                        ui,
                                        palette,
                                        "BOT",
                                        if self.new_prof_kind == ProfileKind::Bot {
                                            ButtonTone::Bot
                                        } else {
                                            ButtonTone::Subtle
                                        },
                                    )
                                    .clicked()
                                    {
                                        self.new_prof_kind = ProfileKind::Bot;
                                        self.new_prof_ram_max = 1024;
                                    }
                                });
                            });

                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("MAX RAM (MB)")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut self.new_prof_ram_max)
                                        .range(256..=32768)
                                        .speed(128),
                                );
                            });

                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("GROUP (OPT)")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_prof_group)
                                        .hint_text("BOT-SWARM")
                                        .font(egui::FontId::monospace(12.0))
                                        .desired_width(120.0),
                                );
                            });
                        });

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("ASSIGNED ACCOUNT")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        let acc_label = self
                            .new_prof_account_id
                            .as_deref()
                            .and_then(|id| self.accounts.get(id))
                            .map(|a| format!("{} ({})", a.username, a.role))
                            .unwrap_or_else(|| "<None>".to_string());
                        egui::ComboBox::from_id_salt("new_prof_acc_combo")
                            .selected_text(acc_label)
                            .width(260.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.new_prof_account_id, None, "<None>");
                                for acc in &self.accounts.accounts {
                                    ui.selectable_value(
                                        &mut self.new_prof_account_id,
                                        Some(acc.id.clone()),
                                        format!("{} ({})", acc.username, acc.role),
                                    );
                                }
                            });

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("QUICK-CONNECT SERVER")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_server)
                                .hint_text("play.smp.gg:25565")
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(ui.available_width()),
                        );
                    });
                });

                ui.add_space(8.0);
                let loader_ok = self.new_prof_loader_choice == LoaderChoice::Vanilla
                    || !self.new_prof_loader_ver.trim().is_empty();
                let name_ok = !self.new_prof_name.trim().is_empty();
                let mc_ok = !self.new_prof_mc_version.trim().is_empty();
                let can_create = name_ok && mc_ok && loader_ok;

                ui.horizontal(|ui| {
                    if !loader_ok {
                        ui.label(
                            RichText::new(format!(
                                "Waiting for a valid {} loader version for MC {} (will not fall back to Vanilla).",
                                self.new_prof_loader_choice.label(),
                                self.new_prof_mc_version
                            ))
                            .monospace()
                            .size(11.0)
                            .color(palette.warn),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(can_create, |ui| {
                                theme::term_button(ui, palette, "CREATE PROFILE", ButtonTone::Cta)
                            })
                            .inner
                            .clicked()
                        {
                            let loader = self
                                .new_prof_loader_choice
                                .to_mod_loader(&self.new_prof_loader_ver);
                            let mut p = Profile::new(
                                self.new_prof_name.trim(),
                                self.new_prof_kind,
                                self.new_prof_mc_version.trim(),
                                loader.clone(),
                            );
                            p.memory_max_mb = self.new_prof_ram_max;
                            p.account_id = self.new_prof_account_id.clone();
                            if !self.new_prof_server.trim().is_empty() {
                                p.quick_connect = Some(self.new_prof_server.trim().to_string());
                            }
                            if !self.new_prof_group.trim().is_empty() {
                                p.group = Some(self.new_prof_group.trim().to_string());
                            }
                            let pname = p.name.clone();
                            let pid = p.id.clone();
                            let mc = p.mc_version.clone();
                            self.profiles.upsert(p);
                            let _ = self.profiles.save(&self.paths.profiles_file());
                            self.target_profile_id = Some(pid);
                            self.new_prof_name.clear();
                            self.new_prof_group.clear();
                            self.show_new_profile = false;
                            self.push_toast(
                                if self.new_prof_kind == ProfileKind::Bot {
                                    ToastTone::Bot
                                } else {
                                    ToastTone::Player
                                },
                                format!("Created profile '{pname}' ({mc} · {loader})"),
                            );
                        }
                        if theme::term_button(ui, palette, "CANCEL", ButtonTone::Subtle).clicked() {
                            self.show_new_profile = false;
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }

        // Filter & Search Toolbar
        theme::term_surface(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                for (f, lbl, tone) in [
                    (
                        ProfileFilter::All,
                        format!("ALL [{total_count}]"),
                        ButtonTone::Focus,
                    ),
                    (
                        ProfileFilter::Players,
                        format!("PLAYERS [{players_tot}]"),
                        ButtonTone::Player,
                    ),
                    (
                        ProfileFilter::Bots,
                        format!("BOTS [{bots_tot}]"),
                        ButtonTone::Bot,
                    ),
                ] {
                    let active = self.profile_filter == f;
                    if theme::term_button(
                        ui,
                        palette,
                        &lbl,
                        if active { tone } else { ButtonTone::Subtle },
                    )
                    .clicked()
                    {
                        self.profile_filter = f;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let chk_lbl = if self.checking_all_updates {
                        "[↻] CHECKING..."
                    } else {
                        "[↻] CHECK UPDATES"
                    };
                    if ui
                        .add_enabled_ui(!self.checking_all_updates, |ui| {
                            theme::term_button(ui, palette, chk_lbl, ButtonTone::Warn)
                        })
                        .inner
                        .clicked()
                    {
                        self.spawn_check_all_updates(ctx);
                    }

                    theme::term_search_bar(
                        ui,
                        palette,
                        &mut self.search_query,
                        ">",
                        "filter profiles...",
                        210.0,
                    );
                });
            });
        });

        ui.add_space(8.0);

        if !self.collections.collections.is_empty() {
            let mut quick_start_col: Option<String> = None;
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("BOT COLLECTIONS:")
                            .monospace()
                            .size(11.0)
                            .strong()
                            .color(palette.bot),
                    );
                    for col in &self.collections.collections {
                        let lbl = format!("[▶▶] START ALL: {} ({})", col.name, col.profile_ids.len());
                        if theme::term_button(ui, palette, &lbl, ButtonTone::Bot).clicked() {
                            quick_start_col = Some(col.id.clone());
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::term_button(ui, palette, "MANAGE →", ButtonTone::Subtle).clicked() {
                            self.active_view = ActiveView::Collections;
                        }
                    });
                });
            });
            if let Some(cid) = quick_start_col {
                self.launch_collection(&cid);
            }
            ui.add_space(8.0);
        }

        if self.profiles.profiles.is_empty() {
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
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
                            "Click '[+] NEW PROFILE', '[↓] IMPORT PACK', or drop a .mrpack / .zip file onto the window.",
                        )
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                    );
                    ui.add_space(28.0);
                });
            });
            return;
        }

        let query = self.search_query.trim().to_lowercase();
        let mut to_launch = Vec::new();
        let mut to_stop = Vec::new();
        let mut to_manage: Option<String> = None;
        let mut to_logs: Option<String> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for p in &self.profiles.profiles {
                match self.profile_filter {
                    ProfileFilter::Players if p.kind != ProfileKind::Player => continue,
                    ProfileFilter::Bots if p.kind != ProfileKind::Bot => continue,
                    _ => {}
                }
                let acc_name = p
                    .account_id
                    .as_deref()
                    .and_then(|id| self.accounts.get(id))
                    .map(|a| a.username.as_str())
                    .unwrap_or("UNASSIGNED");
                let loader_str = p.mod_loader.to_string();

                if !query.is_empty()
                    && !p.name.to_lowercase().contains(&query)
                    && !acc_name.to_lowercase().contains(&query)
                    && !loader_str.to_lowercase().contains(&query)
                    && !p.mc_version.to_lowercase().contains(&query)
                {
                    continue;
                }

                let status = self.statuses.get(&p.id);
                let is_active = matches!(
                    status.map(|s| &s.state),
                    Some(InstanceState::Running | InstanceState::Preparing)
                );
                let (kind_fg, kind_bg, kind_lbl) = match p.kind {
                    ProfileKind::Player => (palette.player, palette.player_bg, "PLAYER"),
                    ProfileKind::Bot => (palette.bot, palette.bot_bg, "BOT"),
                };
                let mods_count = list_installed_content(&self.paths, &p.id, ContentKind::Mod)
                    .map(|v| v.len())
                    .unwrap_or(0);
                let upd_count = self.profile_update_counts.get(&p.id).copied().unwrap_or(0);

                theme::term_row(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    let row_w = ui.available_width();
                    let right_w = 240.0_f32;
                    let mid_w = (row_w - 30.0 - 38.0 - right_w - 22.0).max(160.0);

                    ui.horizontal(|ui| {
                        let mut checked = self.selected_profiles.contains(&p.id);
                        if ui.checkbox(&mut checked, "").changed() {
                            if checked {
                                self.selected_profiles.insert(p.id.clone());
                            } else {
                                self.selected_profiles.remove(&p.id);
                            }
                        }

                        theme::paint_avatar_tile(ui, &p.name, kind_fg, kind_bg, 38.0);
                        ui.add_space(4.0);

                        ui.allocate_ui_with_layout(
                            egui::vec2(mid_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(mid_w);
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .link(
                                            RichText::new(&p.name)
                                                .monospace()
                                                .size(13.5)
                                                .strong()
                                                .color(palette.ink),
                                        )
                                        .clicked()
                                    {
                                        to_manage = Some(p.id.clone());
                                    }
                                    theme::term_badge(ui, kind_lbl, kind_fg, kind_bg);
                                    if let Some(grp) = &p.group
                                        && !grp.trim().is_empty()
                                    {
                                        theme::term_badge_bordered(
                                            ui,
                                            grp,
                                            palette.meta,
                                            palette.meta_bg,
                                            palette.border_hi,
                                        );
                                    }

                                    match status.map(|s| &s.state) {
                                        Some(InstanceState::Running) => {
                                            let pid = status.and_then(|s| s.pid).unwrap_or(0);
                                            theme::term_badge(
                                                ui,
                                                &format!("● RUNNING PID:{pid}"),
                                                palette.mr,
                                                palette.mr_bg,
                                            );
                                        }
                                        Some(InstanceState::Preparing) => {
                                            let prog = status
                                                .and_then(|s| s.progress.as_deref())
                                                .unwrap_or("PREPARING");
                                            theme::term_badge(
                                                ui,
                                                &format!("◐ {prog}"),
                                                palette.warn,
                                                palette.warn_bg,
                                            );
                                        }
                                        Some(InstanceState::Failed { .. }) => {
                                            theme::term_badge(
                                                ui,
                                                "× FAILED",
                                                palette.danger,
                                                palette.danger_bg,
                                            );
                                        }
                                        _ => {
                                            theme::term_badge_bordered(
                                                ui,
                                                "○ IDLE",
                                                palette.ink_muted,
                                                Color32::TRANSPARENT,
                                                palette.border,
                                            );
                                        }
                                    }

                                    if upd_count > 0 {
                                        theme::term_badge(
                                            ui,
                                            &format!("↑ {upd_count} UPD"),
                                            palette.warn,
                                            palette.warn_bg,
                                        );
                                    }
                                });

                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.label(
                                        RichText::new(format!("MC {}", p.mc_version))
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.meta),
                                    );
                                    ui.label(
                                        RichText::new("│")
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.border_hi),
                                    );
                                    ui.label(
                                        RichText::new(&loader_str)
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.meta),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "│ ACCT: {} │ RAM: {}M │ MODS: {} │ TIME: {}",
                                            acc_name,
                                            p.memory_max_mb,
                                            mods_count,
                                            format_playtime(p.playtime_secs)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                    );
                                    if let Some(srv) = &p.quick_connect
                                        && !srv.trim().is_empty()
                                    {
                                        ui.label(
                                            RichText::new("│")
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.border_hi),
                                        );
                                        ui.label(
                                            RichText::new(srv)
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.player),
                                        );
                                    }
                                });
                            },
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::term_button(ui, palette, "LOGS", ButtonTone::Subtle).clicked()
                            {
                                to_logs = Some(p.id.clone());
                            }
                            if theme::term_button(ui, palette, "MANAGE", ButtonTone::Secondary)
                                .clicked()
                            {
                                to_manage = Some(p.id.clone());
                            }
                            if is_active {
                                if theme::term_button(ui, palette, "[■] STOP", ButtonTone::Danger)
                                    .clicked()
                                {
                                    to_stop.push(p.id.clone());
                                }
                            } else if theme::term_button(ui, palette, "[▶] PLAY", ButtonTone::Cta)
                                .clicked()
                            {
                                to_launch.push(p.clone());
                            }
                        });
                    });
                });
                ui.add_space(5.0);
            }
        });

        if !to_launch.is_empty() {
            self.launch_profiles(to_launch);
        }
        for id in to_stop {
            self.stop_profile(id);
        }
        if let Some(pid) = to_manage {
            self.open_instance_detail(&pid);
        }
        if let Some(pid) = to_logs {
            self.console_profile_id = Some(pid);
            self.active_view = ActiveView::Console;
        }
    }

    fn ui_accounts(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 260.0).max(180.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ACCOUNTS")
                            .monospace()
                            .size(16.0)
                            .strong()
                            .color(palette.ink),
                    );
                    ui.label(
                        RichText::new(format!("[{:02}]", self.accounts.accounts.len()))
                            .monospace()
                            .size(14.0)
                            .strong()
                            .color(palette.player),
                    );
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Microsoft (MSA) player/bot tokens & local offline bot identities.",
                        )
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                    )
                    .truncate(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::term_button(ui, palette, "[+] MSA BOT", ButtonTone::Bot).clicked() {
                    self.start_msa_login(AccountRole::Bot);
                }
                if theme::term_button(ui, palette, "[+] MSA PLAYER", ButtonTone::Player).clicked() {
                    self.start_msa_login(AccountRole::Player);
                }
            });
        });
        theme::term_divider(ui, palette);
        ui.add_space(4.0);

        if let Some(role) = self.pending_live_auth {
            let auth_url = MsaAuthClient::live_authorize_url();
            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                theme::section_header(
                    ui,
                    palette,
                    &format!("MICROSOFT SIGN-IN ({role})"),
                    palette.player,
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("1. Open Microsoft login:")
                            .monospace()
                            .size(11.5)
                            .color(palette.ink_dim),
                    );
                    ui.hyperlink_to(
                        RichText::new("login.live.com")
                            .monospace()
                            .size(11.5)
                            .color(palette.player),
                        &auth_url,
                    );
                    if theme::term_button(ui, palette, "COPY URL", ButtonTone::Secondary).clicked()
                    {
                        ui.ctx().copy_text(auth_url.clone());
                        self.push_toast(ToastTone::Focus, "Copied Microsoft login URL");
                    }
                });
                ui.label(
                    RichText::new(
                        "2. After signing in, paste the final redirect URL (https://login.live.com/oauth20_desktop.srf?code=...):",
                    )
                    .monospace()
                    .size(11.0)
                    .color(palette.ink_muted),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.live_auth_code_input)
                            .hint_text("https://login.live.com/oauth20_desktop.srf?code=...")
                            .font(egui::FontId::monospace(11.5))
                            .desired_width(400.0),
                    );
                    if theme::term_button(ui, palette, "COMPLETE SIGN-IN", ButtonTone::Cta)
                        .clicked()
                    {
                        self.complete_live_login(role);
                    }
                    if theme::term_button(ui, palette, "CANCEL", ButtonTone::Subtle).clicked() {
                        self.pending_live_auth = None;
                        self.live_auth_code_input.clear();
                    }
                });
            });
            ui.add_space(8.0);
        }

        if let Some((dev, role)) = &self.pending_device_code {
            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                theme::section_header(
                    ui,
                    palette,
                    &format!("MICROSOFT DEVICE CODE ({role})"),
                    palette.warn,
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("1. Open:").monospace().color(palette.ink_dim));
                    ui.hyperlink(&dev.verification_uri);
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("2. Enter code:")
                            .monospace()
                            .color(palette.ink_dim),
                    );
                    theme::term_badge(ui, &dev.user_code, palette.mr, palette.mr_bg);
                    if theme::term_button(ui, palette, "COPY CODE", ButtonTone::Secondary).clicked()
                    {
                        ui.ctx().copy_text(dev.user_code.clone());
                    }
                });
            });
            ui.add_space(8.0);
        }

        // Offline quick-add bar
        theme::term_surface(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("QUICK OFFLINE:")
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_offline_name)
                        .hint_text("username (e.g. Bot04)")
                        .font(egui::FontId::monospace(12.0))
                        .desired_width(180.0),
                );
                let role_lbl = format!(
                    "ROLE: {}",
                    if self.new_offline_role == AccountRole::Bot {
                        "BOT"
                    } else {
                        "PLAYER"
                    }
                );
                let role_tone = if self.new_offline_role == AccountRole::Bot {
                    ButtonTone::Bot
                } else {
                    ButtonTone::Player
                };
                if theme::term_button(ui, palette, &role_lbl, role_tone).clicked() {
                    self.new_offline_role = match self.new_offline_role {
                        AccountRole::Bot => AccountRole::Player,
                        AccountRole::Player => AccountRole::Bot,
                    };
                }
                if theme::term_button(ui, palette, "[+] ADD", ButtonTone::Cta).clicked()
                    && !self.new_offline_name.trim().is_empty()
                {
                    let name = self.new_offline_name.trim().to_string();
                    let role = self.new_offline_role;
                    let acc = Account::new_offline(&name, role);
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                    self.new_offline_name.clear();
                    self.push_toast(
                        if role == AccountRole::Bot {
                            ToastTone::Bot
                        } else {
                            ToastTone::Player
                        },
                        format!("Added offline {role} '{name}'"),
                    );
                }
            });
        });
        ui.add_space(8.0);

        let mut to_remove: Option<String> = None;
        let mut to_refresh: Vec<Account> = Vec::new();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for acc in &self.accounts.accounts {
                let (role_fg, role_bg, role_lbl) = match acc.role {
                    AccountRole::Player => (palette.player, palette.player_bg, "PLAYER"),
                    AccountRole::Bot => (palette.bot, palette.bot_bg, "BOT"),
                };
                let confirming = self.confirm_delete_account.as_deref() == Some(&acc.id);

                theme::term_row(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    let acct_right_w = 210.0_f32;
                    let acct_mid_w = (ui.available_width() - 36.0 - acct_right_w - 20.0).max(160.0);

                    ui.horizontal(|ui| {
                        theme::paint_avatar_tile(ui, &acc.username, role_fg, role_bg, 36.0);
                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(acct_mid_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(acct_mid_w);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(&acc.username)
                                            .monospace()
                                            .size(13.5)
                                            .strong()
                                            .color(palette.ink),
                                    );
                                    theme::term_badge(ui, role_lbl, role_fg, role_bg);
                                    if acc.is_msa() {
                                        theme::term_badge_bordered(
                                            ui,
                                            "MSA",
                                            palette.meta,
                                            palette.meta_bg,
                                            palette.border_hi,
                                        );
                                        if acc.is_token_valid() {
                                            theme::term_badge(
                                                ui,
                                                "● TOKEN OK",
                                                palette.mr,
                                                palette.mr_bg,
                                            );
                                        } else {
                                            theme::term_badge(
                                                ui,
                                                "! NEEDS REFRESH",
                                                palette.warn,
                                                palette.warn_bg,
                                            );
                                        }
                                    } else {
                                        theme::term_badge_bordered(
                                            ui,
                                            "OFFLINE",
                                            palette.ink_muted,
                                            Color32::TRANSPARENT,
                                            palette.border,
                                        );
                                    }
                                });
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("UUID: {}", acc.mc_uuid))
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.ink_muted),
                                    )
                                    .truncate(),
                                );
                            },
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if confirming {
                                if theme::term_button(
                                    ui,
                                    palette,
                                    "CONFIRM DEL",
                                    ButtonTone::Danger,
                                )
                                .clicked()
                                {
                                    to_remove = Some(acc.id.clone());
                                }
                                if theme::term_button(ui, palette, "CANCEL", ButtonTone::Secondary)
                                    .clicked()
                                {
                                    self.confirm_delete_account = None;
                                }
                            } else {
                                if theme::term_button(ui, palette, "DEL", ButtonTone::Danger)
                                    .clicked()
                                {
                                    self.confirm_delete_account = Some(acc.id.clone());
                                }
                                if acc.is_msa()
                                    && theme::term_button(
                                        ui,
                                        palette,
                                        "REFRESH",
                                        ButtonTone::Secondary,
                                    )
                                    .clicked()
                                {
                                    to_refresh.push(acc.clone());
                                }
                            }
                        });
                    });
                });
                ui.add_space(5.0);
            }
        });

        for acc in to_refresh {
            let client_id = self.settings.msa_client_id.clone();
            let tx = self.tx.clone();
            self.rt.spawn(async move {
                let client = MsaAuthClient::new(client_id);
                match client.refresh_account(&acc).await {
                    Ok(refreshed) => {
                        let _ = tx.send(BgEvent::AccountRefreshed(refreshed));
                    }
                    Err(e) => {
                        let _ = tx.send(BgEvent::Toast(
                            ToastTone::Danger,
                            format!("Token refresh failed: {e}"),
                        ));
                    }
                }
            });
        }

        if let Some(id) = to_remove {
            self.confirm_delete_account = None;
            let removed_name = self
                .accounts
                .remove(&id)
                .map(|a| a.username)
                .unwrap_or_else(|| id.clone());
            let _ = self.accounts.save(&self.paths.accounts_file());

            // Unassign from any profiles referencing the deleted account and save profiles.json
            let mut unassigned = 0usize;
            for p in &mut self.profiles.profiles {
                if p.account_id.as_deref() == Some(&id) {
                    p.account_id = None;
                    unassigned += 1;
                }
            }
            if unassigned > 0 {
                let _ = self.profiles.save(&self.paths.profiles_file());
            }
            self.push_toast(
                ToastTone::Danger,
                format!(
                    "Removed account '{removed_name}'{}",
                    if unassigned > 0 {
                        format!(" (unassigned from {unassigned} profile(s))")
                    } else {
                        String::new()
                    }
                ),
            );
        }
    }

    fn ui_console(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        if self
            .console_profile_id
            .as_deref()
            .and_then(|id| self.profiles.get(id))
            .is_none()
        {
            self.console_profile_id = self.profiles.profiles.first().map(|p| p.id.clone());
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("LIVE CONSOLE")
                    .monospace()
                    .size(16.0)
                    .strong()
                    .color(palette.ink),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(pid) = &self.console_profile_id
                    && let Some(st) = self.statuses.get(pid)
                    && theme::term_button(ui, palette, "COPY", ButtonTone::Secondary).clicked()
                {
                    let joined = st.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                    ui.ctx().copy_text(joined);
                    self.push_toast(
                        ToastTone::Focus,
                        format!("Copied {} log lines to clipboard", st.logs.len()),
                    );
                }
                if let Some(pid) = self.console_profile_id.clone()
                    && theme::term_button(ui, palette, "CLEAR", ButtonTone::Subtle).clicked()
                {
                    let mgr = self.manager.clone();
                    self.rt.spawn(async move {
                        mgr.clear_logs(&pid).await;
                    });
                }

                for (lvl, lbl, tone) in [
                    (LogLevelFilter::Err, "ERR", ButtonTone::Danger),
                    (LogLevelFilter::Warn, "WARN", ButtonTone::Warn),
                    (LogLevelFilter::Info, "INFO", ButtonTone::Player),
                    (LogLevelFilter::All, "ALL", ButtonTone::Focus),
                ] {
                    let active = self.console_filter == lvl;
                    if theme::term_button(
                        ui,
                        palette,
                        lbl,
                        if active { tone } else { ButtonTone::Subtle },
                    )
                    .clicked()
                    {
                        self.console_filter = lvl;
                    }
                }

                theme::term_search_bar(
                    ui,
                    palette,
                    &mut self.console_search,
                    "/",
                    "filter...",
                    150.0,
                );

                let sel_label = self
                    .console_profile_id
                    .as_deref()
                    .and_then(|id| self.profiles.get(id))
                    .map(|p| {
                        let running = matches!(
                            self.statuses.get(&p.id).map(|s| &s.state),
                            Some(InstanceState::Running)
                        );
                        format!("{} {}", if running { "●" } else { "○" }, p.name)
                    })
                    .unwrap_or_else(|| "<Select instance>".to_string());

                egui::ComboBox::from_id_salt("console_instance_select")
                    .width(170.0)
                    .selected_text(sel_label)
                    .show_ui(ui, |ui| {
                        for p in &self.profiles.profiles {
                            let running = matches!(
                                self.statuses.get(&p.id).map(|s| &s.state),
                                Some(InstanceState::Running)
                            );
                            ui.selectable_value(
                                &mut self.console_profile_id,
                                Some(p.id.clone()),
                                format!("{} {}", if running { "●" } else { "○" }, p.name),
                            );
                        }
                    });
            });
        });
        theme::term_divider(ui, palette);
        ui.add_space(4.0);

        let selected_status = self
            .console_profile_id
            .as_deref()
            .and_then(|id| self.statuses.get(id));
        let is_running = selected_status
            .map(|s| matches!(s.state, InstanceState::Running))
            .unwrap_or(false);
        let title = selected_status
            .map(|s| format!("{} ({})", s.profile_name, s.account_name))
            .unwrap_or_else(|| {
                self.console_profile_id
                    .as_deref()
                    .and_then(|id| self.profiles.get(id))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "No instance selected".to_string())
            });
        let q = self.console_search.trim().to_lowercase();

        theme::term_box(
            ui,
            palette.bg,
            palette.border_hi,
            Margin::same(0),
            |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());

                theme::term_box(
                    ui,
                    palette.panel,
                    palette.border,
                    Margin::symmetric(12, 6),
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("● STREAMING stdout/stderr — {title}"))
                                    .monospace()
                                    .size(11.5)
                                    .color(if is_running {
                                        palette.mr
                                    } else {
                                        palette.ink_muted
                                    }),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let count = selected_status.map(|s| s.logs.len()).unwrap_or(0);
                                    ui.label(
                                        RichText::new(format!("{count} / 2000 LINES · AUTO-SCROLL ON"))
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.ink_muted),
                                    );
                                },
                            );
                        });
                    },
                );

                egui::Frame::NONE
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(ui.available_height());

                        if let Some(st) = selected_status {
                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for line in &st.logs {
                                        if !matches_log_filter(line, self.console_filter, &q) {
                                            continue;
                                        }
                                        let col = log_line_color(line, palette);
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(line)
                                                    .monospace()
                                                    .size(12.0)
                                                    .color(col),
                                            )
                                            .wrap(),
                                        );
                                    }
                                    render_blinking_cursor(ui, palette);
                                });
                        } else {
                            ui.label(
                                RichText::new(
                                    "Select and launch an instance to stream stdout/stderr here.",
                                )
                                .monospace()
                                .size(12.0)
                                .color(palette.ink_muted),
                            );
                            render_blinking_cursor(ui, palette);
                        }
                    });
            },
        );
    }

    fn ui_config(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, palette: &Palette) {
        ui.label(
            RichText::new("CONFIGURATION")
                .monospace()
                .size(16.0)
                .strong()
                .color(palette.ink),
        );
        ui.label(
            RichText::new(
                "Terminal color palettes, CurseForge API v1 key, Java auto-detection, and swarm tuning.",
            )
            .monospace()
            .size(11.5)
            .color(palette.ink_muted),
        );
        theme::term_divider(ui, palette);
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // 1. Terminal Color Palettes (persisted immediately)
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                theme::section_header(
                    ui,
                    palette,
                    "TERMINAL COLOR PALETTE (PERSISTED)",
                    palette.focus,
                );
                ui.add_space(8.0);
                let mut chosen = self.theme_preset;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                    for preset in ThemePreset::ALL {
                        if theme::theme_swatch_card(
                            ui,
                            palette,
                            preset,
                            self.theme_preset == preset,
                        )
                        .clicked()
                        {
                            chosen = preset;
                        }
                    }
                });
                if chosen != self.theme_preset {
                    self.set_theme_preset(ctx, chosen);
                }
            });

            ui.add_space(10.0);

            // 2. CurseForge API v1 + Java & Swarm Engine
            ui.columns(2, |cols| {
                // Left: CurseForge API v1
                cols[0].vertical(|ui| {
                    theme::term_surface(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            theme::section_header(ui, palette, "CURSEFORGE API v1", palette.cf);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    match self.cf_key_status {
                                        CfKeyStatus::Valid => {
                                            theme::term_badge(
                                                ui,
                                                "CONNECTED",
                                                palette.mr,
                                                palette.mr_bg,
                                            );
                                        }
                                        CfKeyStatus::Testing => {
                                            theme::term_badge(
                                                ui,
                                                "TESTING...",
                                                palette.focus,
                                                palette.focus_bg,
                                            );
                                        }
                                        CfKeyStatus::Invalid => {
                                            theme::term_badge(
                                                ui,
                                                "INVALID KEY",
                                                palette.danger,
                                                palette.danger_bg,
                                            );
                                        }
                                        CfKeyStatus::Untested => {
                                            theme::term_badge(
                                                ui,
                                                "KEY SET",
                                                palette.cf,
                                                palette.cf_bg,
                                            );
                                        }
                                        CfKeyStatus::Unset => {
                                            theme::term_badge(
                                                ui,
                                                "NOT SET",
                                                palette.warn,
                                                palette.warn_bg,
                                            );
                                        }
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("API KEY (x-api-key or env MUB_CURSEFORGE_API_KEY)")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.curseforge_api_key)
                                    .password(!self.show_cf_key)
                                    .hint_text("paste CurseForge API v1 key...")
                                    .font(egui::FontId::monospace(12.0))
                                    .desired_width((ui.available_width() - 140.0).max(100.0)),
                            );
                            if resp.changed() {
                                self.cf_key_status =
                                    if self.settings.effective_curseforge_api_key().is_some() {
                                        CfKeyStatus::Untested
                                    } else {
                                        CfKeyStatus::Unset
                                    };
                                self.mods_view.invalidate_catalog();
                            }

                            let eye_lbl = if self.show_cf_key { "HIDE" } else { "SHOW" };
                            if theme::term_button(ui, palette, eye_lbl, ButtonTone::Secondary)
                                .clicked()
                            {
                                self.show_cf_key = !self.show_cf_key;
                            }

                            let testing = self.cf_key_status == CfKeyStatus::Testing;
                            if ui
                                .add_enabled_ui(!testing, |ui| {
                                    theme::term_button(ui, palette, "TEST", ButtonTone::CurseForge)
                                })
                                .inner
                                .clicked()
                            {
                                if let Some(key) = self.settings.effective_curseforge_api_key() {
                                    self.cf_key_status = CfKeyStatus::Testing;
                                    let tx = self.tx.clone();
                                    let ctx = ctx.clone();
                                    self.rt.spawn(async move {
                                        let client = CurseForgeClient::new(key);
                                        let res = client
                                            .test_api_key()
                                            .await
                                            .map_err(|e| e.to_string());
                                        let _ = tx.send(BgEvent::CfKeyTested(res));
                                        ctx.request_repaint();
                                    });
                                } else {
                                    self.cf_key_status = CfKeyStatus::Unset;
                                    self.push_toast(
                                        ToastTone::Warn,
                                        "Enter a CurseForge API key or set MUB_CURSEFORGE_API_KEY first.",
                                    );
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Unlocks CurseForge mod/modpack/shader search, Murmur2 fingerprint update checks, and CF .zip pack import.",
                            )
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                        );
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Get key:")
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.ink_muted),
                            );
                            ui.hyperlink_to(
                                RichText::new("https://console.curseforge.com/")
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.player),
                                "https://console.curseforge.com/",
                            );
                        });
                    });
                });

                // Right: Java & Swarm Engine
                cols[1].vertical(|ui| {
                    theme::term_surface(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            theme::section_header(
                                ui,
                                palette,
                                "JAVA & SWARM ENGINE",
                                palette.meta,
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let lbl = if self.detecting_javas {
                                        "[↻] SCANNING..."
                                    } else {
                                        "[↻] AUTO-DETECT"
                                    };
                                    if ui
                                        .add_enabled_ui(!self.detecting_javas, |ui| {
                                            theme::term_button(
                                                ui,
                                                palette,
                                                lbl,
                                                ButtonTone::Player,
                                            )
                                        })
                                        .inner
                                        .clicked()
                                    {
                                        self.detecting_javas = true;
                                        let tx = self.tx.clone();
                                        let ctx = ctx.clone();
                                        self.rt.spawn(async move {
                                            let javas =
                                                tokio::task::spawn_blocking(detect_java_installations)
                                                    .await
                                                    .unwrap_or_default();
                                            let _ = tx.send(BgEvent::JavasDetected(javas));
                                            ctx.request_repaint();
                                        });
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);

                        // Shared Data Directory + [DIR] OPEN
                        ui.label(
                            RichText::new("SHARED DATA DIRECTORY")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.horizontal(|ui| {
                            let path_w = (ui.available_width() - 108.0).max(100.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(path_w, 20.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(self.paths.root().display().to_string())
                                                .monospace()
                                                .size(11.5)
                                                .color(palette.meta),
                                        )
                                        .truncate(),
                                    );
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::term_button(
                                        ui,
                                        palette,
                                        "[DIR] OPEN",
                                        ButtonTone::Secondary,
                                    )
                                    .clicked()
                                    {
                                        let _ = open_in_file_manager(self.paths.root());
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);

                        // Detected Java Runtimes
                        if !self.detected_javas.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "DETECTED RUNTIMES ({}) — CLICK TO APPLY AS DEFAULT",
                                    self.detected_javas.len()
                                ))
                                .monospace()
                                .size(10.5)
                                .color(palette.ink_muted),
                            );
                            let mut picked_java: Option<(String, u32, String)> = None;
                            for j in &self.detected_javas {
                                let j_path = j.path.to_string_lossy().into_owned();
                                let is_cur = self.settings.default_java_path == j_path;
                                theme::term_row(ui, palette, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let lbl_w = (ui.available_width() - 90.0).max(100.0);
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(lbl_w, 20.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(format!(
                                                            "JAVA {} · {} {}",
                                                            j.major, j.vendor, j.version
                                                        ))
                                                        .monospace()
                                                        .size(11.0)
                                                        .strong()
                                                        .color(palette.ink),
                                                    )
                                                    .truncate(),
                                                );
                                            },
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if theme::term_button(
                                                    ui,
                                                    palette,
                                                    if is_cur { "ACTIVE" } else { "USE" },
                                                    if is_cur {
                                                        ButtonTone::Focus
                                                    } else {
                                                        ButtonTone::Secondary
                                                    },
                                                )
                                                .clicked()
                                                {
                                                    picked_java = Some((
                                                        j_path.clone(),
                                                        j.major,
                                                        j.version.clone(),
                                                    ));
                                                }
                                            },
                                        );
                                    });
                                });
                                ui.add_space(2.0);
                            }
                            if let Some((path, major, ver)) = picked_java {
                                self.settings.default_java_path = path;
                                self.push_toast(
                                    ToastTone::Player,
                                    format!("Default Java -> Java {major} ({ver})"),
                                );
                            }
                            ui.add_space(4.0);
                        }

                        ui.label(
                            RichText::new("DEFAULT JAVA PATH")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.default_java_path)
                                .font(egui::FontId::monospace(11.5))
                                .desired_width(ui.available_width()),
                        );
                        ui.add_space(6.0);

                        ui.label(
                            RichText::new("MSA CLIENT ID")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.msa_client_id)
                                .font(egui::FontId::monospace(11.5))
                                .desired_width(ui.available_width()),
                        );
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("BATCH STAGGER (MS)")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut self.settings.batch_launch_stagger_ms)
                                        .speed(50),
                                );
                            });
                            ui.add_space(16.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("MAX CONCURRENT DL")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::DragValue::new(
                                        &mut self.settings.max_concurrent_downloads,
                                    )
                                    .range(1..=64),
                                );
                            });
                        });

                        ui.add_space(10.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::term_button(ui, palette, "SAVE CONFIG", ButtonTone::Cta)
                                .clicked()
                            {
                                self.settings.theme_palette = self.theme_preset.id().to_string();
                                match self.settings.save(&self.paths.settings_file()) {
                                    Ok(()) => {
                                        self.mods_view.invalidate_catalog();
                                        self.push_toast(
                                            ToastTone::Focus,
                                            "Configuration saved to settings.json",
                                        );
                                    }
                                    Err(e) => {
                                        self.push_toast(
                                            ToastTone::Danger,
                                            format!("Failed to save settings: {e}"),
                                        );
                                    }
                                }
                            }
                        });
                    });
                });
            });

            ui.add_space(12.0);
            self.render_mcp_config_card(ui, palette);

            ui.add_space(12.0);
            self.render_app_updates_card(ui, ctx, palette);
        });
    }

    fn render_mcp_config_card(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        let exe_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { "mub.exe" } else { "mub" })))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mub".to_string());

        let escaped = exe_path.replace('\\', "\\\\");
        let snippet = format!(
            "{{\n  \"mcpServers\": {{\n    \"modsundbots\": {{\n      \"command\": \"{escaped}\",\n      \"args\": [\"mcp\"]\n    }}\n  }}\n}}"
        );

        theme::term_surface(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            theme::section_header(ui, palette, "MCP SERVER (LLM AUTOMATION)", palette.focus);
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Control ModsUndBots from Claude, Cursor, Antigravity, or any MCP client (`mub mcp` over stdio). Changes sync live into the GUI.",
                )
                .monospace()
                .size(11.0)
                .color(palette.ink_muted),
            );
            ui.add_space(6.0);
            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(&snippet)
                        .monospace()
                        .size(11.0)
                        .color(palette.ink),
                );
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::term_button(ui, palette, "COPY MCP CONFIG JSON", ButtonTone::Focus)
                    .clicked()
                {
                    ui.ctx().copy_text(snippet);
                    self.push_toast(
                        ToastTone::Focus,
                        "Copied MCP server config JSON to clipboard",
                    );
                }
            });
        });
    }

    fn ui_collections(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, palette: &Palette) {
        // Header
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("▸ BOT COLLECTIONS")
                            .monospace()
                            .size(16.0)
                            .strong()
                            .color(palette.bot),
                    );
                });
                ui.label(
                    RichText::new(
                        "Group bot instances into named collections with one-click 'Start All' staggered launching.",
                    )
                    .monospace()
                    .size(11.5)
                    .color(palette.ink_muted),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::term_button(ui, palette, "[+] NEW COLLECTION", ButtonTone::Cta).clicked()
                {
                    self.show_new_collection = !self.show_new_collection;
                }
            });
        });

        theme::term_divider(ui, palette);
        ui.add_space(4.0);

        // Inline New Collection drawer
        if self.show_new_collection {
            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::section_header(ui, palette, "CREATE BOT COLLECTION", palette.bot);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::term_button(ui, palette, "CLOSE", ButtonTone::Subtle).clicked() {
                            self.show_new_collection = false;
                        }
                    });
                });
                ui.add_space(6.0);
                egui::Grid::new("new_col_grid")
                    .num_columns(2)
                    .spacing([14.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("COLLECTION NAME")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_col_name)
                                .hint_text("e.g. FarmSquad")
                                .desired_width(280.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("DESCRIPTION")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_col_desc)
                                .hint_text("Optional notes...")
                                .desired_width(360.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("STAGGER DELAY (MS)")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_col_stagger)
                                .hint_text(&format!(
                                    "default ({} ms)",
                                    self.settings.batch_launch_stagger_ms
                                ))
                                .desired_width(180.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("INITIAL MEMBERS")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                        ui.checkbox(
                            &mut self.new_col_include_all_bots,
                            RichText::new("Auto-add all existing Bot profiles")
                                .monospace()
                                .size(11.5)
                                .color(palette.ink),
                        );
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::term_button(ui, palette, "CREATE COLLECTION", ButtonTone::Bot)
                        .clicked()
                    {
                        let name = self.new_col_name.trim().to_string();
                        if name.is_empty() {
                            self.push_toast(ToastTone::Danger, "Collection name cannot be empty");
                        } else if self.collections.find_by_name(&name).is_some() {
                            self.push_toast(
                                ToastTone::Danger,
                                format!("Collection '{name}' already exists"),
                            );
                        } else {
                            let mut col = BotCollection::new(&name);
                            col.description = self.new_col_desc.trim().to_string();
                            if let Ok(ms) = self.new_col_stagger.trim().parse::<u64>() {
                                col.stagger_ms = Some(ms);
                            }
                            if self.new_col_include_all_bots {
                                for p in &self.profiles.profiles {
                                    if p.kind == ProfileKind::Bot {
                                        col.add_profile(&p.id);
                                    }
                                }
                            }
                            self.collections.upsert(col);
                            let _ = self.collections.save(&self.paths.collections_file());
                            self.collections_mtime = file_mtime(&self.paths.collections_file());
                            self.new_col_name.clear();
                            self.new_col_desc.clear();
                            self.new_col_stagger.clear();
                            self.show_new_collection = false;
                            self.push_toast(
                                ToastTone::Bot,
                                format!("Created collection '{name}'"),
                            );
                        }
                    }
                });
            });
            ui.add_space(8.0);
        }

        if self.collections.collections.is_empty() {
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    ui.label(
                        RichText::new("NO BOT COLLECTIONS YET")
                            .monospace()
                            .size(14.0)
                            .strong()
                            .color(palette.ink),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Click '[+] NEW COLLECTION' above (or ask an LLM via MCP) to group bots for one-click 'Start All'.",
                        )
                        .monospace()
                        .size(11.5)
                        .color(palette.ink_muted),
                    );
                    ui.add_space(28.0);
                });
            });
            return;
        }

        let mut start_col_id: Option<String> = None;
        let mut stop_col_id: Option<String> = None;
        let mut delete_col_id: Option<String> = None;
        let mut launch_single: Option<Profile> = None;
        let mut stop_single: Option<String> = None;
        let mut open_logs: Option<String> = None;
        let mut toggle_member: Option<(String, String, bool)> = None;
        let mut add_all_bots_to: Option<String> = None;
        let mut save_edit_id: Option<String> = None;

        let cols_snapshot = self.collections.collections.clone();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for col in &cols_snapshot {
                let running_in_col = col
                    .profile_ids
                    .iter()
                    .filter(|pid| {
                        matches!(
                            self.statuses.get(*pid).map(|s| &s.state),
                            Some(InstanceState::Running | InstanceState::Preparing)
                        )
                    })
                    .count();
                let is_editing = self.editing_collection_id.as_deref() == Some(col.id.as_str());
                let stagger_eff = col.stagger_ms.unwrap_or(self.settings.batch_launch_stagger_ms);

                theme::term_surface(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&col.name)
                                        .monospace()
                                        .size(14.5)
                                        .strong()
                                        .color(palette.ink),
                                );
                                theme::term_badge(
                                    ui,
                                    &format!("{} BOTS", col.profile_ids.len()),
                                    palette.bot,
                                    palette.bot_bg,
                                );
                                if running_in_col > 0 {
                                    theme::term_badge(
                                        ui,
                                        &format!("{running_in_col} RUNNING"),
                                        palette.mr,
                                        palette.mr_bg,
                                    );
                                }
                                let launch_mode_label = if stagger_eff == 0 {
                                    "parallel".to_string()
                                } else {
                                    format!("stagger: {stagger_eff}ms")
                                };
                                ui.label(
                                    RichText::new(launch_mode_label)
                                        .monospace()
                                        .size(10.5)
                                        .color(palette.ink_muted),
                                );
                            });
                            if !col.description.trim().is_empty() {
                                ui.label(
                                    RichText::new(&col.description)
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::term_button(ui, palette, "[×] DELETE", ButtonTone::Danger)
                                .clicked()
                            {
                                delete_col_id = Some(col.id.clone());
                            }
                            if theme::term_button(
                                ui,
                                palette,
                                if is_editing { "DONE" } else { "[✎] EDIT" },
                                if is_editing {
                                    ButtonTone::Focus
                                } else {
                                    ButtonTone::Secondary
                                },
                            )
                            .clicked()
                            {
                                if is_editing {
                                    self.editing_collection_id = None;
                                } else {
                                    self.editing_collection_id = Some(col.id.clone());
                                    self.edit_col_name = col.name.clone();
                                    self.edit_col_desc = col.description.clone();
                                    self.edit_col_stagger = col
                                        .stagger_ms
                                        .map(|v| v.to_string())
                                        .unwrap_or_default();
                                }
                            }
                            if running_in_col > 0
                                && theme::term_button(
                                    ui,
                                    palette,
                                    "[■] STOP ALL",
                                    ButtonTone::Danger,
                                )
                                .clicked()
                            {
                                stop_col_id = Some(col.id.clone());
                            }
                            if theme::term_button(ui, palette, "[▶▶] START ALL", ButtonTone::Bot)
                                .clicked()
                            {
                                start_col_id = Some(col.id.clone());
                            }
                        });
                    });

                    if is_editing {
                        ui.add_space(8.0);
                        theme::term_raised(ui, palette, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("NAME:")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_col_name)
                                        .desired_width(160.0),
                                );
                                ui.label(
                                    RichText::new("DESC:")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_col_desc)
                                        .desired_width(200.0),
                                );
                                ui.label(
                                    RichText::new("STAGGER MS:")
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_col_stagger)
                                        .hint_text("default")
                                        .desired_width(70.0),
                                );
                                if theme::term_button(ui, palette, "SAVE", ButtonTone::Cta)
                                    .clicked()
                                {
                                    save_edit_id = Some(col.id.clone());
                                }
                                if theme::term_button(
                                    ui,
                                    palette,
                                    "+ ALL BOTS",
                                    ButtonTone::Bot,
                                )
                                .clicked()
                                {
                                    add_all_bots_to = Some(col.id.clone());
                                }
                            });

                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("MEMBERS (CLICK TO TOGGLE):")
                                    .monospace()
                                    .size(10.5)
                                    .color(palette.ink_muted),
                            );
                            ui.horizontal_wrapped(|ui| {
                                for p in &self.profiles.profiles {
                                    let included = col.profile_ids.iter().any(|id| id == &p.id);
                                    let acc = p
                                        .account_id
                                        .as_deref()
                                        .and_then(|id| self.accounts.get(id))
                                        .map(|a| a.username.as_str())
                                        .unwrap_or("UNASSIGNED");
                                    let lbl = if included {
                                        format!("[✓] {} ({acc})", p.name)
                                    } else {
                                        format!("[ ] {} ({acc})", p.name)
                                    };
                                    let tone = if included {
                                        ButtonTone::Bot
                                    } else {
                                        ButtonTone::Subtle
                                    };
                                    if theme::term_button(ui, palette, &lbl, tone).clicked() {
                                        toggle_member =
                                            Some((col.id.clone(), p.id.clone(), !included));
                                    }
                                }
                            });
                        });
                    }

                    if !col.profile_ids.is_empty() {
                        ui.add_space(6.0);
                        for pid in &col.profile_ids {
                            let Some(p) = self.profiles.get(pid) else {
                                continue;
                            };
                            let acc = p
                                .account_id
                                .as_deref()
                                .and_then(|id| self.accounts.get(id))
                                .map(|a| a.username.as_str())
                                .unwrap_or("UNASSIGNED");
                            let st = self.statuses.get(&p.id);
                            let active = matches!(
                                st.map(|s| &s.state),
                                Some(InstanceState::Running | InstanceState::Preparing)
                            );
                            let state_str = match st.map(|s| &s.state) {
                                Some(InstanceState::Preparing) => "PREPARING",
                                Some(InstanceState::Running) => "RUNNING",
                                Some(InstanceState::Failed { .. }) => "FAILED",
                                _ => "IDLE",
                            };

                            theme::term_row(ui, palette, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&p.name)
                                            .monospace()
                                            .size(12.0)
                                            .strong()
                                            .color(palette.ink),
                                    );
                                    ui.label(
                                        RichText::new(format!("· {acc}"))
                                            .monospace()
                                            .size(11.0)
                                            .color(if acc == "UNASSIGNED" {
                                                palette.danger
                                            } else {
                                                palette.player
                                            }),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "· {} · {}",
                                            p.mc_version, p.mod_loader
                                        ))
                                        .monospace()
                                        .size(10.5)
                                        .color(palette.ink_muted),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if theme::term_button(
                                                ui,
                                                palette,
                                                "LOGS",
                                                ButtonTone::Subtle,
                                            )
                                            .clicked()
                                            {
                                                open_logs = Some(p.id.clone());
                                            }
                                            if active {
                                                if theme::term_button(
                                                    ui,
                                                    palette,
                                                    "[■] STOP",
                                                    ButtonTone::Danger,
                                                )
                                                .clicked()
                                                {
                                                    stop_single = Some(p.id.clone());
                                                }
                                            } else if theme::term_button(
                                                ui,
                                                palette,
                                                "[▶] START",
                                                ButtonTone::Focus,
                                            )
                                            .clicked()
                                            {
                                                launch_single = Some(p.clone());
                                            }
                                            ui.label(
                                                RichText::new(state_str)
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(if active {
                                                        palette.mr
                                                    } else {
                                                        palette.ink_muted
                                                    }),
                                            );
                                        },
                                    );
                                });
                            });
                            ui.add_space(2.0);
                        }
                    }
                });
                ui.add_space(8.0);
            }
        });

        if let Some(cid) = start_col_id {
            self.launch_collection(&cid);
        }
        if let Some(cid) = stop_col_id {
            self.stop_collection(&cid);
        }
        if let Some(cid) = delete_col_id {
            self.collections.remove(&cid);
            let _ = self.collections.save(&self.paths.collections_file());
            self.collections_mtime = file_mtime(&self.paths.collections_file());
            if self.editing_collection_id.as_deref() == Some(cid.as_str()) {
                self.editing_collection_id = None;
            }
        }
        if let Some(p) = launch_single {
            self.launch_profiles(vec![p]);
        }
        if let Some(pid) = stop_single {
            self.stop_profile(pid);
        }
        if let Some(pid) = open_logs {
            self.console_profile_id = Some(pid);
            self.active_view = ActiveView::Console;
        }
        if let Some((cid, pid, add)) = toggle_member {
            if let Some(c) = self.collections.get_mut(&cid) {
                if add {
                    c.add_profile(pid);
                } else {
                    c.remove_profile(&pid);
                }
            }
            let _ = self.collections.save(&self.paths.collections_file());
            self.collections_mtime = file_mtime(&self.paths.collections_file());
        }
        if let Some(cid) = add_all_bots_to {
            if let Some(c) = self.collections.get_mut(&cid) {
                for p in &self.profiles.profiles {
                    if p.kind == ProfileKind::Bot {
                        c.add_profile(&p.id);
                    }
                }
            }
            let _ = self.collections.save(&self.paths.collections_file());
            self.collections_mtime = file_mtime(&self.paths.collections_file());
        }
        if let Some(cid) = save_edit_id {
            if let Some(c) = self.collections.get_mut(&cid) {
                let n = self.edit_col_name.trim();
                if !n.is_empty() {
                    c.name = n.to_string();
                }
                c.description = self.edit_col_desc.trim().to_string();
                c.stagger_ms = self.edit_col_stagger.trim().parse::<u64>().ok();
            }
            let _ = self.collections.save(&self.paths.collections_file());
            self.collections_mtime = file_mtime(&self.paths.collections_file());
            self.push_toast(ToastTone::Focus, "Collection updated");
        }
    }

    fn trigger_app_update_check(&mut self, ctx: &egui::Context, silent: bool) {
        if self.checking_app_update || self.installing_app_update {
            return;
        }
        self.checking_app_update = true;
        self.app_update_error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let token = self.settings.effective_github_update_token();
        self.rt.spawn(async move {
            let res = check_for_updates(token.as_deref())
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(BgEvent::AppUpdateChecked { silent, res });
            ctx.request_repaint();
        });
    }

    fn trigger_app_update_install(&mut self, ctx: &egui::Context) {
        let Some(check) = self.app_update_check.clone() else {
            return;
        };
        if self.installing_app_update || !check.update_available {
            return;
        }
        self.installing_app_update = true;
        self.app_update_error = None;
        self.task_progress = Some((0.35, format!("Downloading {}...", check.tag_name)));
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let token = self.settings.effective_github_update_token();
        self.rt.spawn(async move {
            let res = download_and_install_update(&check, token.as_deref())
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(BgEvent::AppUpdateInstalled(res));
            ctx.request_repaint();
        });
    }

    fn render_app_updates_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &Palette,
    ) {
        theme::term_surface(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::section_header(ui, palette, "APP UPDATES (GITHUB RELEASES)", palette.focus);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::term_badge(
                        ui,
                        &format!("v{APP_VERSION}"),
                        palette.ink,
                        palette.surface,
                    );
                    if let Some(installed) = &self.app_update_installed {
                        theme::term_badge(
                            ui,
                            &format!("INSTALLED v{}", installed.new_version),
                            palette.focus,
                            palette.focus_bg,
                        );
                    } else if let Some(check) = &self.app_update_check {
                        if check.update_available {
                            theme::term_badge(
                                ui,
                                &format!("AVAILABLE {}", check.tag_name),
                                palette.warn,
                                palette.warn_bg,
                            );
                        } else {
                            theme::term_badge(ui, "UP TO DATE", palette.focus, palette.focus_bg);
                        }
                    }
                });
            });

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let busy = self.checking_app_update || self.installing_app_update;
                let check_lbl = if self.checking_app_update {
                    "[↻] CHECKING..."
                } else {
                    "[↻] CHECK FOR UPDATES"
                };
                if ui
                    .add_enabled_ui(!busy, |ui| {
                        theme::term_button(ui, palette, check_lbl, ButtonTone::Secondary)
                    })
                    .inner
                    .clicked()
                {
                    self.trigger_app_update_check(ctx, false);
                }

                if let Some(check) = &self.app_update_check
                    && check.update_available
                    && self.app_update_installed.is_none()
                {
                    let install_lbl = if self.installing_app_update {
                        "INSTALLING..."
                    } else {
                        "DOWNLOAD & INSTALL"
                    };
                    if ui
                        .add_enabled_ui(!busy && check.asset_name.is_some(), |ui| {
                            theme::term_button(ui, palette, install_lbl, ButtonTone::Cta)
                        })
                        .inner
                        .clicked()
                    {
                        self.trigger_app_update_install(ctx);
                    }
                }

                if self.app_update_installed.is_some()
                    && theme::term_button(ui, palette, "RESTART NOW", ButtonTone::Focus).clicked()
                {
                    match spawn_updated_relaunch() {
                        Ok(()) => std::process::exit(0),
                        Err(e) => {
                            self.push_toast(ToastTone::Danger, format!("Relaunch failed: {e}"));
                        }
                    }
                }
            });

            if let Some(err) = &self.app_update_error {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(err)
                        .monospace()
                        .size(11.0)
                        .color(palette.danger),
                );
            } else if let Some(check) = &self.app_update_check {
                ui.add_space(6.0);
                let asset_info = check
                    .asset_name
                    .as_deref()
                    .unwrap_or("no matching binary for this OS/arch");
                ui.label(
                    RichText::new(format!(
                        "Latest: {}  ·  Asset: {asset_info}",
                        check.tag_name
                    ))
                    .monospace()
                    .size(11.0)
                    .color(palette.ink_muted),
                );
                if !check.release_notes.trim().is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(check.release_notes.trim())
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_dim),
                    );
                }
            }

            ui.add_space(8.0);
            ui.label(
                RichText::new("GITHUB TOKEN (OPTIONAL — REQUIRED FOR PRIVATE REPO, OR ENV MUB_GITHUB_TOKEN)")
                    .monospace()
                    .size(11.0)
                    .color(palette.ink_muted),
            );
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.github_update_token)
                        .password(!self.show_gh_token)
                        .hint_text("ghp_... or github_pat_... (Contents: Read)")
                        .font(egui::FontId::monospace(12.0))
                        .desired_width((ui.available_width() - 80.0).max(120.0)),
                );
                let eye_lbl = if self.show_gh_token { "HIDE" } else { "SHOW" };
                if theme::term_button(ui, palette, eye_lbl, ButtonTone::Secondary).clicked() {
                    self.show_gh_token = !self.show_gh_token;
                }
            });
        });
    }
}

fn cmd_row(ui: &mut egui::Ui, palette: &Palette, label: &str, tag: &str, tag_col: Color32) -> bool {
    let mut clicked = false;
    let resp = theme::term_row(ui, palette, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(label)
                            .monospace()
                            .size(12.0)
                            .color(palette.ink),
                    )
                    .frame(false),
                )
                .clicked()
            {
                clicked = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(tag)
                        .monospace()
                        .size(11.0)
                        .strong()
                        .color(tag_col),
                );
            });
        });
    });
    if resp.response.interact(egui::Sense::click()).clicked() {
        clicked = true;
    }
    clicked
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([980.0, 640.0])
            .with_title("ModsUndBots v0.2.0 — Player & Bot Launcher")
            .with_icon(theme::build_window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "ModsUndBots",
        options,
        Box::new(|cc| Ok(Box::new(MubApp::new(cc)))),
    )
}
