//! Bot Wall: live thumbnails of every running instance, so hidden bot windows can be watched
//! without cluttering the desktop. Click a picture (or SHOW) to bring the real window to the front.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Stroke};
use mub_core::window::{self, Frame};
use mub_core::{InstanceState, InstanceStatus, ProfileKind, ProfileStore};

use crate::instance_view::format_playtime;
use crate::theme::{self, ButtonTone, Palette};

/// Thumbnail width in pixels; frames are scaled down to this before they reach the GUI thread.
const THUMB_W: u32 = 320;
const CAPTURE_EVERY: Duration = Duration::from_millis(1000);

pub enum WallAction {
    Show(String),
    Hide(String),
    HideAllBots,
    ShowAll,
    UnhideOrphans,
    OpenConsole(String),
}

#[derive(Default)]
struct Shared {
    /// (profile id, pid) of the instances to capture.
    targets: Vec<(String, u32)>,
    /// Latest frame per profile, with whether its window is hidden and when it was taken.
    frames: HashMap<String, Captured>,
    /// The capture thread only works while the Wall is on screen.
    visible_until: Option<Instant>,
}

struct Captured {
    frame: Option<Frame>,
    hidden: Option<bool>,
    at: Instant,
}

struct Thumb {
    tex: egui::TextureHandle,
    at: Instant,
}

pub struct WallState {
    shared: Arc<Mutex<Shared>>,
    thumbs: HashMap<String, Thumb>,
    hidden: HashMap<String, Option<bool>>,
}

impl WallState {
    pub fn new(ctx: &egui::Context) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let worker = Arc::clone(&shared);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("mub-wall-capture".into())
            .spawn(move || capture_loop(worker, ctx))
            .expect("spawn wall capture thread");
        Self {
            shared,
            thumbs: HashMap::new(),
            hidden: HashMap::new(),
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        statuses: &HashMap<String, InstanceStatus>,
        profiles: &ProfileStore,
    ) -> Vec<WallAction> {
        let mut actions = Vec::new();

        // Running instances: bots first, then by name.
        let mut live: Vec<&InstanceStatus> = statuses
            .values()
            .filter(|s| matches!(s.state, InstanceState::Running | InstanceState::Preparing))
            .collect();
        let is_bot = |s: &InstanceStatus| {
            profiles
                .get(&s.profile_id)
                .map(|p| p.kind == ProfileKind::Bot)
                .unwrap_or(true)
        };
        live.sort_by(|a, b| {
            is_bot(b)
                .cmp(&is_bot(a))
                .then_with(|| a.profile_name.to_lowercase().cmp(&b.profile_name.to_lowercase()))
        });

        self.sync_with_capture(ui.ctx(), &live);

        // Header
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("BOT WALL")
                    .monospace()
                    .size(16.0)
                    .strong()
                    .color(palette.ink),
            );
            let hidden_n = live
                .iter()
                .filter(|s| self.hidden.get(&s.profile_id).copied().flatten().unwrap_or(false))
                .count();
            ui.label(
                RichText::new(format!("{} running · {} hidden", live.len(), hidden_n))
                    .monospace()
                    .size(12.0)
                    .color(palette.ink_dim),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::term_button(ui, palette, "UNHIDE LOST", ButtonTone::Subtle)
                    .on_hover_text(
                        "Show every hidden Java game window on this PC, also ones mub no longer tracks",
                    )
                    .clicked()
                {
                    actions.push(WallAction::UnhideOrphans);
                }
                if theme::term_button(ui, palette, "SHOW ALL", ButtonTone::Secondary).clicked() {
                    actions.push(WallAction::ShowAll);
                }
                if theme::term_button(ui, palette, "HIDE ALL BOTS", ButtonTone::Bot).clicked() {
                    actions.push(WallAction::HideAllBots);
                }
            });
        });
        theme::term_divider(ui, palette);
        ui.add_space(6.0);

        if live.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No instances running.")
                        .monospace()
                        .size(14.0)
                        .color(palette.ink_dim),
                );
                ui.label(
                    RichText::new(
                        "Bot windows start hidden and show up here as live pictures.\nClick a picture to bring that game to the front.",
                    )
                    .monospace()
                    .size(12.0)
                    .color(palette.ink_muted),
                );
            });
            return actions;
        }

        let thumb = egui::vec2(THUMB_W as f32, THUMB_W as f32 * 9.0 / 16.0);
        let gap = 10.0;
        let tile_w = thumb.x + 2.0 * 8.0 + 2.0;
        let cols = (((ui.available_width() + gap) / (tile_w + gap)).floor() as usize).max(1);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in live.chunks(cols) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for s in row {
                            self.tile(ui, palette, s, is_bot(s), thumb, &mut actions);
                        }
                    });
                    ui.add_space(gap);
                }
            });

        actions
    }

    fn tile(
        &self,
        ui: &mut egui::Ui,
        palette: &Palette,
        s: &InstanceStatus,
        bot: bool,
        thumb: egui::Vec2,
        actions: &mut Vec<WallAction>,
    ) {
        let hidden = self.hidden.get(&s.profile_id).copied().flatten();
        let border = match hidden {
            Some(false) => palette.focus,
            _ => palette.border,
        };
        theme::term_box(ui, palette.surface, border, Margin::same(8), |ui| {
            ui.set_width(thumb.x);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&s.profile_name)
                        .monospace()
                        .size(13.0)
                        .strong()
                        .color(palette.ink),
                );
                if bot {
                    theme::term_badge(ui, "BOT", palette.bot, palette.bot_bg);
                } else {
                    theme::term_badge(ui, "PLAYER", palette.player, palette.player_bg);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match (&s.state, hidden) {
                        (InstanceState::Preparing, _) => {
                            theme::term_badge(ui, "STARTING", palette.warn, palette.warn_bg)
                        }
                        (_, Some(true)) => theme::term_badge(ui, "HIDDEN", palette.meta, palette.meta_bg),
                        (_, Some(false)) => {
                            theme::term_badge(ui, "SHOWN", palette.focus, palette.focus_bg)
                        }
                        (_, None) => theme::term_badge(ui, "NO WINDOW", palette.ink_muted, palette.raised),
                    }
                });
            });
            ui.add_space(4.0);

            // Picture (click = show + focus)
            let (rect, resp) = ui.allocate_exact_size(thumb, Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_gray(12));
            if let Some(t) = self.thumbs.get(&s.profile_id) {
                let size = t.tex.size_vec2();
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                let img = egui::Rect::from_center_size(rect.center(), size * scale);
                painter.image(
                    t.tex.id(),
                    img,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                let age = t.at.elapsed().as_secs();
                if age >= 5 {
                    painter.text(
                        rect.right_bottom() - egui::vec2(6.0, 4.0),
                        egui::Align2::RIGHT_BOTTOM,
                        format!("{age}s old"),
                        egui::FontId::monospace(11.0),
                        palette.warn,
                    );
                }
            } else {
                let msg = match &s.state {
                    InstanceState::Preparing => s.progress.clone().unwrap_or_else(|| "preparing…".into()),
                    _ => "waiting for the game window…".into(),
                };
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    msg,
                    egui::FontId::monospace(11.0),
                    palette.ink_muted,
                );
            }
            if resp.hovered() {
                painter.rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(1.0_f32, palette.focus),
                    egui::StrokeKind::Inside,
                );
            }
            if resp
                .on_hover_text("Click to bring this game to the front")
                .clicked()
            {
                actions.push(WallAction::Show(s.profile_id.clone()));
            }
            ui.add_space(4.0);

            let mut info = format!(
                "{} · up {}",
                s.account_name,
                format_playtime(s.elapsed_secs().max(0) as u64)
            );
            if let Some(pid) = s.pid {
                info.push_str(&format!(" · pid {pid}"));
            }
            if s.adopted {
                info.push_str(" · re-attached");
            }
            ui.label(
                RichText::new(info)
                    .monospace()
                    .size(11.0)
                    .color(palette.ink_dim),
            );
            ui.horizontal(|ui| {
                if hidden == Some(false) {
                    if theme::term_button(ui, palette, "HIDE", ButtonTone::Subtle).clicked() {
                        actions.push(WallAction::Hide(s.profile_id.clone()));
                    }
                } else if theme::term_button(ui, palette, "SHOW", ButtonTone::Focus).clicked() {
                    actions.push(WallAction::Show(s.profile_id.clone()));
                }
                if theme::term_button(ui, palette, "LOGS", ButtonTone::Secondary).clicked() {
                    actions.push(WallAction::OpenConsole(s.profile_id.clone()));
                }
            });
        });
    }

    /// Tell the capture thread what to grab, and upload any new frames as textures.
    fn sync_with_capture(&mut self, ctx: &egui::Context, live: &[&InstanceStatus]) {
        let mut sh = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        sh.visible_until = Some(Instant::now() + Duration::from_millis(1500));
        sh.targets = live
            .iter()
            .filter(|s| s.state == InstanceState::Running)
            .filter_map(|s| s.pid.map(|p| (s.profile_id.clone(), p)))
            .collect();
        let wanted: std::collections::HashSet<String> =
            sh.targets.iter().map(|(id, _)| id.clone()).collect();
        sh.frames.retain(|id, _| wanted.contains(id.as_str()));
        self.thumbs.retain(|id, _| wanted.contains(id.as_str()));
        self.hidden.retain(|id, _| wanted.contains(id.as_str()));

        for (id, cap) in sh.frames.iter_mut() {
            self.hidden.insert(id.clone(), cap.hidden);
            let Some(frame) = cap.frame.take() else { continue };
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            match self.thumbs.get_mut(id) {
                Some(t) => {
                    t.tex.set(img, egui::TextureOptions::LINEAR);
                    t.at = cap.at;
                }
                None => {
                    let tex = ctx.load_texture(format!("wall-{id}"), img, egui::TextureOptions::LINEAR);
                    self.thumbs.insert(id.clone(), Thumb { tex, at: cap.at });
                }
            }
        }
        drop(sh);
        // Keep the tiles' clocks and pictures ticking while the Wall is open.
        ctx.request_repaint_after(CAPTURE_EVERY);
    }
}

fn capture_loop(shared: Arc<Mutex<Shared>>, ctx: egui::Context) {
    loop {
        std::thread::sleep(CAPTURE_EVERY);
        let targets = {
            let sh = shared.lock().unwrap_or_else(|e| e.into_inner());
            match sh.visible_until {
                Some(t) if Instant::now() < t => sh.targets.clone(),
                _ => continue,
            }
        };
        if targets.is_empty() {
            continue;
        }
        let mut got = Vec::with_capacity(targets.len());
        for (id, pid) in targets {
            let (frame, hidden) = match window::main_window(pid) {
                Some(h) => (window::capture(h, THUMB_W), Some(window::is_hidden(h))),
                None => (None, None),
            };
            got.push((id, frame, hidden));
        }
        {
            let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            for (id, frame, hidden) in got {
                let entry = sh.frames.entry(id).or_insert(Captured {
                    frame: None,
                    hidden,
                    at: now,
                });
                entry.hidden = hidden;
                if frame.is_some() {
                    entry.frame = frame;
                    entry.at = now;
                }
            }
        }
        ctx.request_repaint();
    }
}
