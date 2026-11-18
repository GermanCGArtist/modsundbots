mod mods_view;
mod theme;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, Color32, Margin, RichText};
use mods_view::ModsViewState;
use mub_core::{
    Account, AccountRole, AccountStore, AppPaths, DeviceCodeResponse, InstanceManager,
    InstanceState, InstanceStatus, LauncherSettings, ModLoader, MsaAuthClient, Profile,
    ProfileKind, ProfileStore,
};
use theme::{ButtonTone, GlassElevation, GlassMode, NavIcon, Palette};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Instances,
    Mods,
    Accounts,
    Logs,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileFilter {
    All,
    Players,
    Bots,
}

enum BgEvent {
    DeviceCodeReady(DeviceCodeResponse, AccountRole),
    AccountAdded(Account),
    AccountRefreshed(Account),
    StatusSnapshot(Vec<InstanceStatus>),
    Error(String),
}

struct MubApp {
    rt: Arc<tokio::runtime::Runtime>,
    paths: AppPaths,
    settings: LauncherSettings,
    accounts: AccountStore,
    profiles: ProfileStore,
    manager: InstanceManager,

    glass_mode: GlassMode,
    active_tab: Tab,
    profile_filter: ProfileFilter,
    search_query: String,
    selected_profiles: HashSet<String>,
    selected_log_profile: Option<String>,
    statuses: HashMap<String, InstanceStatus>,

    tx: mpsc::UnboundedSender<BgEvent>,
    rx: mpsc::UnboundedReceiver<BgEvent>,
    banner_msg: Option<(bool, String)>,

    // New profile form
    show_new_profile: bool,
    new_prof_name: String,
    new_prof_kind: ProfileKind,
    new_prof_mc_version: String,
    new_prof_fabric_ver: String,
    new_prof_use_fabric: bool,
    new_prof_account_id: Option<String>,
    new_prof_server: String,
    new_prof_ram_max: u32,

    // Account form
    new_offline_name: String,
    new_offline_role: AccountRole,
    pending_device_code: Option<(DeviceCodeResponse, AccountRole)>,
    pending_live_auth: Option<AccountRole>,
    live_auth_code_input: String,

    // Modrinth store
    mods_view: ModsViewState,
}

impl MubApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let glass_mode = GlassMode::Dark;
        theme::apply_theme(&cc.egui_ctx, glass_mode);
        let rt = Arc::new(
            tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime"),
        );
        let paths = AppPaths::default_root().expect("Failed to resolve app data dir");
        let _ = paths.ensure_dirs();
        let settings = LauncherSettings::load(&paths.settings_file()).unwrap_or_default();
        let accounts = AccountStore::load(&paths.accounts_file()).unwrap_or_default();
        let profiles = ProfileStore::load(&paths.profiles_file()).unwrap_or_default();
        let first_profile_id = profiles.profiles.first().map(|p| p.id.clone());
        let manager = InstanceManager::new();
        let (tx, rx) = mpsc::unbounded_channel();

        let mgr_clone = manager.clone();
        let tx_clone = tx.clone();
        let ctx_clone = cc.egui_ctx.clone();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                let list = mgr_clone.list_statuses().await;
                if tx_clone.send(BgEvent::StatusSnapshot(list)).is_err() {
                    break;
                }
                ctx_clone.request_repaint();
            }
        });

        Self {
            rt,
            paths,
            settings,
            accounts,
            profiles,
            manager,
            glass_mode,
            active_tab: Tab::Instances,
            profile_filter: ProfileFilter::All,
            search_query: String::new(),
            selected_profiles: HashSet::new(),
            selected_log_profile: None,
            statuses: HashMap::new(),
            tx,
            rx,
            banner_msg: None,
            show_new_profile: false,
            new_prof_name: String::new(),
            new_prof_kind: ProfileKind::Bot,
            new_prof_mc_version: "1.21.4".to_string(),
            new_prof_fabric_ver: String::new(),
            new_prof_use_fabric: false,
            new_prof_account_id: None,
            new_prof_server: String::new(),
            new_prof_ram_max: 1024,
            new_offline_name: String::new(),
            new_offline_role: AccountRole::Bot,
            pending_device_code: None,
            pending_live_auth: None,
            live_auth_code_input: String::new(),
            mods_view: ModsViewState::new(first_profile_id),
        }
    }

    fn set_glass_mode(&mut self, ctx: &egui::Context, mode: GlassMode) {
        if self.glass_mode != mode {
            self.glass_mode = mode;
            theme::apply_theme(ctx, mode);
        }
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                BgEvent::DeviceCodeReady(dev, role) => {
                    self.pending_device_code = Some((dev, role));
                }
                BgEvent::AccountAdded(acc) => {
                    self.pending_device_code = None;
                    self.banner_msg = Some((false, format!("Signed in as {}", acc.username)));
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                }
                BgEvent::AccountRefreshed(acc) => {
                    self.banner_msg =
                        Some((false, format!("Refreshed token for {}", acc.username)));
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                }
                BgEvent::StatusSnapshot(list) => {
                    self.statuses.clear();
                    for s in list {
                        self.statuses.insert(s.profile_id.clone(), s);
                    }
                }
                BgEvent::Error(e) => {
                    self.banner_msg = Some((true, e));
                }
            }
        }
        self.mods_view
            .drain_events(ctx, &self.rt, &mut self.banner_msg);
    }

    fn launch_profiles(&self, profiles: Vec<Profile>) {
        let mut items = Vec::new();
        for p in profiles {
            let Some(acc_id) = &p.account_id else {
                let _ = self.tx.send(BgEvent::Error(format!(
                    "Profile '{}' has no assigned account",
                    p.name
                )));
                continue;
            };
            let Some(acc) = self.accounts.get(acc_id).cloned() else {
                let _ = self.tx.send(BgEvent::Error(format!(
                    "Assigned account for '{}' not found",
                    p.name
                )));
                continue;
            };
            items.push((p, acc));
        }

        if items.is_empty() {
            return;
        }

        let mgr = self.manager.clone();
        let paths = self.paths.clone();
        let settings = self.settings.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let client = MsaAuthClient::new(&settings.msa_client_id);
            let mut prepared_items = Vec::with_capacity(items.len());
            for (p, mut acc) in items {
                if acc.is_msa() && !acc.is_token_valid() {
                    match client.refresh_account(&acc).await {
                        Ok(refreshed) => {
                            acc = refreshed.clone();
                            let _ = tx.send(BgEvent::AccountRefreshed(refreshed));
                        }
                        Err(e) => {
                            let _ = tx.send(BgEvent::Error(format!(
                                "Token refresh failed for {}: {e}",
                                acc.username
                            )));
                            continue;
                        }
                    }
                }
                prepared_items.push((p, acc));
            }
            let results = mgr.launch_batch(&paths, &settings, prepared_items).await;
            for (id, res) in results {
                if let Err(e) = res {
                    let _ = tx.send(BgEvent::Error(format!("Launch failed ({id}): {e}")));
                }
            }
        });
    }

    fn stop_profile(&self, profile_id: String) {
        let mgr = self.manager.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            if let Err(e) = mgr.stop_profile(&profile_id).await {
                let _ = tx.send(BgEvent::Error(format!("Failed to stop {profile_id}: {e}")));
            }
        });
    }

    fn stop_all(&self) {
        let mgr = self.manager.clone();
        self.rt.spawn(async move {
            let _ = mgr.stop_all().await;
        });
    }

    fn start_msa_login(&mut self, role: AccountRole) {
        let client_id = self.settings.msa_client_id.clone();
        let client = MsaAuthClient::new(client_id.clone());
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
                            let _ = tx.send(BgEvent::Error(format!("MSA sign-in failed: {e}")));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgEvent::Error(format!("Device code request failed: {e}")));
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
                    let _ = tx.send(BgEvent::Error(format!("Microsoft sign-in failed: {e}")));
                }
            }
        });
    }
}

impl eframe::App for MubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);
        let palette = Palette::for_mode(self.glass_mode);
        theme::paint_liquid_backdrop(ctx, self.glass_mode);

        let running_count = self
            .statuses
            .values()
            .filter(|s| matches!(s.state, InstanceState::Running | InstanceState::Preparing))
            .count();

        let mut next_mode = self.glass_mode;

        // Floating Apple Liquid Glass Sidebar Island
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(248.0)
            .frame(
                egui::Frame::NONE.inner_margin(Margin {
                    left: 14,
                    right: 6,
                    top: 14,
                    bottom: 14,
                }),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                theme::glass_box(
                    ui,
                    &palette,
                    GlassElevation::FloatingIsland,
                    22,
                    Margin::same(14),
                    |ui| {
                        ui.set_height(ui.available_height());
                        ui.set_width(ui.available_width());

                        // App Emblem & Title
                        ui.horizontal(|ui| {
                            theme::paint_app_emblem(ui, &palette, 38.0);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(
                                    RichText::new("ModsUndBots")
                                        .size(17.5)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                ui.label(
                                    RichText::new("Player & Bot Launcher")
                                        .size(11.0)
                                        .color(palette.text_muted),
                                );
                            });
                        });

                        ui.add_space(14.0);
                        theme::glass_divider(ui, &palette);
                        ui.add_space(4.0);
                        theme::section_eyebrow(ui, &palette, "Navigation");
                        ui.add_space(6.0);

                        let nav_items = [
                            (
                                Tab::Instances,
                                NavIcon::Instances,
                                "Instances",
                                Some(self.profiles.profiles.len()),
                            ),
                            (Tab::Mods, NavIcon::Mods, "Modrinth Mods", None),
                            (
                                Tab::Accounts,
                                NavIcon::Accounts,
                                "Accounts",
                                Some(self.accounts.accounts.len()),
                            ),
                            (Tab::Logs, NavIcon::Logs, "Live Logs", None),
                            (Tab::Settings, NavIcon::Settings, "Settings", None),
                        ];

                        for (tab, icon, label, badge) in nav_items {
                            if theme::nav_pill(
                                ui,
                                &palette,
                                icon,
                                label,
                                badge,
                                self.active_tab == tab,
                            )
                            .clicked()
                            {
                                self.active_tab = tab;
                            }
                            ui.add_space(3.0);
                        }

                        // Bottom Fleet Status + Appearance Switcher + Emergency Stop
                        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                            if running_count > 0 {
                                if theme::pill_button(
                                    ui,
                                    &palette,
                                    "Stop All Instances",
                                    ButtonTone::Danger,
                                )
                                .clicked()
                                {
                                    self.stop_all();
                                }
                                ui.add_space(8.0);
                            }

                            // Dark / Light Liquid Glass mode switcher
                            theme::segmented_control(
                                ui,
                                &palette,
                                &mut next_mode,
                                &[(GlassMode::Dark, "Dark Glass"), (GlassMode::Light, "Light Glass")],
                            );
                            ui.add_space(10.0);

                            // Frosted Fleet Status Well
                            theme::glass_box(
                                ui,
                                &palette,
                                GlassElevation::Well,
                                16,
                                Margin::same(12),
                                |ui| {
                                    ui.set_width(ui.available_width());
                                    theme::section_eyebrow(ui, &palette, "Fleet Status");
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        let dot_color = if running_count > 0 {
                                            palette.accent_emerald
                                        } else {
                                            palette.text_muted
                                        };
                                        theme::status_dot(ui, dot_color, running_count > 0);
                                        ui.label(
                                            RichText::new(format!("{running_count} Active"))
                                                .size(14.0)
                                                .strong()
                                                .color(palette.text_primary),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{} total profiles configured",
                                            self.profiles.profiles.len()
                                        ))
                                        .size(11.0)
                                        .color(palette.text_muted),
                                    );
                                },
                            );
                        });
                    },
                );
            });

        if next_mode != self.glass_mode {
            self.set_glass_mode(ctx, next_mode);
        }

        // Floating Liquid Glass Toast Notification
        if let Some((is_err, msg)) = self.banner_msg.clone() {
            egui::TopBottomPanel::bottom("banner")
                .frame(egui::Frame::NONE.inner_margin(Margin {
                    left: 20,
                    right: 20,
                    top: 0,
                    bottom: 14,
                }))
                .show_separator_line(false)
                .show(ctx, |ui| {
                    theme::glass_box(
                        ui,
                        &palette,
                        GlassElevation::Elevated,
                        18,
                        Margin::symmetric(16, 10),
                        |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let accent = if is_err {
                                    palette.accent_red
                                } else {
                                    palette.accent_emerald
                                };
                                theme::status_dot(ui, accent, true);
                                ui.label(RichText::new(msg).strong().color(palette.text_primary));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if theme::pill_button(
                                            ui,
                                            &palette,
                                            "Dismiss",
                                            ButtonTone::Subtle,
                                        )
                                        .clicked()
                                        {
                                            self.banner_msg = None;
                                        }
                                    },
                                );
                            });
                        },
                    );
                });
        }

        // Main Transparent Workspace Area
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(Margin {
                left: 16,
                right: 22,
                top: 20,
                bottom: 20,
            }))
            .show(ctx, |ui| match self.active_tab {
                Tab::Instances => self.ui_instances(ui, &palette),
                Tab::Mods => {
                    self.mods_view
                        .ui(ui, &palette, &self.rt, &self.paths, &self.profiles);
                }
                Tab::Accounts => self.ui_accounts(ui, &palette),
                Tab::Logs => self.ui_logs(ui, &palette),
                Tab::Settings => self.ui_settings(ui, ctx, &palette),
            });
    }
}

impl MubApp {
    fn ui_instances(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        // Header row
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Instances")
                        .size(26.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.label(
                    RichText::new("Manage and batch-launch player and bot Minecraft profiles.")
                        .size(12.5)
                        .color(palette.text_muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill_button(ui, palette, "+ New Profile", ButtonTone::Primary).clicked() {
                    self.show_new_profile = !self.show_new_profile;
                }
                if theme::pill_button(ui, palette, "Launch All Bots", ButtonTone::Secondary)
                    .clicked()
                {
                    let bots: Vec<Profile> = self
                        .profiles
                        .profiles
                        .iter()
                        .filter(|p| p.kind == ProfileKind::Bot)
                        .cloned()
                        .collect();
                    self.launch_profiles(bots);
                }
                if !self.selected_profiles.is_empty()
                    && theme::pill_button(
                        ui,
                        palette,
                        &format!("Launch Selected ({})", self.selected_profiles.len()),
                        ButtonTone::Secondary,
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

        ui.add_space(14.0);

        // Floating Liquid Glass Filter & Search Toolbar
        theme::glass_toolbar(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::segmented_control(
                    ui,
                    palette,
                    &mut self.profile_filter,
                    &[
                        (ProfileFilter::All, "All"),
                        (ProfileFilter::Players, "Players"),
                        (ProfileFilter::Bots, "Bots"),
                    ],
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::glass_search_bar(
                        ui,
                        palette,
                        &mut self.search_query,
                        "Search profiles...",
                        240.0,
                    );
                });
            });
        });

        ui.add_space(12.0);

        // New Profile Elevated Glass Sheet
        if self.show_new_profile {
            theme::glass_elevated(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Create New Profile")
                            .size(16.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                });
                ui.add_space(10.0);
                egui::Grid::new("new_profile_grid")
                    .num_columns(2)
                    .spacing([18.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Name").color(palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_name)
                                .hint_text("e.g. FarmBot-01")
                                .desired_width(250.0),
                        );
                        ui.end_row();

                        ui.label(RichText::new("Kind").color(palette.text_secondary));
                        if theme::segmented_control(
                            ui,
                            palette,
                            &mut self.new_prof_kind,
                            &[(ProfileKind::Player, "Player"), (ProfileKind::Bot, "Bot")],
                        ) {
                            self.new_prof_ram_max = match self.new_prof_kind {
                                ProfileKind::Player => 4096,
                                ProfileKind::Bot => 1024,
                            };
                        }
                        ui.end_row();

                        ui.label(RichText::new("Minecraft Version").color(palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_mc_version)
                                .desired_width(140.0),
                        );
                        ui.end_row();

                        ui.label(RichText::new("Mod Loader").color(palette.text_secondary));
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.new_prof_use_fabric, "Fabric");
                            if self.new_prof_use_fabric {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_prof_fabric_ver)
                                        .hint_text("Loader version (e.g. 0.16.10)")
                                        .desired_width(190.0),
                                );
                            }
                        });
                        ui.end_row();

                        ui.label(RichText::new("Max RAM (MB)").color(palette.text_secondary));
                        ui.add(
                            egui::Slider::new(&mut self.new_prof_ram_max, 256..=16384)
                                .step_by(256.0),
                        );
                        ui.end_row();

                        ui.label(RichText::new("Account").color(palette.text_secondary));
                        let selected_label = self
                            .new_prof_account_id
                            .as_deref()
                            .and_then(|id| self.accounts.get(id))
                            .map(|a| format!("{} ({})", a.username, a.role))
                            .unwrap_or_else(|| "<None>".to_string());
                        egui::ComboBox::from_id_salt("new_prof_acc")
                            .selected_text(selected_label)
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
                        ui.end_row();

                        ui.label(RichText::new("Quick-Connect").color(palette.text_secondary));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_server)
                                .hint_text("optional host:port")
                                .desired_width(250.0),
                        );
                        ui.end_row();
                    });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if theme::pill_button(ui, palette, "Create Profile", ButtonTone::Primary)
                        .clicked()
                        && !self.new_prof_name.trim().is_empty()
                    {
                        let loader = if self.new_prof_use_fabric
                            && !self.new_prof_fabric_ver.trim().is_empty()
                        {
                            ModLoader::Fabric {
                                loader_version: self.new_prof_fabric_ver.trim().to_string(),
                            }
                        } else {
                            ModLoader::Vanilla
                        };
                        let mut p = Profile::new(
                            self.new_prof_name.trim(),
                            self.new_prof_kind,
                            self.new_prof_mc_version.trim(),
                            loader,
                        );
                        p.memory_max_mb = self.new_prof_ram_max;
                        p.account_id = self.new_prof_account_id.clone();
                        if !self.new_prof_server.trim().is_empty() {
                            p.quick_connect = Some(self.new_prof_server.trim().to_string());
                        }
                        self.profiles.upsert(p);
                        let _ = self.profiles.save(&self.paths.profiles_file());
                        self.new_prof_name.clear();
                        self.show_new_profile = false;
                    }
                    if theme::pill_button(ui, palette, "Cancel", ButtonTone::Subtle).clicked() {
                        self.show_new_profile = false;
                    }
                });
            });
            ui.add_space(12.0);
        }

        // Empty state
        if self.profiles.profiles.is_empty() {
            theme::glass_card(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    ui.label(
                        RichText::new("No profiles yet")
                            .size(17.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Click '+ New Profile' to set up your first player or bot instance.",
                        )
                        .color(palette.text_muted),
                    );
                    ui.add_space(28.0);
                });
            });
            return;
        }

        let mut to_launch = Vec::new();
        let mut to_stop = Vec::new();
        let mut to_remove = Vec::new();
        let query = self.search_query.trim().to_lowercase();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for p in &self.profiles.profiles {
                match self.profile_filter {
                    ProfileFilter::Players if p.kind != ProfileKind::Player => continue,
                    ProfileFilter::Bots if p.kind != ProfileKind::Bot => continue,
                    _ => {}
                }
                if !query.is_empty() && !p.name.to_lowercase().contains(&query) {
                    continue;
                }

                let status = self.statuses.get(&p.id);
                let (status_label, status_color, is_active): (String, Color32, bool) =
                    match status.map(|s| &s.state) {
                        Some(InstanceState::Preparing) => {
                            let detail = status
                                .and_then(|s| s.progress.as_deref())
                                .unwrap_or("Preparing...");
                            (detail.to_string(), palette.accent_amber, true)
                        }
                        Some(InstanceState::Running) => {
                            let pid = status.and_then(|s| s.pid).unwrap_or(0);
                            (
                                format!("Running • PID {pid}"),
                                palette.accent_emerald,
                                true,
                            )
                        }
                        Some(InstanceState::Stopped { exit_code }) => (
                            format!("Stopped ({exit_code:?})"),
                            palette.text_muted,
                            false,
                        ),
                        Some(InstanceState::Failed { error }) => (
                            format!("Failed: {error}"),
                            palette.accent_red,
                            false,
                        ),
                        None => ("Idle".to_string(), palette.text_muted, false),
                    };

                theme::glass_card(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut checked = self.selected_profiles.contains(&p.id);
                        if ui.checkbox(&mut checked, "").changed() {
                            if checked {
                                self.selected_profiles.insert(p.id.clone());
                            } else {
                                self.selected_profiles.remove(&p.id);
                            }
                        }

                        let kind_accent = match p.kind {
                            ProfileKind::Player => palette.accent_blue,
                            ProfileKind::Bot => palette.accent_emerald,
                        };
                        theme::paint_avatar_badge(ui, &p.name, kind_accent, 38.0);
                        ui.add_space(4.0);

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&p.name)
                                        .size(15.5)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                match p.kind {
                                    ProfileKind::Player => {
                                        theme::pill_badge(ui, "PLAYER", palette.accent_blue)
                                    }
                                    ProfileKind::Bot => {
                                        theme::pill_badge(ui, "BOT", palette.accent_emerald)
                                    }
                                }
                                theme::status_pill(ui, &status_label, status_color, is_active);
                            });

                            let acc_name = p
                                .account_id
                                .as_deref()
                                .and_then(|id| self.accounts.get(id))
                                .map(|a| a.username.as_str())
                                .unwrap_or("Unassigned");

                            let mut meta = format!(
                                "MC {}  •  {}  •  Account: {}  •  {} MB",
                                p.mc_version, p.mod_loader, acc_name, p.memory_max_mb
                            );
                            if let Some(srv) = &p.quick_connect {
                                meta.push_str(&format!("  •  Server: {srv}"));
                            }
                            ui.label(RichText::new(meta).size(12.0).color(palette.text_muted));
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !is_active
                                && theme::pill_button(ui, palette, "Delete", ButtonTone::Subtle)
                                    .clicked()
                            {
                                to_remove.push(p.id.clone());
                            }
                            if theme::pill_button(ui, palette, "Mods", ButtonTone::Secondary)
                                .clicked()
                            {
                                self.mods_view.selected_profile_id = Some(p.id.clone());
                                self.mods_view.loaded_once = false;
                                self.mods_view.active_project = None;
                                self.active_tab = Tab::Mods;
                            }
                            if theme::pill_button(ui, palette, "Logs", ButtonTone::Secondary)
                                .clicked()
                            {
                                self.selected_log_profile = Some(p.id.clone());
                                self.active_tab = Tab::Logs;
                            }
                            if is_active {
                                if theme::pill_button(ui, palette, "Stop", ButtonTone::Danger)
                                    .clicked()
                                {
                                    to_stop.push(p.id.clone());
                                }
                            } else if theme::pill_button(
                                ui,
                                palette,
                                "Launch",
                                ButtonTone::Primary,
                            )
                            .clicked()
                            {
                                to_launch.push(p.clone());
                            }
                        });
                    });
                });
                ui.add_space(8.0);
            }
        });

        if !to_launch.is_empty() {
            self.launch_profiles(to_launch);
        }
        for id in to_stop {
            self.stop_profile(id);
        }
        if !to_remove.is_empty() {
            for id in to_remove {
                self.profiles.remove(&id);
                self.selected_profiles.remove(&id);
            }
            let _ = self.profiles.save(&self.paths.profiles_file());
        }
    }

    fn ui_accounts(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Accounts")
                        .size(26.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.label(
                    RichText::new("Manage Microsoft (MSA) player/bot accounts and offline test bots.")
                        .size(12.5)
                        .color(palette.text_muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill_button(ui, palette, "+ Microsoft Bot", ButtonTone::Primary).clicked()
                {
                    self.start_msa_login(AccountRole::Bot);
                }
                if theme::pill_button(ui, palette, "+ Microsoft Player", ButtonTone::Secondary)
                    .clicked()
                {
                    self.start_msa_login(AccountRole::Player);
                }
            });
        });

        ui.add_space(14.0);

        if let Some(role) = self.pending_live_auth {
            let auth_url = MsaAuthClient::live_authorize_url();
            theme::glass_elevated(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!("Microsoft Sign-In ({role})"))
                        .size(15.5)
                        .strong()
                        .color(palette.accent_blue),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("1. Open Microsoft login in your browser:");
                    ui.hyperlink_to("Sign in at login.live.com", &auth_url);
                    if theme::pill_button(ui, palette, "Copy URL", ButtonTone::Secondary).clicked()
                    {
                        ui.ctx().copy_text(auth_url.clone());
                    }
                });
                ui.label(
                    RichText::new(
                        "2. After signing in, you'll land on a blank page. Copy the full URL from your browser address bar and paste it below:",
                    )
                    .size(12.0)
                    .color(palette.text_muted),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.live_auth_code_input)
                            .hint_text("https://login.live.com/oauth20_desktop.srf?code=...")
                            .desired_width(420.0),
                    );
                    if theme::pill_button(ui, palette, "Complete Sign-In", ButtonTone::Primary)
                        .clicked()
                    {
                        self.complete_live_login(role);
                    }
                    if theme::pill_button(ui, palette, "Cancel", ButtonTone::Subtle).clicked() {
                        self.pending_live_auth = None;
                        self.live_auth_code_input.clear();
                    }
                });
            });
            ui.add_space(12.0);
        }

        if let Some((dev, role)) = &self.pending_device_code {
            theme::glass_elevated(ui, palette, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "Microsoft Sign-In ({role}) — Waiting for browser completion"
                    ))
                    .strong()
                    .color(palette.accent_amber),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("1. Open:");
                    ui.hyperlink(&dev.verification_uri);
                });
                ui.horizontal(|ui| {
                    ui.label("2. Enter code:");
                    theme::pill_badge(ui, &dev.user_code, palette.accent_emerald);
                    if theme::pill_button(ui, palette, "Copy Code", ButtonTone::Secondary).clicked()
                    {
                        ui.ctx().copy_text(dev.user_code.clone());
                    }
                });
            });
            ui.add_space(12.0);
        }

        // Offline bot quick-add glass bar
        theme::glass_toolbar(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Offline / Local Bot")
                        .strong()
                        .color(palette.text_primary),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_offline_name)
                        .hint_text("Username (e.g. Bot01)")
                        .desired_width(190.0),
                );
                theme::segmented_control(
                    ui,
                    palette,
                    &mut self.new_offline_role,
                    &[(AccountRole::Bot, "Bot"), (AccountRole::Player, "Player")],
                );
                if theme::pill_button(ui, palette, "Add Offline", ButtonTone::Secondary).clicked()
                    && !self.new_offline_name.trim().is_empty()
                {
                    let acc =
                        Account::new_offline(self.new_offline_name.trim(), self.new_offline_role);
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                    self.new_offline_name.clear();
                }
            });
        });

        ui.add_space(12.0);

        let mut to_remove = Vec::new();
        let mut to_refresh = Vec::new();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for acc in &self.accounts.accounts {
                theme::glass_card(ui, palette, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let role_accent = match acc.role {
                            AccountRole::Player => palette.accent_blue,
                            AccountRole::Bot => palette.accent_emerald,
                        };
                        theme::paint_avatar_badge(ui, &acc.username, role_accent, 38.0);
                        ui.add_space(4.0);

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&acc.username)
                                        .size(15.0)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                match acc.role {
                                    AccountRole::Player => {
                                        theme::pill_badge(ui, "PLAYER", palette.accent_blue)
                                    }
                                    AccountRole::Bot => {
                                        theme::pill_badge(ui, "BOT", palette.accent_emerald)
                                    }
                                }
                                if acc.is_msa() {
                                    theme::pill_badge(ui, "MSA", palette.accent_cyan);
                                    if acc.is_token_valid() {
                                        theme::status_pill(
                                            ui,
                                            "Token Valid",
                                            palette.accent_emerald,
                                            false,
                                        );
                                    } else {
                                        theme::status_pill(
                                            ui,
                                            "Needs Refresh",
                                            palette.accent_amber,
                                            true,
                                        );
                                    }
                                } else {
                                    theme::pill_badge(ui, "OFFLINE", palette.text_muted);
                                }
                            });
                            ui.label(
                                RichText::new(format!("UUID: {}", acc.mc_uuid))
                                    .size(11.5)
                                    .monospace()
                                    .color(palette.text_muted),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::pill_button(ui, palette, "Remove", ButtonTone::Subtle)
                                .clicked()
                            {
                                to_remove.push(acc.id.clone());
                            }
                            if acc.is_msa()
                                && theme::pill_button(
                                    ui,
                                    palette,
                                    "Refresh Token",
                                    ButtonTone::Secondary,
                                )
                                .clicked()
                            {
                                to_refresh.push(acc.clone());
                            }
                        });
                    });
                });
                ui.add_space(8.0);
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
                        let _ = tx.send(BgEvent::Error(format!("Refresh failed: {e}")));
                    }
                }
            });
        }
        if !to_remove.is_empty() {
            for id in to_remove {
                self.accounts.remove(&id);
            }
            let _ = self.accounts.save(&self.paths.accounts_file());
        }
    }

    fn ui_logs(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Live Logs")
                        .size(26.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.label(
                    RichText::new("Inspect real-time stdout & stderr streams from active instances.")
                        .size(12.5)
                        .color(palette.text_muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(id) = &self.selected_log_profile
                    && let Some(status) = self.statuses.get(id)
                    && theme::pill_button(ui, palette, "Copy Logs", ButtonTone::Secondary).clicked()
                {
                    let joined = status.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                    ui.ctx().copy_text(joined);
                }

                let current_name = self
                    .selected_log_profile
                    .as_deref()
                    .and_then(|id| self.statuses.get(id))
                    .map(|s| format!("{} ({})", s.profile_name, s.account_name))
                    .unwrap_or_else(|| "<Select instance>".to_string());

                egui::ComboBox::from_id_salt("log_instance_select")
                    .selected_text(current_name)
                    .show_ui(ui, |ui| {
                        for s in self.statuses.values() {
                            ui.selectable_value(
                                &mut self.selected_log_profile,
                                Some(s.profile_id.clone()),
                                format!("{} ({})", s.profile_name, s.account_name),
                            );
                        }
                    });
            });
        });

        ui.add_space(12.0);

        let selected_status = self
            .selected_log_profile
            .as_deref()
            .and_then(|id| self.statuses.get(id));

        theme::glass_terminal(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());

            // macOS-style smoked glass terminal header bar
            egui::Frame::NONE
                .inner_margin(Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let is_running = selected_status
                            .map(|s| matches!(s.state, InstanceState::Running))
                            .unwrap_or(false);
                        theme::status_dot(
                            ui,
                            if is_running {
                                palette.accent_emerald
                            } else {
                                Color32::from_rgb(130, 142, 168)
                            },
                            is_running,
                        );
                        let title = selected_status
                            .map(|s| format!("{} — {}", s.profile_name, s.account_name))
                            .unwrap_or_else(|| "Console".to_string());
                        ui.label(
                            RichText::new(title)
                                .size(12.5)
                                .strong()
                                .color(Color32::from_rgb(236, 242, 255)),
                        );

                        if let Some(st) = selected_status {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!("{} lines", st.logs.len()))
                                            .size(11.0)
                                            .monospace()
                                            .color(Color32::from_rgb(150, 164, 192)),
                                    );
                                },
                            );
                        }
                    });
                });

            // Hairline separator inside terminal
            let (sep, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(sep, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 22));

            egui::Frame::NONE
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(ui.available_height());
                    if let Some(status) = selected_status {
                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for line in &status.logs {
                                    let color = if line.contains("ERROR")
                                        || line.contains("Exception")
                                    {
                                        Color32::from_rgb(255, 112, 112)
                                    } else if line.contains("WARN") {
                                        Color32::from_rgb(255, 206, 84)
                                    } else {
                                        Color32::from_rgb(234, 240, 252)
                                    };
                                    ui.label(
                                        RichText::new(line).monospace().size(12.0).color(color),
                                    );
                                }
                            });
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new(
                                    "Select a running or stopped instance to inspect logs.",
                                )
                                .color(Color32::from_rgb(156, 170, 196)),
                            );
                        });
                    }
                });
        });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, palette: &Palette) {
        ui.label(
            RichText::new("Settings")
                .size(26.0)
                .strong()
                .color(palette.text_primary),
        );
        ui.label(
            RichText::new("Appearance, launcher paths, Java runtime, and batch launch tuning.")
                .size(12.5)
                .color(palette.text_muted),
        );
        ui.add_space(14.0);

        // Appearance Card
        theme::glass_card(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            theme::section_eyebrow(ui, palette, "Appearance");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Liquid Glass Material")
                            .size(14.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Switch between midnight obsidian glass and frosted daylight glass.")
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut mode = self.glass_mode;
                    if theme::segmented_control(
                        ui,
                        palette,
                        &mut mode,
                        &[(GlassMode::Dark, "Dark Glass"), (GlassMode::Light, "Light Glass")],
                    ) {
                        self.set_glass_mode(ctx, mode);
                    }
                });
            });
        });

        ui.add_space(12.0);

        // Runtime & Paths Card
        theme::glass_card(ui, palette, |ui| {
            ui.set_width(ui.available_width());
            theme::section_eyebrow(ui, palette, "Runtime & Launch Tuning");
            ui.add_space(10.0);
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([22.0, 12.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Shared Data Directory").color(palette.text_secondary));
                    ui.label(
                        RichText::new(self.paths.root().display().to_string())
                            .monospace()
                            .color(palette.text_muted),
                    );
                    ui.end_row();

                    ui.label(
                        RichText::new("Default Java Executable").color(palette.text_secondary),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.default_java_path)
                            .desired_width(320.0),
                    );
                    ui.end_row();

                    ui.label(RichText::new("MSA Client ID").color(palette.text_secondary));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.msa_client_id)
                            .desired_width(320.0),
                    );
                    ui.end_row();

                    ui.label(
                        RichText::new("Batch Launch Stagger (ms)").color(palette.text_secondary),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.batch_launch_stagger_ms).speed(50),
                    );
                    ui.end_row();

                    ui.label(
                        RichText::new("Max Concurrent Downloads").color(palette.text_secondary),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.max_concurrent_downloads)
                            .range(1..=64),
                    );
                    ui.end_row();
                });

            ui.add_space(16.0);
            if theme::pill_button(ui, palette, "Save Settings", ButtonTone::Primary).clicked() {
                match self.settings.save(&self.paths.settings_file()) {
                    Ok(()) => {
                        self.banner_msg = Some((false, "Settings saved.".to_string()));
                    }
                    Err(e) => {
                        self.banner_msg = Some((true, format!("Failed to save settings: {e}")));
                    }
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1220.0, 780.0])
            .with_min_inner_size([960.0, 620.0])
            .with_title("ModsUndBots Launcher"),
        ..Default::default()
    };
    eframe::run_native(
        "ModsUndBots",
        options,
        Box::new(|cc| Ok(Box::new(MubApp::new(cc)))),
    )
}
