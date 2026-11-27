use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, Margin, RichText};
use mub_core::{
    AccountStore, AppPaths, CatalogClient, ContentKind, ContentSource, ContentUpdateInfo,
    InstanceManager, InstanceState, InstanceStatus, JavaInstallation, LauncherSettings, ModLoader,
    Profile, ProfileKind, ProfileStore, export_mrpack, list_installed_content, list_saved_worlds,
    open_in_file_manager, remove_installed_content, sync_content_to_profiles,
    toggle_installed_content,
};
use tokio::sync::mpsc;

use crate::theme::{self, ButtonTone, Palette};
use crate::{LoaderChoice, ToastTone, VersionPickerCache};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceSubTab {
    Content,
    WorldsFiles,
    Logs,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentFilterKind {
    All,
    Mod,
    ResourcePack,
    Shader,
    DataPack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevelFilter {
    All,
    Info,
    Warn,
    Err,
}

pub enum InstanceAction {
    Back,
    AddContent(String),
    Launch(Profile),
    Stop(String),
    ProfileUpdated(Profile),
    ProfileCreated(Profile),
    ProfileDeleted { id: String, delete_files: bool },
    UpdateCountChanged { profile_id: String, count: usize },
    Toast(ToastTone, String),
    TaskProgress(Option<(f32, String)>),
}

enum InstBgEvent {
    UpdatesChecked {
        profile_id: String,
        updates: HashMap<(ContentKind, String), ContentUpdateInfo>,
    },
    SingleUpdated {
        profile_id: String,
        kind: ContentKind,
        old_filename: String,
        new_filename: String,
    },
    AllUpdated {
        profile_id: String,
        count: usize,
    },
    Error(String),
}

struct SettingsDraft {
    profile_id: String,
    name: String,
    kind: ProfileKind,
    group: String,
    mc_version: String,
    show_snapshots: bool,
    mc_search: String,
    loader_choice: LoaderChoice,
    loader_version: String,
    account_id: Option<String>,
    memory_min_mb: u32,
    memory_max_mb: u32,
    java_override: String,
    jvm_args_text: String,
    use_resolution: bool,
    res_width: u32,
    res_height: u32,
    quick_connect: String,
}

impl SettingsDraft {
    fn from_profile(p: &Profile) -> Self {
        let (loader_choice, loader_version) = match &p.mod_loader {
            ModLoader::Vanilla => (LoaderChoice::Vanilla, String::new()),
            ModLoader::Fabric { loader_version } => (LoaderChoice::Fabric, loader_version.clone()),
            ModLoader::Quilt { loader_version } => (LoaderChoice::Quilt, loader_version.clone()),
            ModLoader::NeoForge { loader_version } => {
                (LoaderChoice::NeoForge, loader_version.clone())
            }
            ModLoader::Forge { loader_version } => (LoaderChoice::Forge, loader_version.clone()),
        };
        let (use_resolution, res_width, res_height) = match p.resolution {
            Some((w, h)) => (true, w, h),
            None => (false, 1280, 720),
        };
        Self {
            profile_id: p.id.clone(),
            name: p.name.clone(),
            kind: p.kind,
            group: p.group.clone().unwrap_or_default(),
            mc_version: p.mc_version.clone(),
            show_snapshots: false,
            mc_search: String::new(),
            loader_choice,
            loader_version,
            account_id: p.account_id.clone(),
            memory_min_mb: p.memory_min_mb,
            memory_max_mb: p.memory_max_mb,
            java_override: p.java_path.clone().unwrap_or_default(),
            jvm_args_text: p.jvm_args.join(" "),
            use_resolution,
            res_width,
            res_height,
            quick_connect: p.quick_connect.clone().unwrap_or_default(),
        }
    }
}

pub struct InstanceDetailState {
    pub profile_id: String,
    pub sub_tab: InstanceSubTab,
    content_filter: ContentFilterKind,
    log_filter: LogLevelFilter,
    log_search: String,

    // Export .mrpack drawer
    show_export: bool,
    export_dest: String,
    export_version: String,
    export_summary: String,

    // Content update tracking for current profile
    checking_updates: bool,
    updating_items: HashSet<(ContentKind, String)>,
    updates: HashMap<(ContentKind, String), ContentUpdateInfo>,

    // Settings editor draft & 2-step delete
    draft: Option<SettingsDraft>,
    confirm_delete: bool,
    delete_disk_files: bool,

    tx: mpsc::UnboundedSender<InstBgEvent>,
    rx: mpsc::UnboundedReceiver<InstBgEvent>,
}

impl InstanceDetailState {
    pub fn new(profile_id: impl Into<String>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            profile_id: profile_id.into(),
            sub_tab: InstanceSubTab::Content,
            content_filter: ContentFilterKind::All,
            log_filter: LogLevelFilter::All,
            log_search: String::new(),
            show_export: false,
            export_dest: String::new(),
            export_version: "1.0.0".to_string(),
            export_summary: String::new(),
            checking_updates: false,
            updating_items: HashSet::new(),
            updates: HashMap::new(),
            draft: None,
            confirm_delete: false,
            delete_disk_files: false,
            tx,
            rx,
        }
    }

    pub fn open_profile(&mut self, profile: &Profile, paths: &AppPaths) {
        self.profile_id = profile.id.clone();
        self.confirm_delete = false;
        self.delete_disk_files = false;
        self.show_export = false;
        self.export_dest = paths
            .root()
            .join(format!("{}.mrpack", sanitize_filename(&profile.name)))
            .display()
            .to_string();
        self.export_version = "1.0.0".to_string();
        self.export_summary.clear();
        self.updates.clear();
        self.updating_items.clear();
        self.checking_updates = false;
        self.draft = Some(SettingsDraft::from_profile(profile));
    }

    fn drain_events(&mut self, actions: &mut Vec<InstanceAction>) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                InstBgEvent::UpdatesChecked {
                    profile_id,
                    updates,
                } => {
                    self.checking_updates = false;
                    actions.push(InstanceAction::TaskProgress(None));
                    if profile_id == self.profile_id {
                        let count = updates.len();
                        self.updates = updates;
                        actions.push(InstanceAction::UpdateCountChanged { profile_id, count });
                        let tone = if count > 0 {
                            ToastTone::Warn
                        } else {
                            ToastTone::Mr
                        };
                        actions.push(InstanceAction::Toast(
                            tone,
                            if count > 0 {
                                format!("Found {count} content update(s) via MR/CF hashes.")
                            } else {
                                "All installed content is up to date.".to_string()
                            },
                        ));
                    }
                }
                InstBgEvent::SingleUpdated {
                    profile_id,
                    kind,
                    old_filename,
                    new_filename,
                } => {
                    self.updating_items.remove(&(kind, old_filename.clone()));
                    actions.push(InstanceAction::TaskProgress(None));
                    if profile_id == self.profile_id {
                        self.updates.remove(&(kind, old_filename));
                        actions.push(InstanceAction::UpdateCountChanged {
                            profile_id,
                            count: self.updates.len(),
                        });
                    }
                    actions.push(InstanceAction::Toast(
                        ToastTone::Mr,
                        format!("Updated to {new_filename}"),
                    ));
                }
                InstBgEvent::AllUpdated { profile_id, count } => {
                    self.updating_items.clear();
                    actions.push(InstanceAction::TaskProgress(None));
                    if profile_id == self.profile_id {
                        self.updates.clear();
                        actions.push(InstanceAction::UpdateCountChanged {
                            profile_id,
                            count: 0,
                        });
                    }
                    actions.push(InstanceAction::Toast(
                        ToastTone::Mr,
                        format!("Updated {count} content item(s)."),
                    ));
                }
                InstBgEvent::Error(e) => {
                    self.checking_updates = false;
                    self.updating_items.clear();
                    actions.push(InstanceAction::TaskProgress(None));
                    actions.push(InstanceAction::Toast(ToastTone::Danger, e));
                }
            }
        }
    }

    fn spawn_check_updates(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        paths: &AppPaths,
        settings: &LauncherSettings,
        profile: &Profile,
        actions: &mut Vec<InstanceAction>,
    ) {
        if self.checking_updates {
            return;
        }
        self.checking_updates = true;
        actions.push(InstanceAction::TaskProgress(Some((
            0.25,
            format!("Hashing & checking updates for {}...", profile.name),
        ))));
        let catalog = CatalogClient::with_github(
            settings.effective_curseforge_api_key(),
            settings.effective_github_update_token(),
            settings.github_mod_repos.clone(),
        );
        let paths = paths.clone();
        let pid = profile.id.clone();
        let mc = profile.mc_version.clone();
        let loader = profile.mod_loader.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            let kinds = [
                ContentKind::Mod,
                ContentKind::ResourcePack,
                ContentKind::Shader,
                ContentKind::DataPack,
            ];
            let mut found = HashMap::new();
            for k in kinds {
                match catalog
                    .identify_and_check_updates(&paths, &pid, k, &mc, &loader)
                    .await
                {
                    Ok(list) => {
                        for u in list {
                            found.insert((u.kind, u.filename.clone()), u);
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(InstBgEvent::Error(format!(
                            "Update check failed ({}): {e}",
                            k.label()
                        )));
                        ctx.request_repaint();
                        return;
                    }
                }
            }
            let _ = tx.send(InstBgEvent::UpdatesChecked {
                profile_id: pid,
                updates: found,
            });
            ctx.request_repaint();
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_update_single(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        paths: &AppPaths,
        settings: &LauncherSettings,
        profile: &Profile,
        update: ContentUpdateInfo,
        actions: &mut Vec<InstanceAction>,
    ) {
        let key = (update.kind, update.filename.clone());
        if self.updating_items.contains(&key) {
            return;
        }
        self.updating_items.insert(key);
        actions.push(InstanceAction::TaskProgress(Some((
            0.5,
            format!("Updating {}...", update.filename),
        ))));
        let catalog = CatalogClient::with_github(
            settings.effective_curseforge_api_key(),
            settings.effective_github_update_token(),
            settings.github_mod_repos.clone(),
        );
        let paths = paths.clone();
        let pid = profile.id.clone();
        let mc = profile.mc_version.clone();
        let loader = profile.mod_loader.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            let kind = update.kind;
            let old_filename = update.filename.clone();
            match catalog
                .update_content_item(&paths, &pid, &update, Some(&mc), Some(&loader))
                .await
            {
                Ok(new_filename) => {
                    let _ = tx.send(InstBgEvent::SingleUpdated {
                        profile_id: pid,
                        kind,
                        old_filename,
                        new_filename,
                    });
                }
                Err(e) => {
                    let _ = tx.send(InstBgEvent::Error(format!("Update failed: {e}")));
                }
            }
            ctx.request_repaint();
        });
    }

    fn spawn_update_all(
        &mut self,
        rt: &Arc<tokio::runtime::Runtime>,
        ctx: &egui::Context,
        paths: &AppPaths,
        settings: &LauncherSettings,
        profile: &Profile,
        actions: &mut Vec<InstanceAction>,
    ) {
        if self.updates.is_empty() {
            return;
        }
        let list: Vec<ContentUpdateInfo> = self.updates.values().cloned().collect();
        for u in &list {
            self.updating_items.insert((u.kind, u.filename.clone()));
        }
        actions.push(InstanceAction::TaskProgress(Some((
            0.35,
            format!("Updating {} item(s)...", list.len()),
        ))));

        let catalog = CatalogClient::with_github(
            settings.effective_curseforge_api_key(),
            settings.effective_github_update_token(),
            settings.github_mod_repos.clone(),
        );
        let paths = paths.clone();
        let pid = profile.id.clone();
        let mc = profile.mc_version.clone();
        let loader = profile.mod_loader.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        rt.spawn(async move {
            match catalog
                .update_all_content(&paths, &pid, &list, Some(&mc), Some(&loader))
                .await
            {
                Ok(updated) => {
                    let _ = tx.send(InstBgEvent::AllUpdated {
                        profile_id: pid,
                        count: updated.len(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(InstBgEvent::Error(format!("Batch update failed: {e}")));
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
        settings: &LauncherSettings,
        profiles: &ProfileStore,
        accounts: &AccountStore,
        statuses: &HashMap<String, InstanceStatus>,
        manager: &InstanceManager,
        version_cache: &mut VersionPickerCache,
        detected_javas: &[JavaInstallation],
    ) -> Vec<InstanceAction> {
        let mut actions = Vec::new();
        self.drain_events(&mut actions);

        let Some(profile) = profiles.get(&self.profile_id).cloned() else {
            theme::term_surface(ui, palette, |ui| {
                ui.label(
                    RichText::new("Instance not found.")
                        .monospace()
                        .color(palette.ink_muted),
                );
                if theme::term_button(ui, palette, "<- BACK TO INSTANCES", ButtonTone::Secondary)
                    .clicked()
                {
                    actions.push(InstanceAction::Back);
                }
            });
            return actions;
        };

        if self
            .draft
            .as_ref()
            .is_none_or(|d| d.profile_id != profile.id)
        {
            self.draft = Some(SettingsDraft::from_profile(&profile));
            self.export_dest = paths
                .root()
                .join(format!("{}.mrpack", sanitize_filename(&profile.name)))
                .display()
                .to_string();
        }

        let status = statuses.get(&profile.id);
        let is_active = matches!(
            status.map(|s| &s.state),
            Some(InstanceState::Running | InstanceState::Preparing)
        );

        // Top Bar
        ui.horizontal(|ui| {
            if theme::term_button(ui, palette, "<- BACK TO INSTANCES", ButtonTone::Secondary)
                .clicked()
            {
                actions.push(InstanceAction::Back);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::term_button(ui, palette, "CLONE", ButtonTone::Secondary).clicked() {
                    let clone_name = unique_clone_name(&profile.name, profiles);
                    let cloned = profile.clone_with_name(&clone_name);
                    actions.push(InstanceAction::ProfileCreated(cloned));
                    actions.push(InstanceAction::Toast(
                        ToastTone::Focus,
                        format!("Cloned '{}' -> '{clone_name}'", profile.name),
                    ));
                }
                if theme::term_button(ui, palette, "[↑] EXPORT .MRPACK", ButtonTone::Secondary)
                    .clicked()
                {
                    self.show_export = !self.show_export;
                }
                if theme::term_button(ui, palette, "[DIR] OPEN FOLDER", ButtonTone::Secondary)
                    .clicked()
                {
                    let dir = paths.instance_dir(&profile.id);
                    if let Err(e) = open_in_file_manager(&dir) {
                        actions.push(InstanceAction::Toast(
                            ToastTone::Danger,
                            format!("Failed to open folder: {e}"),
                        ));
                    } else {
                        actions.push(InstanceAction::Toast(
                            ToastTone::Meta,
                            format!("Opened {}", dir.display()),
                        ));
                    }
                }
            });
        });

        // Inline Export .mrpack drawer
        if self.show_export {
            ui.add_space(6.0);
            theme::term_raised(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                theme::section_header(ui, palette, "EXPORT .MRPACK ARCHIVE", palette.mr);
                ui.add_space(6.0);
                egui::Grid::new("export_mrpack_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("DESTINATION")
                                .monospace()
                                .size(11.5)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.export_dest)
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(420.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("PACK VERSION")
                                .monospace()
                                .size(11.5)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.export_version)
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(140.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("SUMMARY")
                                .monospace()
                                .size(11.5)
                                .color(palette.ink_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.export_summary)
                                .hint_text("optional pack summary")
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(420.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if theme::term_button(ui, palette, "EXPORT NOW", ButtonTone::Modrinth).clicked()
                    {
                        let dest = PathBuf::from(self.export_dest.trim());
                        let ver = if self.export_version.trim().is_empty() {
                            "1.0.0"
                        } else {
                            self.export_version.trim()
                        };
                        let sum = if self.export_summary.trim().is_empty() {
                            None
                        } else {
                            Some(self.export_summary.trim())
                        };
                        match export_mrpack(paths, &profile, &dest, ver, sum) {
                            Ok(()) => {
                                self.show_export = false;
                                actions.push(InstanceAction::Toast(
                                    ToastTone::Mr,
                                    format!("Exported .mrpack -> {}", dest.display()),
                                ));
                            }
                            Err(e) => {
                                actions.push(InstanceAction::Toast(
                                    ToastTone::Danger,
                                    format!("Export failed: {e}"),
                                ));
                            }
                        }
                    }
                    if theme::term_button(ui, palette, "CANCEL", ButtonTone::Subtle).clicked() {
                        self.show_export = false;
                    }
                });
            });
        }

        ui.add_space(8.0);

        // Hero Summary Card
        let (kind_fg, kind_bg, kind_label) = match profile.kind {
            ProfileKind::Player => (palette.player, palette.player_bg, "PLAYER"),
            ProfileKind::Bot => (palette.bot, palette.bot_bg, "BOT"),
        };
        let acc_name = profile
            .account_id
            .as_deref()
            .and_then(|id| accounts.get(id))
            .map(|a| a.username.as_str())
            .unwrap_or("UNASSIGNED");
        let java_label = profile
            .java_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Auto");

        theme::term_raised(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            let hero_mid_w = (ui.available_width() - 48.0 - 250.0 - 24.0).max(180.0);
            ui.horizontal(|ui| {
                theme::paint_avatar_tile(ui, &profile.name, kind_fg, kind_bg, 48.0);
                ui.add_space(8.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(hero_mid_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(hero_mid_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&profile.name)
                                    .monospace()
                                    .size(16.0)
                                    .strong()
                                    .color(palette.ink),
                            );
                            theme::term_badge(ui, kind_label, kind_fg, kind_bg);
                            if let Some(grp) = &profile.group
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
                            theme::term_badge(
                                ui,
                                &format!("MC {} · {}", profile.mc_version, profile.mod_loader),
                                palette.meta,
                                palette.meta_bg,
                            );
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
                                    theme::term_badge(
                                        ui,
                                        "◐ PREPARING",
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
                                _ => {}
                            }
                        });
                        ui.add_space(2.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "ACCOUNT: {}  │  HEAP: {}..{} MB  │  PLAYTIME: {}  │  JAVA: {}",
                                    acc_name,
                                    profile.memory_min_mb,
                                    profile.memory_max_mb,
                                    format_playtime(profile.playtime_secs),
                                    java_label
                                ))
                                .monospace()
                                .size(11.5)
                                .color(palette.ink_muted),
                            )
                            .truncate(),
                        );
                    },
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_active {
                        if theme::term_button(ui, palette, "[■] STOP", ButtonTone::Danger).clicked()
                        {
                            actions.push(InstanceAction::Stop(profile.id.clone()));
                        }
                    } else if theme::term_button(ui, palette, "[▶] LAUNCH", ButtonTone::Cta)
                        .clicked()
                    {
                        actions.push(InstanceAction::Launch(profile.clone()));
                    }
                    if theme::term_button(ui, palette, "[+] ADD CONTENT", ButtonTone::Focus)
                        .clicked()
                    {
                        actions.push(InstanceAction::AddContent(profile.id.clone()));
                    }
                });
            });
        });

        ui.add_space(10.0);

        // Gather all installed content across the 4 content dirs
        let mut all_content = Vec::new();
        for kind in [
            ContentKind::Mod,
            ContentKind::ResourcePack,
            ContentKind::Shader,
            ContentKind::DataPack,
        ] {
            if let Ok(items) = list_installed_content(paths, &profile.id, kind) {
                all_content.extend(items);
            }
        }

        // Sub-tabs row + right-side Content actions
        ui.horizontal(|ui| {
            let content_lbl = format!("CONTENT ({})", all_content.len());
            if theme::term_tab(
                ui,
                palette,
                &content_lbl,
                self.sub_tab == InstanceSubTab::Content,
            )
            .clicked()
            {
                self.sub_tab = InstanceSubTab::Content;
            }
            if theme::term_tab(
                ui,
                palette,
                "WORLDS & FILES",
                self.sub_tab == InstanceSubTab::WorldsFiles,
            )
            .clicked()
            {
                self.sub_tab = InstanceSubTab::WorldsFiles;
            }
            if theme::term_tab(ui, palette, "LOGS", self.sub_tab == InstanceSubTab::Logs).clicked()
            {
                self.sub_tab = InstanceSubTab::Logs;
            }
            if theme::term_tab(
                ui,
                palette,
                "SETTINGS",
                self.sub_tab == InstanceSubTab::Settings,
            )
            .clicked()
            {
                self.sub_tab = InstanceSubTab::Settings;
            }

            if self.sub_tab == InstanceSubTab::Content {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::term_button(ui, palette, "[⇄] SYNC TO BOTS", ButtonTone::Bot)
                        .clicked()
                    {
                        let bot_ids: Vec<String> = profiles
                            .profiles
                            .iter()
                            .filter(|p| p.kind == ProfileKind::Bot && p.id != profile.id)
                            .map(|p| p.id.clone())
                            .collect();
                        if bot_ids.is_empty() {
                            actions.push(InstanceAction::Toast(
                                ToastTone::Warn,
                                "No other Bot profiles to sync to.".to_string(),
                            ));
                        } else {
                            let mut total_copied = 0usize;
                            for k in [
                                ContentKind::Mod,
                                ContentKind::ResourcePack,
                                ContentKind::Shader,
                                ContentKind::DataPack,
                            ] {
                                if let Ok(n) =
                                    sync_content_to_profiles(paths, &profile.id, &bot_ids, k)
                                {
                                    total_copied += n;
                                }
                            }
                            actions.push(InstanceAction::Toast(
                                ToastTone::Bot,
                                format!(
                                    "Synced {total_copied} file(s) to {} bot profile(s).",
                                    bot_ids.len()
                                ),
                            ));
                        }
                    }

                    let pending = self.updates.len();
                    if pending > 0
                        && theme::term_button(
                            ui,
                            palette,
                            &format!("[↑] UPDATE ALL ({pending})"),
                            ButtonTone::Warn,
                        )
                        .clicked()
                    {
                        self.spawn_update_all(
                            rt,
                            ctx,
                            paths,
                            settings,
                            &profile,
                            &mut actions,
                        );
                    }

                    let chk_lbl = if self.checking_updates {
                        "[↻] CHECKING..."
                    } else {
                        "[↻] CHECK UPDATES"
                    };
                    if ui
                        .add_enabled_ui(!self.checking_updates, |ui| {
                            theme::term_button(ui, palette, chk_lbl, ButtonTone::Secondary)
                        })
                        .inner
                        .clicked()
                    {
                        self.spawn_check_updates(rt, ctx, paths, settings, &profile, &mut actions);
                    }
                });
            }
        });
        theme::term_divider(ui, palette);
        ui.add_space(4.0);

        match self.sub_tab {
            InstanceSubTab::Content => {
                self.ui_content_tab(
                    ui,
                    ctx,
                    palette,
                    rt,
                    paths,
                    settings,
                    &profile,
                    &all_content,
                    &mut actions,
                );
            }
            InstanceSubTab::WorldsFiles => {
                self.ui_worlds_tab(ui, palette, paths, &profile, &mut actions);
            }
            InstanceSubTab::Logs => {
                self.ui_logs_tab(
                    ui,
                    palette,
                    rt,
                    &profile,
                    status,
                    manager,
                    &mut actions,
                );
            }
            InstanceSubTab::Settings => {
                self.ui_settings_tab(
                    ui,
                    ctx,
                    palette,
                    rt,
                    &profile,
                    accounts,
                    is_active,
                    version_cache,
                    detected_javas,
                    &mut actions,
                );
            }
        }

        actions
    }

    #[allow(clippy::too_many_arguments)]
    fn ui_content_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        paths: &AppPaths,
        settings: &LauncherSettings,
        profile: &Profile,
        all_content: &[mub_core::InstalledContentItem],
        actions: &mut Vec<InstanceAction>,
    ) {
        // Kind filter pills
        ui.horizontal_wrapped(|ui| {
            let filters = [
                (ContentFilterKind::All, "ALL"),
                (ContentFilterKind::Mod, "MODS"),
                (ContentFilterKind::ResourcePack, "RESOURCE PACKS"),
                (ContentFilterKind::Shader, "SHADERS"),
                (ContentFilterKind::DataPack, "DATA PACKS"),
            ];
            for (fk, lbl) in filters {
                let active = self.content_filter == fk;
                let tone = if active {
                    ButtonTone::Focus
                } else {
                    ButtonTone::Subtle
                };
                if theme::term_button(ui, palette, lbl, tone).clicked() {
                    self.content_filter = fk;
                }
            }
        });
        ui.add_space(6.0);

        let filtered: Vec<&mub_core::InstalledContentItem> = all_content
            .iter()
            .filter(|item| match self.content_filter {
                ContentFilterKind::All => true,
                ContentFilterKind::Mod => item.kind == ContentKind::Mod,
                ContentFilterKind::ResourcePack => item.kind == ContentKind::ResourcePack,
                ContentFilterKind::Shader => item.kind == ContentKind::Shader,
                ContentFilterKind::DataPack => item.kind == ContentKind::DataPack,
            })
            .collect();

        if filtered.is_empty() {
            theme::term_surface(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("No content installed in this view.")
                            .monospace()
                            .color(palette.ink_muted),
                    );
                    ui.add_space(6.0);
                    if theme::term_button(ui, palette, "[+] BROWSE CATALOG", ButtonTone::Focus)
                        .clicked()
                    {
                        actions.push(InstanceAction::AddContent(profile.id.clone()));
                    }
                    ui.add_space(18.0);
                });
            });
            return;
        }

        let mut update_to_run: Option<ContentUpdateInfo> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in filtered {
                let update_info = self
                    .updates
                    .get(&(item.kind, item.filename.clone()))
                    .cloned();
                let is_updating = self
                    .updating_items
                    .contains(&(item.kind, item.filename.clone()));

                theme::term_row(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    let content_left_w = (ui.available_width() - 320.0).max(160.0);
                    ui.horizontal(|ui| {
                        // Left: status square + title + filename
                        let dot_col = if item.enabled {
                            palette.focus
                        } else {
                            palette.ink_muted
                        };
                        let (dot_rect, _) = ui.allocate_exact_size(
                            egui::vec2(8.0, 8.0),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_filled(dot_rect, egui::CornerRadius::ZERO, dot_col);

                        ui.allocate_ui_with_layout(
                            egui::vec2(content_left_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(content_left_w);
                                ui.horizontal_wrapped(|ui| {
                                    let title = item
                                        .meta
                                        .as_ref()
                                        .and_then(|m| m.title.as_deref())
                                        .unwrap_or(&item.filename);
                                    let mut rt_title = RichText::new(title)
                                        .monospace()
                                        .size(12.5)
                                        .strong()
                                        .color(if item.enabled {
                                            palette.ink
                                        } else {
                                            palette.ink_muted
                                        });
                                    if !item.enabled {
                                        rt_title = rt_title.strikethrough();
                                    }
                                    ui.label(rt_title);

                                    // Source badge
                                    let src = item
                                        .meta
                                        .as_ref()
                                        .map(|m| m.source)
                                        .or_else(|| update_info.as_ref().map(|u| u.source));
                                    match src {
                                        Some(ContentSource::Modrinth) => {
                                            theme::term_badge(ui, "MR", palette.mr, palette.mr_bg);
                                        }
                                        Some(ContentSource::CurseForge) => {
                                            theme::term_badge(ui, "CF", palette.cf, palette.cf_bg);
                                        }
                                        Some(ContentSource::GitHub) => {
                                            theme::term_badge(
                                                ui,
                                                "GH",
                                                palette.meta,
                                                palette.meta_bg,
                                            );
                                        }
                                        None => {
                                            theme::term_badge_bordered(
                                                ui,
                                                "LOCAL",
                                                palette.ink_muted,
                                                Color32::TRANSPARENT,
                                                palette.border_hi,
                                            );
                                        }
                                    }

                                    let kind_tag = match item.kind {
                                        ContentKind::Mod => "MOD",
                                        ContentKind::ResourcePack => "RESOURCE",
                                        ContentKind::Shader => "SHADER",
                                        ContentKind::DataPack => "DATAPACK",
                                        ContentKind::Modpack => "PACK",
                                    };
                                    ui.label(
                                        RichText::new(kind_tag)
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.meta),
                                    );
                                });

                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!(
                                            "{} · {}",
                                            item.filename,
                                            format_bytes(item.size_bytes)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.ink_muted),
                                    )
                                    .truncate(),
                                );
                            },
                        );

                        // Right: Version + Actions
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::term_button(ui, palette, "DEL", ButtonTone::Danger).clicked()
                            {
                                match remove_installed_content(
                                    paths,
                                    &profile.id,
                                    item.kind,
                                    &item.filename,
                                ) {
                                    Ok(()) => {
                                        self.updates.remove(&(item.kind, item.filename.clone()));
                                        actions.push(InstanceAction::Toast(
                                            ToastTone::Danger,
                                            format!("Removed {}", item.filename),
                                        ));
                                    }
                                    Err(e) => {
                                        actions.push(InstanceAction::Toast(
                                            ToastTone::Danger,
                                            format!("Remove failed: {e}"),
                                        ));
                                    }
                                }
                            }

                            let toggle_lbl = if item.enabled { "ON" } else { "OFF" };
                            let toggle_tone = if item.enabled {
                                ButtonTone::Focus
                            } else {
                                ButtonTone::Subtle
                            };
                            if theme::term_button(ui, palette, toggle_lbl, toggle_tone).clicked() {
                                if let Err(e) = toggle_installed_content(
                                    paths,
                                    &profile.id,
                                    item.kind,
                                    &item.filename,
                                ) {
                                    actions.push(InstanceAction::Toast(
                                        ToastTone::Danger,
                                        format!("Toggle failed: {e}"),
                                    ));
                                } else {
                                    actions.push(InstanceAction::Toast(
                                        ToastTone::Focus,
                                        format!(
                                            "{} {}",
                                            if item.enabled { "Disabled" } else { "Enabled" },
                                            item.filename
                                        ),
                                    ));
                                }
                            }

                            if let Some(u) = &update_info {
                                let lbl = if is_updating { "..." } else { "UPDATE" };
                                if ui
                                    .add_enabled_ui(!is_updating, |ui| {
                                        theme::term_button(ui, palette, lbl, ButtonTone::Warn)
                                    })
                                    .inner
                                    .clicked()
                                {
                                    update_to_run = Some(u.clone());
                                }
                            }

                            // Version display
                            ui.vertical(|ui| {
                                let cur_ver = update_info
                                    .as_ref()
                                    .map(|u| u.current_version.as_str())
                                    .or_else(|| {
                                        item.meta.as_ref().map(|m| m.version_number.as_str())
                                    })
                                    .unwrap_or("local");
                                ui.label(
                                    RichText::new(cur_ver)
                                        .monospace()
                                        .size(11.5)
                                        .color(palette.ink_dim),
                                );
                                if let Some(u) = &update_info {
                                    ui.label(
                                        RichText::new(format!("→ {}", u.latest_version.version_number))
                                            .monospace()
                                            .size(11.0)
                                            .strong()
                                            .color(palette.warn),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("CURRENT")
                                            .monospace()
                                            .size(10.5)
                                            .color(palette.ink_muted),
                                    );
                                }
                            });
                        });
                    });
                });
                ui.add_space(4.0);
            }
        });

        if let Some(u) = update_to_run {
            self.spawn_update_single(rt, ctx, paths, settings, profile, u, actions);
        }
    }

    fn ui_worlds_tab(
        &self,
        ui: &mut egui::Ui,
        palette: &Palette,
        paths: &AppPaths,
        profile: &Profile,
        actions: &mut Vec<InstanceAction>,
    ) {
        theme::term_surface(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            theme::section_header(ui, palette, "QUICK DIRECTORIES", palette.meta);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let dirs = [
                    ("mods/", paths.mods_dir(&profile.id)),
                    ("config/", paths.config_dir(&profile.id)),
                    ("resourcepacks/", paths.resourcepacks_dir(&profile.id)),
                    ("shaderpacks/", paths.shaderpacks_dir(&profile.id)),
                    ("saves/", paths.saves_dir(&profile.id)),
                    ("screenshots/", paths.screenshots_dir(&profile.id)),
                    ("logs/", paths.logs_dir(&profile.id)),
                ];
                for (label, dir) in dirs {
                    if theme::term_button(ui, palette, &format!("[DIR] {label}"), ButtonTone::Secondary)
                        .clicked()
                    {
                        match open_in_file_manager(&dir) {
                            Ok(()) => actions.push(InstanceAction::Toast(
                                ToastTone::Meta,
                                format!("Opened {}", dir.display()),
                            )),
                            Err(e) => actions.push(InstanceAction::Toast(
                                ToastTone::Danger,
                                format!("Open folder failed: {e}"),
                            )),
                        }
                    }
                }
            });

            ui.add_space(12.0);
            theme::section_header(ui, palette, "SAVED WORLDS", palette.meta);
            ui.add_space(6.0);

            let worlds = list_saved_worlds(paths, &profile.id).unwrap_or_default();
            if worlds.is_empty() {
                ui.label(
                    RichText::new("No saved worlds found in saves/.")
                        .monospace()
                        .size(12.0)
                        .color(palette.ink_muted),
                );
            } else {
                for w in worlds {
                    theme::term_row(ui, palette, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&w.folder_name)
                                    .monospace()
                                    .strong()
                                    .size(12.5)
                                    .color(palette.ink),
                            );
                            ui.label(
                                RichText::new(format!("· {} datapack(s)", w.datapack_count))
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.ink_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::term_button(
                                        ui,
                                        palette,
                                        "DATAPACKS",
                                        ButtonTone::Secondary,
                                    )
                                    .clicked()
                                    {
                                        if let Err(e) = open_in_file_manager(&w.datapacks_dir) {
                                            actions.push(InstanceAction::Toast(
                                                ToastTone::Danger,
                                                format!("Failed: {e}"),
                                            ));
                                        } else {
                                            actions.push(InstanceAction::Toast(
                                                ToastTone::Meta,
                                                format!("Opened {}", w.datapacks_dir.display()),
                                            ));
                                        }
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

    #[allow(clippy::too_many_arguments)]
    fn ui_logs_tab(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        profile: &Profile,
        status: Option<&InstanceStatus>,
        manager: &InstanceManager,
        actions: &mut Vec<InstanceAction>,
    ) {
        ui.horizontal(|ui| {
            for (lvl, lbl, tone) in [
                (LogLevelFilter::All, "ALL", ButtonTone::Focus),
                (LogLevelFilter::Info, "INFO", ButtonTone::Player),
                (LogLevelFilter::Warn, "WARN", ButtonTone::Warn),
                (LogLevelFilter::Err, "ERR", ButtonTone::Danger),
            ] {
                let active = self.log_filter == lvl;
                if theme::term_button(
                    ui,
                    palette,
                    lbl,
                    if active { tone } else { ButtonTone::Subtle },
                )
                .clicked()
                {
                    self.log_filter = lvl;
                }
            }

            theme::term_search_bar(
                ui,
                palette,
                &mut self.log_search,
                "/",
                "filter logs...",
                190.0,
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::term_button(ui, palette, "CLEAR", ButtonTone::Subtle).clicked() {
                    let mgr = manager.clone();
                    let pid = profile.id.clone();
                    rt.spawn(async move {
                        mgr.clear_logs(&pid).await;
                    });
                }
                if let Some(st) = status
                    && theme::term_button(ui, palette, "COPY", ButtonTone::Secondary).clicked()
                {
                    let joined = st.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                    ui.ctx().copy_text(joined);
                    actions.push(InstanceAction::Toast(
                        ToastTone::Focus,
                        format!("Copied {} log lines", st.logs.len()),
                    ));
                }
            });
        });

        ui.add_space(6.0);

        let is_running = status
            .map(|s| matches!(s.state, InstanceState::Running))
            .unwrap_or(false);
        let q = self.log_search.trim().to_lowercase();

        theme::term_box(
            ui,
            palette.bg,
            palette.border_hi,
            Margin::same(0),
            |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(320.0);

                // Header bar
                theme::term_box(
                    ui,
                    palette.panel,
                    palette.border,
                    Margin::symmetric(12, 6),
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("● {} — stdout/stderr", profile.name))
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
                                    let count = status.map(|s| s.logs.len()).unwrap_or(0);
                                    ui.label(
                                        RichText::new(format!("{count} / 2000 LINES"))
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
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_min_height(280.0);
                        if let Some(st) = status {
                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for line in &st.logs {
                                        if !matches_log_filter(line, self.log_filter, &q) {
                                            continue;
                                        }
                                        let col = log_line_color(line, palette);
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(line)
                                                    .monospace()
                                                    .size(11.5)
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
                                    "No process output yet. Launch this instance to stream logs.",
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

    #[allow(clippy::too_many_arguments)]
    fn ui_settings_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &Palette,
        rt: &Arc<tokio::runtime::Runtime>,
        profile: &Profile,
        accounts: &AccountStore,
        is_active: bool,
        version_cache: &mut VersionPickerCache,
        detected_javas: &[JavaInstallation],
        actions: &mut Vec<InstanceAction>,
    ) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };

        version_cache.ensure_mc_loaded(rt, ctx);
        version_cache.ensure_loader_loaded(rt, ctx, &draft.mc_version, draft.loader_choice);

        theme::term_surface(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.columns(2, |cols| {
                // Left column: Identity & Runtime
                cols[0].vertical(|ui| {
                    theme::section_header(ui, palette, "IDENTITY & RUNTIME", palette.focus);
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("PROFILE NAME")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .font(egui::FontId::monospace(12.0))
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(6.0);

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
                                    if draft.kind == ProfileKind::Player {
                                        ButtonTone::Player
                                    } else {
                                        ButtonTone::Subtle
                                    },
                                )
                                .clicked()
                                {
                                    draft.kind = ProfileKind::Player;
                                }
                                if theme::term_button(
                                    ui,
                                    palette,
                                    "BOT",
                                    if draft.kind == ProfileKind::Bot {
                                        ButtonTone::Bot
                                    } else {
                                        ButtonTone::Subtle
                                    },
                                )
                                .clicked()
                                {
                                    draft.kind = ProfileKind::Bot;
                                }
                            });
                        });
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("GROUP TAG")
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.ink_muted),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.group)
                                    .hint_text("e.g. BOT-SWARM")
                                    .font(egui::FontId::monospace(12.0))
                                    .desired_width(140.0),
                            );
                        });
                    });
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("MINECRAFT VERSION (LIVE MOJANG)")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    if version_cache.render_mc_picker(
                        ui,
                        palette,
                        rt,
                        "inst_set_mc",
                        &mut draft.mc_version,
                        &mut draft.show_snapshots,
                        &mut draft.mc_search,
                    ) {
                        draft.loader_version.clear();
                        version_cache.ensure_loader_loaded(
                            rt,
                            ctx,
                            &draft.mc_version,
                            draft.loader_choice,
                        );
                    }
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("MOD LOADER (LIVE VERSIONS)")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    version_cache.render_loader_picker(
                        ui,
                        palette,
                        rt,
                        "inst_set_loader",
                        &draft.mc_version,
                        &mut draft.loader_choice,
                        &mut draft.loader_version,
                        true,
                    );
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("ASSIGNED ACCOUNT")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    let acc_label = draft
                        .account_id
                        .as_deref()
                        .and_then(|id| accounts.get(id))
                        .map(|a| format!("{} ({})", a.username, a.role))
                        .unwrap_or_else(|| "<None>".to_string());
                    egui::ComboBox::from_id_salt("inst_set_acc")
                        .selected_text(acc_label)
                        .width(ui.available_width().min(280.0))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.account_id, None, "<None>");
                            for a in &accounts.accounts {
                                ui.selectable_value(
                                    &mut draft.account_id,
                                    Some(a.id.clone()),
                                    format!("{} ({})", a.username, a.role),
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
                        egui::TextEdit::singleline(&mut draft.quick_connect)
                            .hint_text("host:port")
                            .font(egui::FontId::monospace(12.0))
                            .desired_width(ui.available_width()),
                    );
                });

                // Right column: JVM & Window
                cols[1].vertical(|ui| {
                    theme::section_header(ui, palette, "JVM & WINDOW", palette.meta);
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("MEMORY (MIN / MAX MB)")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut draft.memory_min_mb)
                                .range(128..=16384)
                                .speed(64),
                        );
                        ui.label(RichText::new("..").monospace().color(palette.ink_muted));
                        ui.add(
                            egui::DragValue::new(&mut draft.memory_max_mb)
                                .range(256..=32768)
                                .speed(128),
                        );
                    });
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("JAVA OVERRIDE")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    let java_combo_label = if draft.java_override.trim().is_empty() {
                        "Auto (Resolve per MC version)".to_string()
                    } else {
                        draft.java_override.clone()
                    };
                    egui::ComboBox::from_id_salt("inst_set_java")
                        .selected_text(java_combo_label)
                        .width(ui.available_width().min(340.0))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut draft.java_override,
                                String::new(),
                                "Auto (Resolve per MC version)",
                            );
                            for j in detected_javas {
                                let p_str = j.path.to_string_lossy().into_owned();
                                ui.selectable_value(
                                    &mut draft.java_override,
                                    p_str.clone(),
                                    format!("Java {} ({} {})", j.major, j.vendor, j.version),
                                );
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.java_override)
                            .hint_text("Leave empty for Auto, or paste custom java path")
                            .font(egui::FontId::monospace(11.5))
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("EXTRA JVM FLAGS")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.jvm_args_text)
                            .hint_text("-XX:+UseZGC -XX:+ZGenerational")
                            .font(egui::FontId::monospace(11.5))
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("WINDOW RESOLUTION (W × H)")
                            .monospace()
                            .size(11.0)
                            .color(palette.ink_muted),
                    );
                    ui.horizontal(|ui| {
                        theme::term_toggle(
                            ui,
                            palette,
                            &mut draft.use_resolution,
                            "CUSTOM",
                            palette.focus,
                        );
                        if draft.use_resolution {
                            ui.add(
                                egui::DragValue::new(&mut draft.res_width)
                                    .range(320..=7680)
                                    .speed(10),
                            );
                            ui.label(RichText::new("×").monospace().color(palette.ink_muted));
                            ui.add(
                                egui::DragValue::new(&mut draft.res_height)
                                    .range(240..=4320)
                                    .speed(10),
                            );
                        }
                    });
                });
            });

            ui.add_space(12.0);
            theme::term_divider(ui, palette);

            let loader_valid = draft.loader_choice == LoaderChoice::Vanilla
                || !draft.loader_version.trim().is_empty();
            let can_save = !draft.name.trim().is_empty() && loader_valid;

            ui.horizontal(|ui| {
                if self.confirm_delete {
                    ui.label(
                        RichText::new("Confirm delete?")
                            .monospace()
                            .strong()
                            .color(palette.danger),
                    );
                    theme::term_toggle(
                        ui,
                        palette,
                        &mut self.delete_disk_files,
                        "Also delete instance folder on disk",
                        palette.danger,
                    );
                    if theme::term_button(ui, palette, "CANCEL", ButtonTone::Secondary).clicked() {
                        self.confirm_delete = false;
                    }
                    if theme::term_button(ui, palette, "CONFIRM DELETE", ButtonTone::Danger)
                        .clicked()
                    {
                        self.confirm_delete = false;
                        actions.push(InstanceAction::ProfileDeleted {
                            id: profile.id.clone(),
                            delete_files: self.delete_disk_files,
                        });
                    }
                } else {
                    if ui
                        .add_enabled_ui(!is_active, |ui| {
                            theme::term_button(ui, palette, "DELETE INSTANCE", ButtonTone::Danger)
                        })
                        .inner
                        .clicked()
                    {
                        self.confirm_delete = true;
                    }
                    if is_active {
                        ui.label(
                            RichText::new("Stop instance before deleting.")
                                .monospace()
                                .size(11.0)
                                .color(palette.ink_muted),
                        );
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled_ui(can_save, |ui| {
                            theme::term_button(ui, palette, "SAVE CHANGES", ButtonTone::Cta)
                        })
                        .inner
                        .clicked()
                    {
                        let mut updated = profile.clone();
                        updated.name = draft.name.trim().to_string();
                        updated.kind = draft.kind;
                        updated.group = {
                            let g = draft.group.trim();
                            if g.is_empty() {
                                None
                            } else {
                                Some(g.to_string())
                            }
                        };
                        updated.mc_version = draft.mc_version.trim().to_string();
                        updated.mod_loader = draft.loader_choice.to_mod_loader(&draft.loader_version);
                        updated.account_id = draft.account_id.clone();
                        updated.memory_min_mb = draft.memory_min_mb.min(draft.memory_max_mb);
                        updated.memory_max_mb = draft.memory_max_mb.max(draft.memory_min_mb);
                        updated.java_path = {
                            let j = draft.java_override.trim();
                            if j.is_empty() {
                                None
                            } else {
                                Some(j.to_string())
                            }
                        };
                        updated.jvm_args = draft
                            .jvm_args_text
                            .split_whitespace()
                            .map(ToString::to_string)
                            .collect();
                        updated.resolution = if draft.use_resolution {
                            Some((draft.res_width, draft.res_height))
                        } else {
                            None
                        };
                        updated.quick_connect = {
                            let qc = draft.quick_connect.trim();
                            if qc.is_empty() {
                                None
                            } else {
                                Some(qc.to_string())
                            }
                        };
                        actions.push(InstanceAction::ProfileUpdated(updated));
                        actions.push(InstanceAction::Toast(
                            ToastTone::Focus,
                            format!("Saved settings for '{}'", draft.name.trim()),
                        ));
                    }
                    if !loader_valid {
                        ui.label(
                            RichText::new("Select a loader version before saving.")
                                .monospace()
                                .size(11.0)
                                .color(palette.warn),
                        );
                    }
                });
            });
        });
    }
}

pub fn format_playtime(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    let hours = mins / 60;
    let rem_m = mins % 60;
    if hours > 0 {
        format!("{hours}h {rem_m:02}m")
    } else {
        format!("{rem_m}m")
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "modpack".to_string()
    } else {
        s
    }
}

fn unique_clone_name(base: &str, profiles: &ProfileStore) -> String {
    let first = format!("{base}-Copy");
    if profiles.find_by_name(&first).is_none() {
        return first;
    }
    for idx in 2..=99 {
        let cand = format!("{base}-Copy-{idx}");
        if profiles.find_by_name(&cand).is_none() {
            return cand;
        }
    }
    format!("{base}-Copy-{}", profiles.profiles.len() + 1)
}

pub fn matches_log_filter(line: &str, filter: LogLevelFilter, query_lower: &str) -> bool {
    if !query_lower.is_empty() && !line.to_lowercase().contains(query_lower) {
        return false;
    }
    match filter {
        LogLevelFilter::All => true,
        LogLevelFilter::Err => {
            line.contains("ERROR") || line.contains("Exception") || line.contains("FATAL")
        }
        LogLevelFilter::Warn => line.contains("WARN"),
        LogLevelFilter::Info => {
            line.contains("INFO") || line.starts_with("[MUB]")
        }
    }
}

pub fn log_line_color(line: &str, palette: &Palette) -> Color32 {
    if line.contains("ERROR") || line.contains("Exception") || line.contains("FATAL") {
        palette.danger
    } else if line.contains("WARN") {
        palette.warn
    } else if line.starts_with("[MUB]") {
        palette.focus
    } else {
        palette.ink_dim
    }
}

pub fn render_blinking_cursor(ui: &mut egui::Ui, palette: &Palette) {
    let time = ui.input(|i| i.time);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(550));
    let on = ((time * 1.8) as u64).is_multiple_of(2);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(">")
                .monospace()
                .size(12.0)
                .strong()
                .color(palette.focus),
        );
        if on {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(7.0, 13.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, palette.focus);
        }
    });
}
