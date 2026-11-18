use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Id, Margin, Mesh, Pos2, Rect, Response, Sense, Shadow,
    Shape, Stroke, StrokeKind, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode, Vec2,
    epaint::RectShape,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassFinish {
    /// Unpolished / satin etched frosted glass with milky diffusion and subtle micro-grain
    Frosted,
    /// Clear / polished refractive liquid glass with crisp specular catchlights
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassElevation {
    /// Floating sidebar / primary navigation island (unpolished satin frost)
    FloatingIsland,
    /// Standard content card (unpolished frosted matte glass)
    Card,
    /// Floating toolbar / filter bar (clear polished liquid glass)
    Toolbar,
    /// Elevated / interactive sheet or toast (clear polished liquid glass)
    Elevated,
    /// Recessed track (segmented controls, search bars, icon wells)
    Well,
    /// Deep smoked frosted glass for live log terminal
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

    // Unpolished / frosted matte glass surfaces (cloudier, satin finish)
    pub island_fill: Color32,
    pub card_fill: Color32,
    pub well_fill: Color32,
    pub terminal_fill: Color32,
    pub frost_grain_tint: Color32,
    pub frost_haze_top: Color32,

    // Clear / polished liquid glass surfaces (more transparent, high specular)
    pub toolbar_fill: Color32,
    pub elevated_fill: Color32,

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

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Color32::from_rgba_premultiplied(
        (a.r() as f32 * inv + b.r() as f32 * t).round() as u8,
        (a.g() as f32 * inv + b.g() as f32 * t).round() as u8,
        (a.b() as f32 * inv + b.b() as f32 * t).round() as u8,
        (a.a() as f32 * inv + b.a() as f32 * t).round() as u8,
    )
}

impl Palette {
    pub const fn for_mode(mode: GlassMode) -> Self {
        match mode {
            GlassMode::Dark => Self {
                mode,
                // Unpolished / frosted matte surfaces (milky, diffused, less clear)
                island_fill: rgba(34, 42, 62, 212),
                card_fill: rgba(36, 44, 66, 188),
                well_fill: rgba(14, 18, 30, 168),
                terminal_fill: rgba(12, 15, 25, 224),
                frost_grain_tint: rgba(230, 240, 255, 13),
                frost_haze_top: rgba(220, 232, 255, 14),

                // Clear / polished liquid glass surfaces (crisper translucency)
                toolbar_fill: rgba(28, 36, 56, 132),
                elevated_fill: rgba(40, 50, 76, 168),

                border_subtle: rgba(255, 255, 255, 32),
                border_bright: rgba(255, 255, 255, 84),
                specular_top: rgba(255, 255, 255, 128),
                sheen_top: rgba(255, 255, 255, 24),
                divider: rgba(255, 255, 255, 26),
                shadow_color: rgba(2, 5, 14, 108),

                control_fill: rgba(255, 255, 255, 18),
                control_hover: rgba(255, 255, 255, 38),
                control_active: rgba(10, 132, 255, 72),
                segment_thumb_fill: rgba(255, 255, 255, 44),
                segment_thumb_border: rgba(255, 255, 255, 108),

                text_primary: Color32::from_rgb(245, 248, 255),
                text_secondary: rgba(235, 242, 255, 208),
                text_muted: rgba(196, 210, 238, 154),
                text_on_accent: Color32::WHITE,

                accent_blue: Color32::from_rgb(56, 162, 255),
                accent_cyan: Color32::from_rgb(90, 216, 255),
                accent_emerald: Color32::from_rgb(48, 214, 128),
                accent_amber: Color32::from_rgb(255, 198, 56),
                accent_red: Color32::from_rgb(255, 92, 92),

                primary_glass_fill: rgba(24, 144, 255, 212),
                primary_glass_hover: rgba(56, 166, 255, 238),
                primary_glass_border: rgba(180, 226, 255, 188),
                danger_glass_fill: rgba(226, 58, 66, 206),
                danger_glass_hover: rgba(244, 78, 86, 232),
                danger_glass_border: rgba(255, 178, 182, 170),
            },
            GlassMode::Light => Self {
                mode,
                // Unpolished / frosted matte surfaces (milky sandblasted daylight glass)
                island_fill: rgba(246, 250, 255, 214),
                card_fill: rgba(248, 251, 255, 196),
                well_fill: rgba(206, 220, 242, 148),
                terminal_fill: rgba(18, 23, 36, 230),
                frost_grain_tint: rgba(80, 105, 145, 14),
                frost_haze_top: rgba(255, 255, 255, 58),

                // Clear / polished liquid glass surfaces
                toolbar_fill: rgba(255, 255, 255, 138),
                elevated_fill: rgba(255, 255, 255, 186),

                border_subtle: rgba(255, 255, 255, 185),
                border_bright: rgba(255, 255, 255, 245),
                specular_top: rgba(255, 255, 255, 252),
                sheen_top: rgba(255, 255, 255, 84),
                divider: rgba(30, 50, 88, 26),
                shadow_color: rgba(28, 48, 92, 34),

                control_fill: rgba(255, 255, 255, 142),
                control_hover: rgba(255, 255, 255, 212),
                control_active: rgba(0, 122, 255, 44),
                segment_thumb_fill: rgba(255, 255, 255, 238),
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

                primary_glass_fill: rgba(0, 122, 255, 224),
                primary_glass_hover: rgba(24, 138, 255, 244),
                primary_glass_border: rgba(164, 214, 255, 215),
                danger_glass_fill: rgba(226, 54, 58, 218),
                danger_glass_hover: rgba(242, 72, 76, 240),
                danger_glass_border: rgba(255, 188, 190, 200),
            },
        }
    }
}

/// Lazily creates and caches a seamless 64x64 procedural micro-frost grain texture
/// used on unpolished / matte frosted glass surfaces.
fn frost_grain_texture(ctx: &egui::Context) -> TextureHandle {
    let key = Id::new("mub_frost_grain_tex_v1");
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(key)) {
        return tex;
    }

    const N: usize = 64;
    let mut pixels = vec![Color32::TRANSPARENT; N * N];
    for y in 0..N {
        for x in 0..N {
            // Deterministic low-discrepancy hash for fine sandblasted glass grain
            let mut h = (x as u32).wrapping_mul(374_761_393)
                ^ (y as u32).wrapping_mul(668_265_263)
                ^ 0x9E37_79B9;
            h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
            h ^= h >> 16;
            let v = (h & 0xFF) as u8;
            // Subtle high-frequency frosted speckle
            let alpha = if v > 150 { ((v - 150) as u16 * 2).min(255) as u8 } else { 0 };
            pixels[y * N + x] = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
        }
    }

    let img = ColorImage {
        size: [N, N],
        pixels,
    };
    let opts = TextureOptions {
        magnification: TextureFilter::Nearest,
        minification: TextureFilter::Nearest,
        wrap_mode: TextureWrapMode::Repeat,
        mipmap_mode: None,
    };
    let tex = ctx.load_texture("mub_frost_grain", img, opts);
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    tex
}

pub fn apply_theme(ctx: &egui::Context, mode: GlassMode) {
    let p = Palette::for_mode(mode);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.window_margin = Margin::same(16);
    style.spacing.interact_size.y = 28.0;
    style.spacing.combo_height = 240.0;
    style.animation_time = 0.22;

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
            CausticOrb { fx: 0.14, fy: 0.16, fr: 0.34, rgb: (40, 175, 255), peak_alpha: 36 },
            CausticOrb { fx: 0.84, fy: 0.20, fr: 0.38, rgb: (142, 92, 255), peak_alpha: 34 },
            CausticOrb { fx: 0.76, fy: 0.84, fr: 0.36, rgb: (36, 214, 152), peak_alpha: 30 },
            CausticOrb { fx: 0.24, fy: 0.82, fr: 0.30, rgb: (48, 128, 255), peak_alpha: 26 },
        ],
        GlassMode::Light => &[
            CausticOrb { fx: 0.14, fy: 0.16, fr: 0.38, rgb: (112, 198, 255), peak_alpha: 64 },
            CausticOrb { fx: 0.84, fy: 0.20, fr: 0.40, rgb: (192, 152, 255), peak_alpha: 56 },
            CausticOrb { fx: 0.76, fy: 0.84, fr: 0.38, rgb: (112, 232, 192), peak_alpha: 56 },
            CausticOrb { fx: 0.24, fy: 0.82, fr: 0.32, rgb: (132, 184, 255), peak_alpha: 50 },
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

/// Renders a multi-layered Apple Liquid Glass container supporting both
/// **Unpolished / Frosted** satin matte glass (with micro-grain texture & soft haze)
/// and **Clear / Polished** liquid glass (with crisp specular rim highlights).
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

    let (fill, border, shadow_blur, shadow_y, finish) = match elevation {
        GlassElevation::FloatingIsland => (
            palette.island_fill,
            palette.border_bright,
            26,
            10,
            GlassFinish::Frosted,
        ),
        GlassElevation::Card => (
            palette.card_fill,
            palette.border_subtle,
            16,
            5,
            GlassFinish::Frosted,
        ),
        GlassElevation::Toolbar => (
            palette.toolbar_fill,
            palette.border_bright,
            18,
            6,
            GlassFinish::Clear,
        ),
        GlassElevation::Elevated => (
            palette.elevated_fill,
            palette.border_bright,
            24,
            8,
            GlassFinish::Clear,
        ),
        GlassElevation::Well => (
            palette.well_fill,
            palette.border_subtle,
            0,
            0,
            GlassFinish::Frosted,
        ),
        GlassElevation::Terminal => (
            palette.terminal_fill,
            palette.border_subtle,
            20,
            8,
            GlassFinish::Frosted,
        ),
    };

    let mut shapes = Vec::with_capacity(6);

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

    // 2. Translucent body
    shapes.push(Shape::rect_filled(rect, cr, fill));

    // 3. Material finish layer:
    //    - Frosted (unpolished): tiled sandblasted micro-grain + wide diffused satin haze
    //    - Clear (polished): tight overhead liquid sheen + crisp specular top catchlight
    match finish {
        GlassFinish::Frosted => {
            // Tiled micro-frost grain clipped cleanly to the rounded rect
            if rect.width() > 8.0 && rect.height() > 8.0 {
                let grain_tex = frost_grain_texture(ui.ctx());
                let uv = Rect::from_min_max(
                    Pos2::ZERO,
                    Pos2::new(rect.width() / 64.0, rect.height() / 64.0),
                );
                let grain_shape =
                    RectShape::filled(rect, cr, palette.frost_grain_tint).with_texture(grain_tex.id(), uv);
                shapes.push(Shape::Rect(grain_shape));
            }

            // Wide, diffused satin frost haze (no harsh mirror glare)
            if elevation != GlassElevation::Well && rect.height() > 28.0 && rect.width() > 28.0 {
                let haze_h = (rect.height() * 0.55).clamp(18.0, 96.0);
                let inset = 1.5_f32;
                let top_rect = Rect::from_min_max(
                    Pos2::new(rect.left() + inset, rect.top() + inset),
                    Pos2::new(
                        rect.right() - inset,
                        (rect.top() + haze_h).min(rect.bottom() - inset),
                    ),
                );
                let mut haze = Mesh::default();
                let corner_pad = (r_f32 * 0.45).min(top_rect.width() * 0.25);
                haze.colored_vertex(
                    Pos2::new(top_rect.left() + corner_pad, top_rect.top()),
                    palette.frost_haze_top,
                );
                haze.colored_vertex(
                    Pos2::new(top_rect.right() - corner_pad, top_rect.top()),
                    palette.frost_haze_top,
                );
                haze.colored_vertex(
                    Pos2::new(top_rect.left(), top_rect.bottom()),
                    Color32::TRANSPARENT,
                );
                haze.colored_vertex(
                    Pos2::new(top_rect.right(), top_rect.bottom()),
                    Color32::TRANSPARENT,
                );
                haze.add_triangle(0, 1, 3);
                haze.add_triangle(0, 3, 2);
                shapes.push(Shape::mesh(haze));
            }
        }
        GlassFinish::Clear => {
            // Crisp liquid sheen along upper third
            if rect.height() > 24.0 && rect.width() > 24.0 {
                let sheen_h = (rect.height() * 0.34).clamp(12.0, 40.0);
                let inset = 1.5_f32;
                let top_rect = Rect::from_min_max(
                    Pos2::new(rect.left() + inset, rect.top() + inset),
                    Pos2::new(
                        rect.right() - inset,
                        (rect.top() + sheen_h).min(rect.bottom() - inset),
                    ),
                );
                let mut sheen = Mesh::default();
                let corner_pad = (r_f32 * 0.45).min(top_rect.width() * 0.25);
                sheen.colored_vertex(
                    Pos2::new(top_rect.left() + corner_pad, top_rect.top()),
                    palette.sheen_top,
                );
                sheen.colored_vertex(
                    Pos2::new(top_rect.right() - corner_pad, top_rect.top()),
                    palette.sheen_top,
                );
                sheen.colored_vertex(
                    Pos2::new(top_rect.left(), top_rect.bottom()),
                    Color32::TRANSPARENT,
                );
                sheen.colored_vertex(
                    Pos2::new(top_rect.right(), top_rect.bottom()),
                    Color32::TRANSPARENT,
                );
                sheen.add_triangle(0, 1, 3);
                sheen.add_triangle(0, 3, 2);
                shapes.push(Shape::mesh(sheen));
            }
        }
    }

    // 4. Refractive hairline perimeter border
    shapes.push(Shape::rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    ));

    // 5. Specular top catchlight (brighter on Clear glass, softer satin on Frosted glass)
    if elevation != GlassElevation::Well && rect.width() > r_f32 * 2.0 + 12.0 {
        let y = rect.top() + 1.0;
        let x0 = rect.left() + r_f32.max(10.0);
        let x1 = rect.right() - r_f32.max(10.0);
        let rim_col = match finish {
            GlassFinish::Clear => palette.specular_top,
            GlassFinish::Frosted => rgba(
                palette.specular_top.r(),
                palette.specular_top.g(),
                palette.specular_top.b(),
                palette.specular_top.a() / 2,
            ),
        };
        shapes.push(Shape::line_segment(
            [Pos2::new(x0, y), Pos2::new(x1, y)],
            Stroke::new(1.0_f32, rim_col),
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
        GlassElevation::Toolbar,
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

/// Apple Liquid Glass capsule button with smooth animated hover/press transitions
/// and specular top catchlight.
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
        let hover_t = ui
            .ctx()
            .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.14);
        let press_t = ui.ctx().animate_bool_with_time(
            resp.id.with("press"),
            resp.is_pointer_button_down_on(),
            0.09,
        );

        // Subtle tactile shrink on press like iOS/macOS buttons
        let draw_rect = rect.shrink(press_t * 0.9);
        let cr = CornerRadius::same((draw_rect.height() * 0.5) as u8);
        let bg = lerp_color(fill, hover_fill, (hover_t + press_t * 0.5).min(1.0));

        let p = ui.painter();
        if matches!(tone, ButtonTone::Primary | ButtonTone::Danger) {
            let glow_alpha = (56.0 + hover_t * 40.0) as u8;
            let glow = Shadow {
                offset: [0, 3],
                blur: (12.0 + hover_t * 5.0) as u8,
                spread: 0,
                color: rgba(fill.r(), fill.g(), fill.b(), glow_alpha),
            };
            p.add(glow.as_shape(draw_rect, cr));
        }

        p.rect_filled(draw_rect, cr, bg);
        let stroke_a = lerp_color(stroke_col, Color32::WHITE, hover_t * 0.25);
        p.rect_stroke(draw_rect, cr, Stroke::new(1.0_f32, stroke_a), StrokeKind::Inside);

        // Specular top catchlight on capsule
        if bg.a() > 0 {
            let pad = draw_rect.height() * 0.45;
            p.line_segment(
                [
                    Pos2::new(draw_rect.left() + pad, draw_rect.top() + 1.0),
                    Pos2::new(draw_rect.right() - pad, draw_rect.top() + 1.0),
                ],
                Stroke::new(1.0_f32, rgba(255, 255, 255, (96.0 + hover_t * 50.0) as u8)),
            );
        }

        let text_pos = Pos2::new(
            draw_rect.center().x - galley.size().x * 0.5,
            draw_rect.center().y - galley.size().y * 0.5,
        );
        p.galley(text_pos, galley, text_col);
    }

    resp
}

/// Apple Liquid Glass animated pill toggle switch (with iOS 26-style liquid thumb glide & stretch).
pub fn glass_toggle(
    ui: &mut egui::Ui,
    palette: &Palette,
    on: &mut bool,
    label: &str,
) -> Response {
    let mut changed = false;
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            let desired = Vec2::new(42.0, 24.0);
            let (rect, switch_resp) = ui.allocate_exact_size(desired, Sense::click());
            if switch_resp.clicked() {
                *on = !*on;
                changed = true;
            }

            if ui.is_rect_visible(rect) {
                let t = ui
                    .ctx()
                    .animate_bool_with_time(switch_resp.id.with("toggle"), *on, 0.22);
                let hover_t = ui.ctx().animate_bool_with_time(
                    switch_resp.id.with("hover"),
                    switch_resp.hovered(),
                    0.14,
                );
                let press_t = ui.ctx().animate_bool_with_time(
                    switch_resp.id.with("press"),
                    switch_resp.is_pointer_button_down_on(),
                    0.10,
                );

                // Mid-motion liquid stretch peaks at t == 0.5 or while pressed
                let motion_stretch = (1.0 - (2.0 * t - 1.0).abs()) * 5.0 + press_t * 3.5;

                let p = ui.painter();
                let track_cr = CornerRadius::same((rect.height() * 0.5) as u8);
                let off_track = palette.well_fill;
                let on_track = rgba(
                    palette.accent_emerald.r(),
                    palette.accent_emerald.g(),
                    palette.accent_emerald.b(),
                    215,
                );
                let track_fill = lerp_color(off_track, on_track, t);
                let track_border = lerp_color(
                    palette.border_subtle,
                    rgba(180, 255, 215, 165),
                    t.max(hover_t * 0.4),
                );

                p.rect_filled(rect, track_cr, track_fill);
                p.rect_stroke(
                    rect,
                    track_cr,
                    Stroke::new(1.0_f32, track_border),
                    StrokeKind::Inside,
                );

                // Liquid glass thumb (glides smoothly + stretches horizontally mid-transition)
                let pad = 2.5_f32;
                let base_d = rect.height() - pad * 2.0;
                let thumb_w = base_d + motion_stretch;
                let min_x = rect.left() + pad;
                let max_x = rect.right() - pad - thumb_w;
                let thumb_x = min_x + (max_x - min_x) * t;
                let thumb_rect = Rect::from_min_size(
                    Pos2::new(thumb_x, rect.top() + pad),
                    Vec2::new(thumb_w, base_d),
                );
                let thumb_cr = CornerRadius::same((base_d * 0.5) as u8);

                let thumb_shadow = Shadow {
                    offset: [0, 2],
                    blur: 8,
                    spread: 0,
                    color: rgba(0, 0, 0, 72),
                };
                p.add(thumb_shadow.as_shape(thumb_rect, thumb_cr));

                let thumb_fill = rgba(255, 255, 255, 240);
                p.rect_filled(thumb_rect, thumb_cr, thumb_fill);
                p.rect_stroke(
                    thumb_rect,
                    thumb_cr,
                    Stroke::new(1.0_f32, Color32::WHITE),
                    StrokeKind::Inside,
                );
            }

            if !label.is_empty() {
                let lbl_resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(13.0)
                            .color(palette.text_primary),
                    )
                    .sense(Sense::click()),
                );
                if lbl_resp.clicked() {
                    *on = !*on;
                    changed = true;
                }
            }

            switch_resp
        })
        .inner;

    if changed {
        ui.ctx().request_repaint();
    }
    resp
}

/// Full-width sidebar capsule button with smooth animated hover/selection transitions,
/// painted vector icon, and optional count badge.
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
        let sel_t = ui
            .ctx()
            .animate_bool_with_time(resp.id.with("sel"), selected, 0.20);
        let hov_t = ui
            .ctx()
            .animate_bool_with_time(resp.id.with("hov"), resp.hovered() && !selected, 0.14);

        let cr = CornerRadius::same(14);
        let p = ui.painter();

        if sel_t > 0.01 {
            let active_fill = match palette.mode {
                GlassMode::Dark => rgba(255, 255, 255, (42.0 * sel_t) as u8),
                GlassMode::Light => rgba(255, 255, 255, (232.0 * sel_t) as u8),
            };
            let shadow = Shadow {
                offset: [0, 3],
                blur: 10,
                spread: 0,
                color: rgba(
                    palette.shadow_color.r(),
                    palette.shadow_color.g(),
                    palette.shadow_color.b(),
                    (palette.shadow_color.a() as f32 * sel_t) as u8,
                ),
            };
            p.add(shadow.as_shape(rect, cr));
            p.rect_filled(rect, cr, active_fill);
            p.rect_stroke(
                rect,
                cr,
                Stroke::new(
                    1.0_f32,
                    lerp_color(Color32::TRANSPARENT, palette.border_bright, sel_t),
                ),
                StrokeKind::Inside,
            );
            p.line_segment(
                [
                    Pos2::new(rect.left() + 14.0, rect.top() + 1.0),
                    Pos2::new(rect.right() - 14.0, rect.top() + 1.0),
                ],
                Stroke::new(
                    1.0_f32,
                    lerp_color(Color32::TRANSPARENT, palette.specular_top, sel_t),
                ),
            );
        }

        if hov_t > 0.01 {
            p.rect_filled(
                rect,
                cr,
                lerp_color(Color32::TRANSPARENT, palette.control_fill, hov_t),
            );
            p.rect_stroke(
                rect,
                cr,
                Stroke::new(
                    1.0_f32,
                    lerp_color(Color32::TRANSPARENT, palette.border_subtle, hov_t),
                ),
                StrokeKind::Inside,
            );
        }

        let icon_color = lerp_color(palette.text_muted, palette.accent_blue, sel_t.max(hov_t * 0.4));
        let text_color = lerp_color(palette.text_secondary, palette.text_primary, sel_t.max(hov_t * 0.6));

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
            let badge_text_col = lerp_color(palette.text_muted, palette.accent_blue, sel_t);
            let badge_galley = p.layout_no_wrap(
                count_str,
                egui::FontId::proportional(11.0),
                badge_text_col,
            );
            let bw = (badge_galley.size().x + 14.0).max(24.0);
            let bh = 20.0;
            let brect = Rect::from_min_size(
                Pos2::new(rect.right() - bw - 10.0, rect.center().y - bh * 0.5),
                Vec2::new(bw, bh),
            );
            let badge_fill = lerp_color(
                palette.well_fill,
                rgba(palette.accent_blue.r(), palette.accent_blue.g(), palette.accent_blue.b(), 40),
                sel_t,
            );
            p.rect_filled(brect, CornerRadius::same(10), badge_fill);
            p.galley(
                Pos2::new(
                    brect.center().x - badge_galley.size().x * 0.5,
                    brect.center().y - badge_galley.size().y * 0.5,
                ),
                badge_galley,
                badge_text_col,
            );
        }
    }

    resp
}

/// Apple Liquid Glass segmented control with a smoothly gliding clear-glass pill thumb.
pub fn segmented_control<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    palette: &Palette,
    id_salt: &str,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    let ctrl_id = ui.make_persistent_id(id_salt);

    glass_box(
        ui,
        palette,
        GlassElevation::Well,
        16,
        Margin::same(3),
        |ui| {
            // Reserve a shape slot behind the labels for the smoothly gliding glass thumb
            let thumb_slot = ui.painter().add(Shape::Noop);
            let mut seg_rects: Vec<Rect> = Vec::with_capacity(options.len());
            let mut active_idx = 0usize;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (idx, &(value, label)) in options.iter().enumerate() {
                    let selected = *current == value;
                    if selected {
                        active_idx = idx;
                    }
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
                    seg_rects.push(rect);

                    if ui.is_rect_visible(rect) {
                        let p = ui.painter();
                        if !selected && resp.hovered() {
                            p.rect_filled(rect, CornerRadius::same(13), palette.control_fill);
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
                        active_idx = idx;
                        changed = true;
                    }
                }
            });

            if let Some(target_rect) = seg_rects.get(active_idx) {
                let base_x = seg_rects[0].left();
                let target_rel_x = target_rect.left() - base_x;
                let target_w = target_rect.width();

                let anim_x = ui
                    .ctx()
                    .animate_value_with_time(ctrl_id.with("x"), target_rel_x, 0.22);
                let anim_w = ui
                    .ctx()
                    .animate_value_with_time(ctrl_id.with("w"), target_w, 0.22);

                // Slight liquid stretch when gliding rapidly between segments
                let velocity_stretch = ((target_rel_x - anim_x).abs() * 0.08).min(6.0);
                let thumb_rect = Rect::from_min_size(
                    Pos2::new(base_x + anim_x - velocity_stretch * 0.5, target_rect.top()),
                    Vec2::new(anim_w + velocity_stretch, target_rect.height()),
                );
                let cr = CornerRadius::same(13);

                let shadow = Shadow {
                    offset: [0, 2],
                    blur: 8,
                    spread: 0,
                    color: palette.shadow_color,
                };
                let pad = thumb_rect.height() * 0.45;
                let thumb_shapes = vec![
                    Shape::Rect(shadow.as_shape(thumb_rect, cr)),
                    Shape::rect_filled(thumb_rect, cr, palette.segment_thumb_fill),
                    Shape::rect_stroke(
                        thumb_rect,
                        cr,
                        Stroke::new(1.0_f32, palette.segment_thumb_border),
                        StrokeKind::Inside,
                    ),
                    Shape::line_segment(
                        [
                            Pos2::new(thumb_rect.left() + pad, thumb_rect.top() + 1.0),
                            Pos2::new(thumb_rect.right() - pad, thumb_rect.top() + 1.0),
                        ],
                        Stroke::new(1.0_f32, palette.specular_top),
                    ),
                ];
                ui.painter().set(thumb_slot, Shape::Vec(thumb_shapes));
            }
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

                let text_w =
                    (ui.available_width() - if query.is_empty() { 4.0 } else { 24.0 }).max(80.0);
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
                        ui.painter().line_segment(
                            [Pos2::new(c.x - s, c.y - s), Pos2::new(c.x + s, c.y + s)],
                            stroke,
                        );
                        ui.painter().line_segment(
                            [Pos2::new(c.x + s, c.y - s), Pos2::new(c.x - s, c.y + s)],
                            stroke,
                        );
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

    let glow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: rgba(
            palette.accent_blue.r(),
            palette.accent_blue.g(),
            palette.accent_blue.b(),
            80,
        ),
    };
    p.add(glow.as_shape(rect, cr));

    p.rect_filled(rect, cr, rgba(22, 134, 255, 225));
    let inner_orb = Pos2::new(rect.right() - size * 0.25, rect.bottom() - size * 0.25);
    p.circle_filled(inner_orb, size * 0.36, rgba(48, 224, 152, 145));
    p.rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, rgba(255, 255, 255, 165)),
        StrokeKind::Inside,
    );

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
    p.circle_stroke(
        c,
        r - 0.5,
        Stroke::new(1.0_f32, rgba(accent.r(), accent.g(), accent.b(), 120)),
    );

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

/// Paints a fixed-size rounded frosted placeholder icon (guaranteed exact `size x size`).
pub fn paint_icon_placeholder(ui: &mut egui::Ui, palette: &Palette, title: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let cr = CornerRadius::same(14);
    let p = ui.painter();
    p.rect_filled(rect, cr, palette.well_fill);
    p.rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, palette.border_subtle),
        StrokeKind::Inside,
    );

    let initial = title
        .chars()
        .next()
        .unwrap_or('M')
        .to_ascii_uppercase()
        .to_string();
    let galley = p.layout_no_wrap(
        initial,
        egui::FontId::proportional(size * 0.42),
        palette.accent_blue,
    );
    p.galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        palette.accent_blue,
    );
}

fn paint_search_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    let lens_c = Pos2::new(c.x - 1.5, c.y - 1.5);
    let stroke = Stroke::new(1.4_f32, color);
    p.circle_stroke(lens_c, 4.5, stroke);
    p.line_segment(
        [
            Pos2::new(lens_c.x + 3.2, lens_c.y + 3.2),
            Pos2::new(c.x + 5.5, c.y + 5.5),
        ],
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
            p.add(Shape::closed_line(
                vec![top, right, br, bot, bl, left],
                stroke,
            ));
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
            for (dy, kx) in [(-3.8_f32, -2.0_f32), (0.0, 2.2), (3.8, -1.0)] {
                let y = c.y + dy;
                p.line_segment([Pos2::new(c.x - 6.0, y), Pos2::new(c.x + 6.0, y)], stroke);
                p.circle_filled(Pos2::new(c.x + kx, y), 2.1, color);
            }
        }
    }
}
