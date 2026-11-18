use eframe::egui::{
    self, Color32, CornerRadius, Margin, Mesh, Pos2, Rect, Response, Sense, Shadow, Shape, Stroke,
    StrokeKind, Vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassElevation {
    /// Floating sidebar / primary navigation island
    FloatingIsland,
    /// Standard content card
    Card,
    /// Elevated / interactive panel or sheet
    Elevated,
    /// Recessed track (segmented controls, search bars, icon wells)
    Well,
    /// Deep smoked glass for live log terminal
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonTone {
    Primary,
    Secondary,
    Danger,
    Subtle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavIcon {
    Instances,
    Mods,
    Accounts,
    Logs,
    Settings,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub mode: GlassMode,

    // Glass surfaces
    pub island_fill: Color32,
    pub card_fill: Color32,
    pub elevated_fill: Color32,
    pub well_fill: Color32,
    pub terminal_fill: Color32,

    // Optical borders & specular highlights
    pub border_subtle: Color32,
    pub border_bright: Color32,
    pub specular_top: Color32,
    pub sheen_top: Color32,
    pub divider: Color32,
    pub shadow_color: Color32,

    // Interactive widget surfaces
    pub control_fill: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    pub segment_thumb_fill: Color32,
    pub segment_thumb_border: Color32,

    // Typography
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_on_accent: Color32,

    // Apple-inspired luminous system accents
    pub accent_blue: Color32,
    pub accent_cyan: Color32,
    pub accent_emerald: Color32,
    pub accent_amber: Color32,
    pub accent_red: Color32,

    // Tinted glass button fills
    pub primary_glass_fill: Color32,
    pub primary_glass_hover: Color32,
    pub primary_glass_border: Color32,
    pub danger_glass_fill: Color32,
    pub danger_glass_hover: Color32,
    pub danger_glass_border: Color32,
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    let pr = ((r as u16 * a as u16) / 255) as u8;
    let pg = ((g as u16 * a as u16) / 255) as u8;
    let pb = ((b as u16 * a as u16) / 255) as u8;
    Color32::from_rgba_premultiplied(pr, pg, pb, a)
}

impl Palette {
    pub const fn for_mode(mode: GlassMode) -> Self {
        match mode {
            GlassMode::Dark => Self {
                mode,
                island_fill: rgba(24, 30, 48, 178),
                card_fill: rgba(30, 38, 60, 148),
                elevated_fill: rgba(40, 50, 76, 176),
                well_fill: rgba(12, 16, 28, 148),
                terminal_fill: rgba(10, 13, 22, 206),

                border_subtle: rgba(255, 255, 255, 34),
                border_bright: rgba(255, 255, 255, 76),
                specular_top: rgba(255, 255, 255, 112),
                sheen_top: rgba(255, 255, 255, 20),
                divider: rgba(255, 255, 255, 26),
                shadow_color: rgba(2, 5, 14, 105),

                control_fill: rgba(255, 255, 255, 18),
                control_hover: rgba(255, 255, 255, 36),
                control_active: rgba(10, 132, 255, 72),
                segment_thumb_fill: rgba(255, 255, 255, 42),
                segment_thumb_border: rgba(255, 255, 255, 92),

                text_primary: Color32::from_rgb(245, 248, 255),
                text_secondary: rgba(235, 242, 255, 205),
                text_muted: rgba(196, 210, 238, 150),
                text_on_accent: Color32::WHITE,

                accent_blue: Color32::from_rgb(56, 162, 255),
                accent_cyan: Color32::from_rgb(90, 216, 255),
                accent_emerald: Color32::from_rgb(48, 214, 128),
                accent_amber: Color32::from_rgb(255, 198, 56),
                accent_red: Color32::from_rgb(255, 92, 92),

                primary_glass_fill: rgba(24, 144, 255, 208),
                primary_glass_hover: rgba(54, 164, 255, 232),
                primary_glass_border: rgba(176, 224, 255, 180),
                danger_glass_fill: rgba(226, 58, 66, 202),
                danger_glass_hover: rgba(244, 78, 86, 228),
                danger_glass_border: rgba(255, 178, 182, 165),
            },
            GlassMode::Light => Self {
                mode,
                island_fill: rgba(255, 255, 255, 182),
                card_fill: rgba(255, 255, 255, 156),
                elevated_fill: rgba(255, 255, 255, 205),
                well_fill: rgba(214, 226, 246, 132),
                terminal_fill: rgba(18, 23, 36, 224),

                border_subtle: rgba(255, 255, 255, 175),
                border_bright: rgba(255, 255, 255, 235),
                specular_top: rgba(255, 255, 255, 245),
                sheen_top: rgba(255, 255, 255, 76),
                divider: rgba(30, 50, 88, 26),
                shadow_color: rgba(28, 48, 92, 34),

                control_fill: rgba(255, 255, 255, 138),
                control_hover: rgba(255, 255, 255, 204),
                control_active: rgba(0, 122, 255, 44),
                segment_thumb_fill: rgba(255, 255, 255, 235),
                segment_thumb_border: rgba(255, 255, 255, 255),

                text_primary: Color32::from_rgb(20, 28, 46),
                text_secondary: Color32::from_rgb(48, 62, 88),
                text_muted: Color32::from_rgb(92, 108, 136),
                text_on_accent: Color32::WHITE,

                accent_blue: Color32::from_rgb(0, 118, 255),
                accent_cyan: Color32::from_rgb(0, 158, 214),
                accent_emerald: Color32::from_rgb(18, 166, 86),
                accent_amber: Color32::from_rgb(204, 126, 0),
                accent_red: Color32::from_rgb(220, 48, 48),

                primary_glass_fill: rgba(0, 122, 255, 222),
                primary_glass_hover: rgba(24, 138, 255, 242),
                primary_glass_border: rgba(164, 214, 255, 210),
                danger_glass_fill: rgba(226, 54, 58, 218),
                danger_glass_hover: rgba(242, 72, 76, 238),
                danger_glass_border: rgba(255, 188, 190, 200),
            },
        }
    }
}

pub fn apply_theme(ctx: &egui::Context, mode: GlassMode) {
    let p = Palette::for_mode(mode);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.window_margin = Margin::same(16);
    style.spacing.interact_size.y = 28.0;
    style.spacing.combo_height = 240.0;

    let mut visuals = match mode {
        GlassMode::Dark => egui::Visuals::dark(),
        GlassMode::Light => egui::Visuals::light(),
    };

    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = p.elevated_fill;
    visuals.extreme_bg_color = p.well_fill;
    visuals.faint_bg_color = p.island_fill;
    visuals.window_corner_radius = CornerRadius::same(18);
    visuals.window_stroke = Stroke::new(1.0_f32, p.border_bright);
    visuals.window_shadow = Shadow {
        offset: [0, 12],
        blur: 28,
        spread: 0,
        color: p.shadow_color,
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 8],
        blur: 22,
        spread: 0,
        color: p.shadow_color,
    };
    visuals.hyperlink_color = p.accent_blue;

    visuals.widgets.noninteractive.bg_fill = p.card_fill;
    visuals.widgets.noninteractive.weak_bg_fill = p.well_fill;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(14);

    visuals.widgets.inactive.bg_fill = p.control_fill;
    visuals.widgets.inactive.weak_bg_fill = p.control_fill;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(14);

    visuals.widgets.hovered.bg_fill = p.control_hover;
    visuals.widgets.hovered.weak_bg_fill = p.control_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_bright);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(14);

    visuals.widgets.active.bg_fill = p.control_active;
    visuals.widgets.active.weak_bg_fill = p.control_active;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent_blue);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.active.corner_radius = CornerRadius::same(14);

    visuals.widgets.open.bg_fill = p.elevated_fill;
    visuals.widgets.open.weak_bg_fill = p.elevated_fill;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.border_bright);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.open.corner_radius = CornerRadius::same(14);

    visuals.selection.bg_fill = rgba(p.accent_blue.r(), p.accent_blue.g(), p.accent_blue.b(), 64);
    visuals.selection.stroke = Stroke::new(1.0_f32, p.accent_blue);

    style.visuals = visuals;
    ctx.set_style(style);
}

/// Paints a lush, Apple macOS Tahoe-inspired atmospheric Liquid Glass wallpaper
/// with a bilinear aurora mesh and soft caustic light orbs behind the frosted surfaces.
pub fn paint_liquid_backdrop(ctx: &egui::Context, mode: GlassMode) {
    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    let colors = match mode {
        GlassMode::Dark => [
            [
                Color32::from_rgb(10, 22, 46),
                Color32::from_rgb(12, 18, 42),
                Color32::from_rgb(18, 28, 62),
            ],
            [
                Color32::from_rgb(8, 17, 34),
                Color32::from_rgb(14, 18, 38),
                Color32::from_rgb(24, 18, 52),
            ],
            [
                Color32::from_rgb(7, 13, 24),
                Color32::from_rgb(16, 22, 44),
                Color32::from_rgb(10, 26, 40),
            ],
        ],
        GlassMode::Light => [
            [
                Color32::from_rgb(214, 232, 255),
                Color32::from_rgb(230, 238, 255),
                Color32::from_rgb(232, 222, 255),
            ],
            [
                Color32::from_rgb(218, 242, 252),
                Color32::from_rgb(238, 244, 255),
                Color32::from_rgb(244, 230, 252),
            ],
            [
                Color32::from_rgb(220, 244, 242),
                Color32::from_rgb(232, 240, 255),
                Color32::from_rgb(218, 236, 255),
            ],
        ],
    };

    let xs = [rect.left(), rect.center().x, rect.right()];
    let ys = [rect.top(), rect.center().y, rect.bottom()];

    let mut mesh = Mesh::default();
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
    painter.add(Shape::mesh(mesh));

    // Soft caustic ambient light orbs (concentric radial falloff, zero extra repaints)
    struct CausticOrb {
        fx: f32,
        fy: f32,
        fr: f32,
        rgb: (u8, u8, u8),
        peak_alpha: u8,
    }

    let w = rect.width();
    let h = rect.height();
    let orbs: &[CausticOrb] = match mode {
        GlassMode::Dark => &[
            CausticOrb { fx: 0.14, fy: 0.16, fr: 0.34, rgb: (40, 175, 255), peak_alpha: 34 },
            CausticOrb { fx: 0.84, fy: 0.20, fr: 0.38, rgb: (142, 92, 255), peak_alpha: 32 },
            CausticOrb { fx: 0.76, fy: 0.84, fr: 0.36, rgb: (36, 214, 152), peak_alpha: 28 },
            CausticOrb { fx: 0.24, fy: 0.82, fr: 0.30, rgb: (48, 128, 255), peak_alpha: 24 },
        ],
        GlassMode::Light => &[
            CausticOrb { fx: 0.14, fy: 0.16, fr: 0.38, rgb: (112, 198, 255), peak_alpha: 62 },
            CausticOrb { fx: 0.84, fy: 0.20, fr: 0.40, rgb: (192, 152, 255), peak_alpha: 54 },
            CausticOrb { fx: 0.76, fy: 0.84, fr: 0.38, rgb: (112, 232, 192), peak_alpha: 54 },
            CausticOrb { fx: 0.24, fy: 0.82, fr: 0.32, rgb: (132, 184, 255), peak_alpha: 48 },
        ],
    };

    let min_dim = w.min(h).max(400.0);
    for orb in orbs {
        let (r, g, b) = orb.rgb;
        let center = Pos2::new(rect.left() + w * orb.fx, rect.top() + h * orb.fy);
        let max_radius = min_dim * orb.fr;
        const RINGS: usize = 6;
        for step in 0..RINGS {
            let t = (step + 1) as f32 / RINGS as f32;
            let radius = max_radius * t;
            let falloff = (1.0 - t * 0.85).powi(2);
            let alpha = ((orb.peak_alpha as f32 * falloff) / 1.4).round().clamp(1.0, 255.0) as u8;
            painter.circle_filled(center, radius, rgba(r, g, b, alpha));
        }
    }
}

/// Renders a true multi-layered Apple Liquid Glass container:
/// ambient drop shadow + frosted translucent body + subtle top optical sheen +
/// refractive inner border + crisp specular top catchlight.
pub fn glass_box<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    elevation: GlassElevation,
    radius: u8,
    margin: Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let bg_idx = ui.painter().add(Shape::Noop);

    let inner = egui::Frame::NONE
        .inner_margin(margin)
        .show(ui, add_contents);

    let rect = inner.response.rect;
    let cr = CornerRadius::same(radius);
    let r_f32 = radius as f32;

    let (fill, border, shadow_blur, shadow_y, draw_sheen) = match elevation {
        GlassElevation::FloatingIsland => (palette.island_fill, palette.border_bright, 28, 10, true),
        GlassElevation::Card => (palette.card_fill, palette.border_subtle, 18, 6, true),
        GlassElevation::Elevated => (palette.elevated_fill, palette.border_bright, 24, 8, true),
        GlassElevation::Well => (palette.well_fill, palette.border_subtle, 0, 0, false),
        GlassElevation::Terminal => (palette.terminal_fill, palette.border_subtle, 20, 8, true),
    };

    let mut shapes = Vec::with_capacity(5);

    // 1. Soft ambient drop shadow
    if shadow_blur > 0 {
        let shadow = Shadow {
            offset: [0, shadow_y],
            blur: shadow_blur,
            spread: 0,
            color: palette.shadow_color,
        };
        shapes.push(Shape::Rect(shadow.as_shape(rect, cr)));
    }

    // 2. Frosted translucent body
    shapes.push(Shape::rect_filled(rect, cr, fill));

    // 3. Subtle top optical sheen (simulates curved glass catching overhead light)
    if draw_sheen && rect.height() > 24.0 && rect.width() > 24.0 {
        let sheen_h = (rect.height() * 0.36).clamp(14.0, 44.0);
        let inset = 1.5_f32;
        let top_rect = Rect::from_min_max(
            Pos2::new(rect.left() + inset, rect.top() + inset),
            Pos2::new(rect.right() - inset, (rect.top() + sheen_h).min(rect.bottom() - inset)),
        );
        let mut sheen = Mesh::default();
        let top_col = palette.sheen_top;
        let bot_col = Color32::TRANSPARENT;
        // Inset horizontally by a fraction of corner radius so sheen sits cleanly inside rounded corners
        let corner_pad = (r_f32 * 0.45).min(top_rect.width() * 0.25);
        sheen.colored_vertex(Pos2::new(top_rect.left() + corner_pad, top_rect.top()), top_col);
        sheen.colored_vertex(Pos2::new(top_rect.right() - corner_pad, top_rect.top()), top_col);
        sheen.colored_vertex(Pos2::new(top_rect.left(), top_rect.bottom()), bot_col);
        sheen.colored_vertex(Pos2::new(top_rect.right(), top_rect.bottom()), bot_col);
        sheen.add_triangle(0, 1, 3);
        sheen.add_triangle(0, 3, 2);
        shapes.push(Shape::mesh(sheen));
    }

    // 4. Refractive hairline perimeter border
    shapes.push(Shape::rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    ));

    // 5. Crisp specular top catchlight along the top edge
    if draw_sheen && rect.width() > r_f32 * 2.0 + 12.0 {
        let y = rect.top() + 1.0;
        let x0 = rect.left() + r_f32.max(10.0);
        let x1 = rect.right() - r_f32.max(10.0);
        shapes.push(Shape::line_segment(
            [Pos2::new(x0, y), Pos2::new(x1, y)],
            Stroke::new(1.0_f32, palette.specular_top),
        ));
    }

    ui.painter().set(bg_idx, Shape::Vec(shapes));
    inner
}

pub fn glass_card<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    glass_box(
        ui,
        palette,
        GlassElevation::Card,
        18,
        Margin::same(16),
        add_contents,
    )
}

pub fn glass_elevated<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    glass_box(
        ui,
        palette,
        GlassElevation::Elevated,
        20,
        Margin::same(18),
        add_contents,
    )
}

pub fn glass_toolbar<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    glass_box(
        ui,
        palette,
        GlassElevation::Card,
        18,
        Margin::symmetric(14, 10),
        add_contents,
    )
}

pub fn glass_rail<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    glass_box(
        ui,
        palette,
        GlassElevation::FloatingIsland,
        20,
        Margin::same(16),
        add_contents,
    )
}

pub fn glass_terminal<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    glass_box(
        ui,
        palette,
        GlassElevation::Terminal,
        18,
        Margin::same(0),
        add_contents,
    )
}

/// Apple Liquid Glass capsule button with specular rim and tone presets.
pub fn pill_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    tone: ButtonTone,
) -> Response {
    let (fill, hover_fill, stroke_col, text_col) = match tone {
        ButtonTone::Primary => (
            palette.primary_glass_fill,
            palette.primary_glass_hover,
            palette.primary_glass_border,
            palette.text_on_accent,
        ),
        ButtonTone::Danger => (
            palette.danger_glass_fill,
            palette.danger_glass_hover,
            palette.danger_glass_border,
            palette.text_on_accent,
        ),
        ButtonTone::Secondary => (
            palette.control_fill,
            palette.control_hover,
            palette.border_bright,
            palette.text_primary,
        ),
        ButtonTone::Subtle => (
            Color32::TRANSPARENT,
            palette.control_fill,
            palette.border_subtle,
            palette.text_secondary,
        ),
    };

    let font_id = egui::FontId::proportional(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id, text_col);
    let desired = Vec2::new((galley.size().x + 28.0).max(64.0), 32.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let cr = CornerRadius::same((rect.height() * 0.5) as u8);
        let bg = if resp.is_pointer_button_down_on() || resp.hovered() {
            hover_fill
        } else {
            fill
        };

        let p = ui.painter();
        if matches!(tone, ButtonTone::Primary | ButtonTone::Danger) {
            let glow = Shadow {
                offset: [0, 3],
                blur: 12,
                spread: 0,
                color: rgba(fill.r(), fill.g(), fill.b(), 64),
            };
            p.add(glow.as_shape(rect, cr));
        }

        p.rect_filled(rect, cr, bg);
        p.rect_stroke(rect, cr, Stroke::new(1.0_f32, stroke_col), StrokeKind::Inside);

        // Specular top catchlight on capsule
        if bg != Color32::TRANSPARENT {
            let pad = rect.height() * 0.45;
            p.line_segment(
                [
                    Pos2::new(rect.left() + pad, rect.top() + 1.0),
                    Pos2::new(rect.right() - pad, rect.top() + 1.0),
                ],
                Stroke::new(1.0_f32, rgba(255, 255, 255, 110)),
            );
        }

        let text_pos = Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        );
        p.galley(text_pos, galley, text_col);
    }

    resp
}

/// Full-width sidebar capsule button with painted vector icon and optional count badge.
pub fn nav_pill(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: NavIcon,
    label: &str,
    badge_count: Option<usize>,
    selected: bool,
) -> Response {
    let desired = Vec2::new(ui.available_width(), 38.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let cr = CornerRadius::same(14);
        let p = ui.painter();

        if selected {
            let fill = match palette.mode {
                GlassMode::Dark => rgba(255, 255, 255, 34),
                GlassMode::Light => rgba(255, 255, 255, 224),
            };
            let shadow = Shadow {
                offset: [0, 3],
                blur: 10,
                spread: 0,
                color: palette.shadow_color,
            };
            p.add(shadow.as_shape(rect, cr));
            p.rect_filled(rect, cr, fill);
            p.rect_stroke(
                rect,
                cr,
                Stroke::new(1.0_f32, palette.border_bright),
                StrokeKind::Inside,
            );
            // Specular highlight along top of selected pill
            p.line_segment(
                [
                    Pos2::new(rect.left() + 14.0, rect.top() + 1.0),
                    Pos2::new(rect.right() - 14.0, rect.top() + 1.0),
                ],
                Stroke::new(1.0_f32, palette.specular_top),
            );
        } else if resp.hovered() {
            p.rect_filled(rect, cr, palette.control_fill);
            p.rect_stroke(
                rect,
                cr,
                Stroke::new(1.0_f32, palette.border_subtle),
                StrokeKind::Inside,
            );
        }

        let icon_color = if selected {
            palette.accent_blue
        } else {
            palette.text_muted
        };
        let text_color = if selected {
            palette.text_primary
        } else {
            palette.text_secondary
        };

        let icon_center = Pos2::new(rect.left() + 20.0, rect.center().y);
        paint_nav_icon(p, icon_center, icon, icon_color);

        let galley = p.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(13.5),
            text_color,
        );
        p.galley(
            Pos2::new(rect.left() + 36.0, rect.center().y - galley.size().y * 0.5),
            galley,
            text_color,
        );

        if let Some(count) = badge_count {
            let count_str = count.to_string();
            let badge_galley = p.layout_no_wrap(
                count_str,
                egui::FontId::proportional(11.0),
                if selected {
                    palette.accent_blue
                } else {
                    palette.text_muted
                },
            );
            let bw = (badge_galley.size().x + 14.0).max(24.0);
            let bh = 20.0;
            let brect = Rect::from_min_size(
                Pos2::new(rect.right() - bw - 10.0, rect.center().y - bh * 0.5),
                Vec2::new(bw, bh),
            );
            let badge_fill = if selected {
                rgba(palette.accent_blue.r(), palette.accent_blue.g(), palette.accent_blue.b(), 36)
            } else {
                palette.well_fill
            };
            p.rect_filled(brect, CornerRadius::same(10), badge_fill);
            p.galley(
                Pos2::new(
                    brect.center().x - badge_galley.size().x * 0.5,
                    brect.center().y - badge_galley.size().y * 0.5,
                ),
                badge_galley,
                if selected {
                    palette.accent_blue
                } else {
                    palette.text_muted
                },
            );
        }
    }

    resp
}

/// Apple Liquid Glass segmented control inside a recessed frosted capsule track.
pub fn segmented_control<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    palette: &Palette,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    glass_box(
        ui,
        palette,
        GlassElevation::Well,
        16,
        Margin::same(3),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &(value, label) in options {
                    let selected = *current == value;
                    let text_col = if selected {
                        palette.text_primary
                    } else {
                        palette.text_muted
                    };
                    let galley = ui.painter().layout_no_wrap(
                        label.to_string(),
                        egui::FontId::proportional(12.5),
                        text_col,
                    );
                    let size = Vec2::new(galley.size().x + 22.0, 26.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    if ui.is_rect_visible(rect) {
                        let cr = CornerRadius::same(13);
                        let p = ui.painter();
                        if selected {
                            p.rect_filled(rect, cr, palette.segment_thumb_fill);
                            p.rect_stroke(
                                rect,
                                cr,
                                Stroke::new(1.0_f32, palette.segment_thumb_border),
                                StrokeKind::Inside,
                            );
                        } else if resp.hovered() {
                            p.rect_filled(rect, cr, palette.control_fill);
                        }
                        p.galley(
                            Pos2::new(
                                rect.center().x - galley.size().x * 0.5,
                                rect.center().y - galley.size().y * 0.5,
                            ),
                            galley,
                            text_col,
                        );
                    }
                    if resp.clicked() && !selected {
                        *current = value;
                        changed = true;
                    }
                }
            });
        },
    );
    changed
}

/// Recessed frosted capsule search field with painted magnifying glass and inline clear button.
/// Returns `(response, cleared_clicked, enter_pressed)`.
pub fn glass_search_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    query: &mut String,
    hint: &str,
    width: f32,
) -> (Response, bool, bool) {
    let mut cleared = false;
    let mut enter_pressed = false;
    let mut edit_resp: Option<Response> = None;

    glass_box(
        ui,
        palette,
        GlassElevation::Well,
        16,
        Margin::symmetric(10, 4),
        |ui| {
            ui.set_width((width - 20.0).max(120.0));
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 20.0), Sense::hover());
                paint_search_icon(ui.painter(), icon_rect.center(), palette.text_muted);

                let text_w = (ui.available_width() - if query.is_empty() { 4.0 } else { 24.0 }).max(80.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text(hint)
                        .frame(false)
                        .desired_width(text_w),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    enter_pressed = true;
                }
                edit_resp = Some(resp);

                if !query.is_empty() {
                    let (x_rect, x_resp) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::click());
                    if ui.is_rect_visible(x_rect) {
                        let c = x_rect.center();
                        let bg = if x_resp.hovered() {
                            palette.control_hover
                        } else {
                            palette.control_fill
                        };
                        ui.painter().circle_filled(c, 8.0, bg);
                        let s = 3.2;
                        let stroke = Stroke::new(1.3_f32, palette.text_secondary);
                        ui.painter()
                            .line_segment([Pos2::new(c.x - s, c.y - s), Pos2::new(c.x + s, c.y + s)], stroke);
                        ui.painter()
                            .line_segment([Pos2::new(c.x + s, c.y - s), Pos2::new(c.x - s, c.y + s)], stroke);
                    }
                    if x_resp.clicked() {
                        query.clear();
                        cleared = true;
                    }
                }
            });
        },
    );

    (
        edit_resp.expect("search text edit rendered"),
        cleared,
        enter_pressed,
    )
}

/// Frosted capsule pill badge with subtle refractive border.
pub fn pill_badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let bg = rgba(color.r(), color.g(), color.b(), 32);
    let stroke = Stroke::new(1.0_f32, rgba(color.r(), color.g(), color.b(), 105));
    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).strong().color(color));
        });
}

/// Frosted status pill with a glowing live indicator dot.
pub fn status_pill(ui: &mut egui::Ui, text: &str, color: Color32, glowing: bool) {
    let bg = rgba(color.r(), color.g(), color.b(), 28);
    let stroke = Stroke::new(1.0_f32, rgba(color.r(), color.g(), color.b(), 95));
    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                status_dot(ui, color, glowing);
                ui.label(egui::RichText::new(text).size(11.0).strong().color(color));
            });
        });
}

/// Painted status dot with optional soft ambient halo.
pub fn status_dot(ui: &mut egui::Ui, color: Color32, glowing: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
    let c = rect.center();
    if glowing {
        ui.painter()
            .circle_filled(c, 5.5, rgba(color.r(), color.g(), color.b(), 58));
    }
    ui.painter().circle_filled(c, 3.4, color);
}

/// Hairline glass divider.
pub fn glass_divider(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette.divider);
    ui.add_space(4.0);
}

/// Uppercase tracked micro-header (Apple HIG section label).
pub fn section_eyebrow(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(10.5)
            .strong()
            .color(palette.text_muted),
    );
}

/// Paints a macOS-style squircle glass app emblem for ModsUndBots.
pub fn paint_app_emblem(ui: &mut egui::Ui, palette: &Palette, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let cr = CornerRadius::same((size * 0.28) as u8);

    // Subtle colored drop glow
    let glow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: rgba(palette.accent_blue.r(), palette.accent_blue.g(), palette.accent_blue.b(), 80),
    };
    p.add(glow.as_shape(rect, cr));

    // Gradient base squircle
    p.rect_filled(rect, cr, rgba(22, 134, 255, 225));
    // Lower-right mint caustic highlight inside squircle
    let inner_orb = Pos2::new(rect.right() - size * 0.25, rect.bottom() - size * 0.25);
    p.circle_filled(inner_orb, size * 0.36, rgba(48, 224, 152, 145));
    // Specular glass border
    p.rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, rgba(255, 255, 255, 165)),
        StrokeKind::Inside,
    );

    // Isometric cube wireframe motif (Minecraft + Bots)
    let c = rect.center();
    let r = size * 0.26;
    let top = Pos2::new(c.x, c.y - r);
    let tr = Pos2::new(c.x + r * 0.88, c.y - r * 0.45);
    let br = Pos2::new(c.x + r * 0.88, c.y + r * 0.45);
    let bot = Pos2::new(c.x, c.y + r);
    let bl = Pos2::new(c.x - r * 0.88, c.y + r * 0.45);
    let tl = Pos2::new(c.x - r * 0.88, c.y - r * 0.45);

    let stroke = Stroke::new(1.6_f32, Color32::WHITE);
    p.add(Shape::closed_line(vec![top, tr, br, bot, bl, tl], stroke));
    p.line_segment([tl, c], stroke);
    p.line_segment([tr, c], stroke);
    p.line_segment([c, bot], stroke);
}

/// Paints a circular frosted avatar emblem with initial(s) for profiles & accounts.
pub fn paint_avatar_badge(ui: &mut egui::Ui, label: &str, accent: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let c = rect.center();
    let r = size * 0.5;
    p.circle_filled(c, r, rgba(accent.r(), accent.g(), accent.b(), 38));
    p.circle_stroke(c, r - 0.5, Stroke::new(1.0_f32, rgba(accent.r(), accent.g(), accent.b(), 120)));

    let ch = label
        .chars()
        .next()
        .unwrap_or('?')
        .to_ascii_uppercase()
        .to_string();
    let galley = p.layout_no_wrap(ch, egui::FontId::proportional(size * 0.44), accent);
    p.galley(
        Pos2::new(c.x - galley.size().x * 0.5, c.y - galley.size().y * 0.5),
        galley,
        accent,
    );
}

fn paint_search_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    let lens_c = Pos2::new(c.x - 1.5, c.y - 1.5);
    let stroke = Stroke::new(1.4_f32, color);
    p.circle_stroke(lens_c, 4.5, stroke);
    p.line_segment(
        [Pos2::new(lens_c.x + 3.2, lens_c.y + 3.2), Pos2::new(c.x + 5.5, c.y + 5.5)],
        stroke,
    );
}

fn paint_nav_icon(p: &egui::Painter, c: Pos2, icon: NavIcon, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    match icon {
        NavIcon::Instances => {
            let s = 4.5;
            let gap = 1.6;
            let offsets = [
                (-s - gap * 0.5, -s - gap * 0.5),
                (gap * 0.5, -s - gap * 0.5),
                (-s - gap * 0.5, gap * 0.5),
                (gap * 0.5, gap * 0.5),
            ];
            for (dx, dy) in offsets {
                let r = Rect::from_min_size(Pos2::new(c.x + dx, c.y + dy), Vec2::splat(s));
                p.rect_stroke(r, CornerRadius::same(1), stroke, StrokeKind::Middle);
            }
        }
        NavIcon::Mods => {
            let r = 6.2;
            let top = Pos2::new(c.x, c.y - r);
            let right = Pos2::new(c.x + r * 0.88, c.y - r * 0.42);
            let br = Pos2::new(c.x + r * 0.88, c.y + r * 0.42);
            let bot = Pos2::new(c.x, c.y + r);
            let bl = Pos2::new(c.x - r * 0.88, c.y + r * 0.42);
            let left = Pos2::new(c.x - r * 0.88, c.y - r * 0.42);
            p.add(Shape::closed_line(vec![top, right, br, bot, bl, left], stroke));
            p.line_segment([left, c], stroke);
            p.line_segment([right, c], stroke);
            p.line_segment([c, bot], stroke);
        }
        NavIcon::Accounts => {
            p.circle_stroke(Pos2::new(c.x, c.y - 3.0), 3.0, stroke);
            let shoulders = Rect::from_min_max(
                Pos2::new(c.x - 5.5, c.y + 1.5),
                Pos2::new(c.x + 5.5, c.y + 6.5),
            );
            p.rect_stroke(shoulders, CornerRadius::same(3), stroke, StrokeKind::Middle);
        }
        NavIcon::Logs => {
            let box_r = Rect::from_center_size(c, Vec2::new(13.0, 10.5));
            p.rect_stroke(box_r, CornerRadius::same(2), stroke, StrokeKind::Middle);
            p.line_segment(
                [Pos2::new(c.x - 3.6, c.y - 2.0), Pos2::new(c.x - 1.2, c.y)],
                stroke,
            );
            p.line_segment(
                [Pos2::new(c.x - 1.2, c.y), Pos2::new(c.x - 3.6, c.y + 2.0)],
                stroke,
            );
            p.line_segment(
                [Pos2::new(c.x + 0.5, c.y + 2.0), Pos2::new(c.x + 3.8, c.y + 2.0)],
                stroke,
            );
        }
        NavIcon::Settings => {
            // Three horizontal slider tracks with knobs (clean macOS Control Center motif)
            for (dy, kx) in [(-3.8_f32, -2.0_f32), (0.0, 2.2), (3.8, -1.0)] {
                let y = c.y + dy;
                p.line_segment([Pos2::new(c.x - 6.0, y), Pos2::new(c.x + 6.0, y)], stroke);
                p.circle_filled(Pos2::new(c.x + kx, y), 2.1, color);
            }
        }
    }
}
