use eframe::egui::{self, Color32, CornerRadius, Margin, Pos2, Rect, Stroke, Vec2};

const fn glass_rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    let pr = ((r as u16 * a as u16) / 255) as u8;
    let pg = ((g as u16 * a as u16) / 255) as u8;
    let pb = ((b as u16 * a as u16) / 255) as u8;
    Color32::from_rgba_premultiplied(pr, pg, pb, a)
}

// Translucent glass layers
pub const GLASS_SIDEBAR: Color32 = glass_rgba(18, 24, 38, 195);
pub const GLASS_CARD: Color32 = glass_rgba(26, 34, 52, 165);
pub const GLASS_CARD_ELEVATED: Color32 = glass_rgba(36, 46, 70, 185);
pub const GLASS_WELL: Color32 = glass_rgba(12, 16, 26, 180);
pub const BG_TERMINAL: Color32 = glass_rgba(10, 13, 21, 210);

// Specular glass borders
pub const GLASS_BORDER: Color32 = glass_rgba(255, 255, 255, 32);
pub const GLASS_BORDER_BRIGHT: Color32 = glass_rgba(255, 255, 255, 68);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(242, 246, 255);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(156, 170, 196);

// Luminous accents
pub const ACCENT_EMERALD: Color32 = Color32::from_rgb(45, 212, 160);
pub const ACCENT_EMERALD_GLASS: Color32 = glass_rgba(16, 165, 120, 200);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(96, 165, 250);
pub const ACCENT_AMBER: Color32 = Color32::from_rgb(251, 191, 36);
pub const ACCENT_RED: Color32 = Color32::from_rgb(248, 113, 113);
pub const ACCENT_RED_GLASS: Color32 = glass_rgba(190, 48, 58, 200);

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.window_margin = Margin::same(16);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = GLASS_CARD_ELEVATED;
    visuals.extreme_bg_color = GLASS_WELL;
    visuals.faint_bg_color = GLASS_SIDEBAR;
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.window_stroke = Stroke::new(1.0_f32, GLASS_BORDER_BRIGHT);

    visuals.widgets.noninteractive.bg_fill = GLASS_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, GLASS_BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 16);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, GLASS_BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(10);

    visuals.widgets.hovered.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 32);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, GLASS_BORDER_BRIGHT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(10);

    visuals.widgets.active.bg_fill = Color32::from_rgba_unmultiplied(45, 212, 160, 64);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_EMERALD);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(10);

    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(45, 212, 160, 56);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT_EMERALD);

    style.visuals = visuals;
    ctx.set_style(style);
}

/// Paints a silky-smooth macOS-style vertex-interpolated aurora mesh behind the glass panels.
pub fn paint_liquid_backdrop(ctx: &egui::Context) {
    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    let c_tl = Color32::from_rgb(10, 26, 30); // soft emerald-tinted obsidian
    let c_tc = Color32::from_rgb(11, 16, 32); // deep slate
    let c_tr = Color32::from_rgb(14, 24, 52); // soft sapphire glow
    let c_ml = Color32::from_rgb(9, 14, 24);
    let c_mc = Color32::from_rgb(12, 16, 30);
    let c_mr = Color32::from_rgb(16, 20, 44);
    let c_bl = Color32::from_rgb(8, 11, 19);
    let c_bc = Color32::from_rgb(20, 14, 38);  // subtle violet glow
    let c_br = Color32::from_rgb(9, 12, 22);

    let xs = [rect.left(), rect.center().x, rect.right()];
    let ys = [rect.top(), rect.center().y, rect.bottom()];
    let colors = [
        [c_tl, c_tc, c_tr],
        [c_ml, c_mc, c_mr],
        [c_bl, c_bc, c_br],
    ];

    let mut mesh = egui::Mesh::default();
    for (iy, &y) in ys.iter().enumerate() {
        for (ix, &x) in xs.iter().enumerate() {
            mesh.colored_vertex(Pos2::new(x, y), colors[iy][ix]);
        }
    }
    for iy in 0..2_u32 {
        for ix in 0..2_u32 {
            let tl = iy * 3 + ix;
            let tr = tl + 1;
            let bl = tl + 3;
            let br = bl + 1;
            mesh.add_triangle(tl, tr, br);
            mesh.add_triangle(tl, br, bl);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(GLASS_CARD)
        .stroke(Stroke::new(1.0_f32, GLASS_BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
}

pub fn glass_rail_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(GLASS_SIDEBAR)
        .stroke(Stroke::new(1.0_f32, GLASS_BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
}

pub fn pill_badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30);
    let stroke = Stroke::new(
        1.0_f32,
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 100),
    );
    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).strong().color(color));
        });
}

pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).strong().color(Color32::WHITE))
        .fill(ACCENT_EMERALD_GLASS)
        .stroke(Stroke::new(1.0_f32, ACCENT_EMERALD))
        .corner_radius(CornerRadius::same(12))
}

pub fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).strong().color(Color32::WHITE))
        .fill(ACCENT_RED_GLASS)
        .stroke(Stroke::new(1.0_f32, ACCENT_RED))
        .corner_radius(CornerRadius::same(12))
}

/// Painted status dot (avoids missing-glyph tofu boxes in default fonts).
pub fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0_f32, color);
}

/// Subtle specular top highlight line for liquid-glass headers.
pub fn glass_divider(ui: &mut egui::Ui) {
    let rect = Rect::from_min_size(
        ui.cursor().min,
        Vec2::new(ui.available_width(), 1.0_f32),
    );
    ui.painter().rect_filled(rect, 0.0_f32, GLASS_BORDER);
    ui.add_space(6.0);
}
