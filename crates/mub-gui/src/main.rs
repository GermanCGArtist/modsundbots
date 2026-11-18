mod mods_view;
mod theme;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};
use mods_view::ModsViewState;
use mub_core::{
    Account, AccountRole, AccountStore, AppPaths, DeviceCodeResponse, InstanceManager,
    InstanceState, InstanceStatus, LauncherSettings, ModLoader, MsaAuthClient, Profile,
    ProfileKind, ProfileStore,
};
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
        theme::apply_theme(&cc.egui_ctx);
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
        self.mods_view.drain_events(ctx, &self.rt, &mut self.banner_msg);
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
        theme::paint_liquid_backdrop(ctx);

        let running_count = self
            .statuses
            .values()
            .filter(|s| matches!(s.state, InstanceState::Running | InstanceState::Preparing))
            .count();

        // Left Liquid-Glass Sidebar Navigation
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(214.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::GLASS_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, theme::GLASS_BORDER))
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("ModsUndBots")
                        .size(20.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new("Player & Bot Launcher")
                        .size(11.5)
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(12.0);
                theme::glass_divider(ui);
                ui.add_space(6.0);

                let nav_items = [
                    (
                        Tab::Instances,
                        format!("Instances ({})", self.profiles.profiles.len()),
                    ),
                    (Tab::Mods, "Modrinth Mods".to_string()),
                    (
                        Tab::Accounts,
                        format!("Accounts ({})", self.accounts.accounts.len()),
                    ),
                    (Tab::Logs, "Live Logs".to_string()),
                    (Tab::Settings, "Settings".to_string()),
                ];

                for (tab, label) in nav_items {
                    let selected = self.active_tab == tab;
                    let fill = if selected {
                        Color32::from_rgba_unmultiplied(45, 212, 160, 42)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let stroke = if selected {
                        Stroke::new(1.0_f32, theme::ACCENT_EMERALD)
                    } else {
                        Stroke::NONE
                    };
                    let text_color = if selected {
                        theme::ACCENT_EMERALD
                    } else {
                        theme::TEXT_PRIMARY
                    };

                    let btn = egui::Button::new(RichText::new(label).size(14.0).color(text_color))
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(CornerRadius::same(12))
                        .min_size(egui::vec2(ui.available_width(), 36.0));
                    if ui.add(btn).clicked() {
                        self.active_tab = tab;
                    }
                    ui.add_space(4.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if running_count > 0
                        && ui
                            .add_sized(
                                [ui.available_width(), 34.0],
                                theme::danger_button("Stop All Instances"),
                            )
                            .clicked()
                    {
                        self.stop_all();
                    }

                    ui.add_space(8.0);
                    theme::card_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let dot_color = if running_count > 0 {
                                theme::ACCENT_EMERALD
                            } else {
                                theme::TEXT_MUTED
                            };
                            theme::status_dot(ui, dot_color);
                            ui.label(
                                RichText::new(format!("{running_count} Active"))
                                    .strong()
                                    .color(theme::TEXT_PRIMARY),
                            );
                        });
                        ui.label(
                            RichText::new(format!("{} Total Profiles", self.profiles.profiles.len()))
                                .size(11.5)
                                .color(theme::TEXT_MUTED),
                        );
                    });
                });
            });

        // Notification Banner
        if let Some((is_err, msg)) = &self.banner_msg.clone() {
            egui::TopBottomPanel::bottom("banner")
                .frame(
                    egui::Frame::new()
                        .fill(if *is_err {
                            Color32::from_rgba_unmultiplied(80, 22, 28, 220)
                        } else {
                            Color32::from_rgba_unmultiplied(16, 60, 44, 220)
                        })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if *is_err {
                                theme::ACCENT_RED
                            } else {
                                theme::ACCENT_EMERALD
                            },
                        ))
                        .inner_margin(Margin::symmetric(16, 8)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let color = if *is_err {
                            theme::ACCENT_RED
                        } else {
                            theme::ACCENT_EMERALD
                        };
                        ui.label(RichText::new(msg).strong().color(color));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Dismiss").clicked() {
                                self.banner_msg = None;
                            }
                        });
                    });
                });
        }

        // Main Transparent Workspace Area (lets ambient aurora show through glass cards)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin::same(22)),
            )
            .show(ctx, |ui| match self.active_tab {
                Tab::Instances => self.ui_instances(ui),
                Tab::Mods => {
                    self.mods_view
                        .ui(ui, &self.rt, &self.paths, &self.profiles);
                }
                Tab::Accounts => self.ui_accounts(ui),
                Tab::Logs => self.ui_logs(ui),
                Tab::Settings => self.ui_settings(ui),
            });
    }
}

impl MubApp {
    fn ui_instances(&mut self, ui: &mut egui::Ui) {
        // Header row
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Instances")
                        .size(22.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new("Manage and batch-launch player and bot Minecraft profiles.")
                        .size(12.5)
                        .color(theme::TEXT_MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(theme::primary_button("+ New Profile")).clicked() {
                    self.show_new_profile = !self.show_new_profile;
                }
                if ui.button("Launch All Bots").clicked() {
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
                    && ui
                        .button(format!("Launch Selected ({})", self.selected_profiles.len()))
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

        ui.add_space(12.0);

        // Filter & Search bar
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.profile_filter, ProfileFilter::All, "All");
            ui.selectable_value(&mut self.profile_filter, ProfileFilter::Players, "Players");
            ui.selectable_value(&mut self.profile_filter, ProfileFilter::Bots, "Bots");
            ui.add_space(12.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Search profiles...")
                    .desired_width(220.0),
            );
        });

        ui.add_space(10.0);

        // New Profile Card
        if self.show_new_profile {
            theme::card_frame().show(ui, |ui| {
                ui.label(RichText::new("Create New Profile").size(15.0).strong());
                ui.add_space(8.0);
                egui::Grid::new("new_profile_grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_name)
                                .hint_text("e.g. FarmBot-01")
                                .desired_width(240.0),
                        );
                        ui.end_row();

                        ui.label("Kind");
                        ui.horizontal(|ui| {
                            if ui
                                .radio_value(&mut self.new_prof_kind, ProfileKind::Player, "Player")
                                .changed()
                            {
                                self.new_prof_ram_max = 4096;
                            }
                            if ui
                                .radio_value(&mut self.new_prof_kind, ProfileKind::Bot, "Bot")
                                .changed()
                            {
                                self.new_prof_ram_max = 1024;
                            }
                        });
                        ui.end_row();

                        ui.label("Minecraft Version");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_mc_version)
                                .desired_width(140.0),
                        );
                        ui.end_row();

                        ui.label("Mod Loader");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.new_prof_use_fabric, "Fabric");
                            if self.new_prof_use_fabric {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_prof_fabric_ver)
                                        .hint_text("Loader version (e.g. 0.16.10)")
                                        .desired_width(180.0),
                                );
                            }
                        });
                        ui.end_row();

                        ui.label("Max RAM (MB)");
                        ui.add(egui::Slider::new(&mut self.new_prof_ram_max, 256..=16384).step_by(256.0));
                        ui.end_row();

                        ui.label("Account");
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

                        ui.label("Quick-Connect");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_prof_server)
                                .hint_text("optional host:port")
                                .desired_width(240.0),
                        );
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(theme::primary_button("Create Profile")).clicked()
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
                    if ui.button("Cancel").clicked() {
                        self.show_new_profile = false;
                    }
                });
            });
            ui.add_space(10.0);
        }

        // Empty state
        if self.profiles.profiles.is_empty() {
            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new("No profiles yet").size(16.0).strong());
                    ui.label(
                        RichText::new("Click '+ New Profile' to set up your first player or bot instance.")
                            .color(theme::TEXT_MUTED),
                    );
                    ui.add_space(20.0);
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
                            (detail.to_string(), theme::ACCENT_AMBER, true)
                        }
                        Some(InstanceState::Running) => {
                            let pid = status.and_then(|s| s.pid).unwrap_or(0);
                            (
                                format!("Running • PID {pid}"),
                                theme::ACCENT_EMERALD,
                                true,
                            )
                        }
                        Some(InstanceState::Stopped { exit_code }) => (
                            format!("Stopped ({exit_code:?})"),
                            theme::TEXT_MUTED,
                            false,
                        ),
                        Some(InstanceState::Failed { error }) => (
                            format!("Failed: {error}"),
                            theme::ACCENT_RED,
                            false,
                        ),
                        None => ("Idle".to_string(), theme::TEXT_MUTED, false),
                    };

                theme::card_frame().show(ui, |ui| {
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

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&p.name)
                                        .size(15.5)
                                        .strong()
                                        .color(theme::TEXT_PRIMARY),
                                );
                                match p.kind {
                                    ProfileKind::Player => {
                                        theme::pill_badge(ui, "PLAYER", theme::ACCENT_BLUE)
                                    }
                                    ProfileKind::Bot => {
                                        theme::pill_badge(ui, "BOT", theme::ACCENT_EMERALD)
                                    }
                                }
                                theme::pill_badge(ui, &status_label, status_color);
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
                            ui.label(RichText::new(meta).size(12.0).color(theme::TEXT_MUTED));
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !is_active && ui.button("Delete").clicked() {
                                to_remove.push(p.id.clone());
                            }
                            if ui.button("Mods").clicked() {
                                self.mods_view.selected_profile_id = Some(p.id.clone());
                                self.mods_view.loaded_once = false;
                                self.mods_view.active_project = None;
                                self.active_tab = Tab::Mods;
                            }
                            if ui.button("Logs").clicked() {
                                self.selected_log_profile = Some(p.id.clone());
                                self.active_tab = Tab::Logs;
                            }
                            if is_active {
                                if ui.add(theme::danger_button("Stop")).clicked() {
                                    to_stop.push(p.id.clone());
                                }
                            } else if ui.add(theme::primary_button("Launch")).clicked() {
                                to_launch.push(p.clone());
                            }
                        });
                    });
                });
                ui.add_space(6.0);
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

    fn ui_accounts(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Accounts")
                        .size(22.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new("Manage Microsoft (MSA) player/bot accounts and offline test bots.")
                        .size(12.5)
                        .color(theme::TEXT_MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(theme::primary_button("+ Microsoft Bot")).clicked() {
                    self.start_msa_login(AccountRole::Bot);
                }
                if ui.button("+ Microsoft Player").clicked() {
                    self.start_msa_login(AccountRole::Player);
                }
            });
        });

        ui.add_space(12.0);

        if let Some(role) = self.pending_live_auth {
            let auth_url = MsaAuthClient::live_authorize_url();
            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!("Microsoft Sign-In ({role})"))
                        .size(15.0)
                        .strong()
                        .color(theme::ACCENT_EMERALD),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("1. Open Microsoft login in your browser:");
                    ui.hyperlink_to("Sign in at login.live.com", &auth_url);
                    if ui.button("Copy URL").clicked() {
                        ui.ctx().copy_text(auth_url.clone());
                    }
                });
                ui.label(
                    RichText::new(
                        "2. After signing in, you'll land on a blank page. Copy the full URL from your browser address bar and paste it below:",
                    )
                    .size(12.0)
                    .color(theme::TEXT_MUTED),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.live_auth_code_input)
                            .hint_text("https://login.live.com/oauth20_desktop.srf?code=...")
                            .desired_width(420.0),
                    );
                    if ui.add(theme::primary_button("Complete Sign-In")).clicked() {
                        self.complete_live_login(role);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_live_auth = None;
                        self.live_auth_code_input.clear();
                    }
                });
            });
            ui.add_space(10.0);
        }

        if let Some((dev, role)) = &self.pending_device_code {
            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "Microsoft Sign-In ({role}) — Waiting for browser completion"
                    ))
                    .strong()
                    .color(theme::ACCENT_AMBER),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("1. Open:");
                    ui.hyperlink(&dev.verification_uri);
                });
                ui.horizontal(|ui| {
                    ui.label("2. Enter code:");
                    theme::pill_badge(ui, &dev.user_code, theme::ACCENT_EMERALD);
                    if ui.button("Copy Code").clicked() {
                        ui.ctx().copy_text(dev.user_code.clone());
                    }
                });
            });
            ui.add_space(10.0);
        }

        // Offline bot quick-add card
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Offline / Local Bot:").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_offline_name)
                        .hint_text("Username (e.g. Bot01)")
                        .desired_width(180.0),
                );
                egui::ComboBox::from_id_salt("offline_role")
                    .selected_text(self.new_offline_role.to_string())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.new_offline_role, AccountRole::Bot, "Bot");
                        ui.selectable_value(
                            &mut self.new_offline_role,
                            AccountRole::Player,
                            "Player",
                        );
                    });
                if ui.button("Add Offline").clicked() && !self.new_offline_name.trim().is_empty() {
                    let acc =
                        Account::new_offline(self.new_offline_name.trim(), self.new_offline_role);
                    self.accounts.upsert(acc);
                    let _ = self.accounts.save(&self.paths.accounts_file());
                    self.new_offline_name.clear();
                }
            });
        });

        ui.add_space(10.0);

        let mut to_remove = Vec::new();
        let mut to_refresh = Vec::new();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for acc in &self.accounts.accounts {
                theme::card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&acc.username)
                                        .size(15.0)
                                        .strong()
                                        .color(theme::TEXT_PRIMARY),
                                );
                                match acc.role {
                                    AccountRole::Player => {
                                        theme::pill_badge(ui, "PLAYER", theme::ACCENT_BLUE)
                                    }
                                    AccountRole::Bot => {
                                        theme::pill_badge(ui, "BOT", theme::ACCENT_EMERALD)
                                    }
                                }
                                if acc.is_msa() {
                                    theme::pill_badge(ui, "MSA", theme::ACCENT_BLUE);
                                    if acc.is_token_valid() {
                                        theme::pill_badge(
                                            ui,
                                            "Token Valid",
                                            theme::ACCENT_EMERALD,
                                        );
                                    } else {
                                        theme::pill_badge(
                                            ui,
                                            "Needs Refresh",
                                            theme::ACCENT_AMBER,
                                        );
                                    }
                                } else {
                                    theme::pill_badge(ui, "OFFLINE", theme::TEXT_MUTED);
                                }
                            });
                            ui.label(
                                RichText::new(format!("UUID: {}", acc.mc_uuid))
                                    .size(11.5)
                                    .color(theme::TEXT_MUTED),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                to_remove.push(acc.id.clone());
                            }
                            if acc.is_msa() && ui.button("Refresh Token").clicked() {
                                to_refresh.push(acc.clone());
                            }
                        });
                    });
                });
                ui.add_space(6.0);
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

    fn ui_logs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Live Logs")
                    .size(22.0)
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(id) = &self.selected_log_profile
                    && let Some(status) = self.statuses.get(id)
                    && ui.button("Copy Logs").clicked()
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

        ui.add_space(10.0);

        egui::Frame::new()
            .fill(theme::BG_TERMINAL)
            .stroke(Stroke::new(1.0_f32, theme::GLASS_BORDER))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                if let Some(id) = &self.selected_log_profile
                    && let Some(status) = self.statuses.get(id)
                {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &status.logs {
                                let color = if line.contains("ERROR") || line.contains("Exception")
                                {
                                    theme::ACCENT_RED
                                } else if line.contains("WARN") {
                                    theme::ACCENT_AMBER
                                } else {
                                    theme::TEXT_PRIMARY
                                };
                                ui.label(RichText::new(line).monospace().size(12.0).color(color));
                            }
                        });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("Select a running or stopped instance to inspect logs.")
                                .color(theme::TEXT_MUTED),
                        );
                    });
                }
            });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Settings")
                .size(22.0)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.label(
            RichText::new("Global launcher paths, Java runtime, and batch launch tuning.")
                .size(12.5)
                .color(theme::TEXT_MUTED),
        );
        ui.add_space(12.0);

        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([20.0, 12.0])
                .show(ui, |ui| {
                    ui.label("Shared Data Directory");
                    ui.label(
                        RichText::new(self.paths.root().display().to_string())
                            .monospace()
                            .color(theme::TEXT_MUTED),
                    );
                    ui.end_row();

                    ui.label("Default Java Executable");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.default_java_path)
                            .desired_width(300.0),
                    );
                    ui.end_row();

                    ui.label("MSA Client ID");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.msa_client_id)
                            .desired_width(300.0),
                    );
                    ui.end_row();

                    ui.label("Batch Launch Stagger (ms)");
                    ui.add(
                        egui::DragValue::new(&mut self.settings.batch_launch_stagger_ms).speed(50),
                    );
                    ui.end_row();

                    ui.label("Max Concurrent Downloads");
                    ui.add(
                        egui::DragValue::new(&mut self.settings.max_concurrent_downloads)
                            .range(1..=64),
                    );
                    ui.end_row();
                });

            ui.add_space(14.0);
            if ui.add(theme::primary_button("Save Settings")).clicked() {
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
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([920.0, 600.0])
            .with_title("ModsUndBots Launcher"),
        ..Default::default()
    };
    eframe::run_native(
        "ModsUndBots",
        options,
        Box::new(|cc| Ok(Box::new(MubApp::new(cc)))),
    )
}
