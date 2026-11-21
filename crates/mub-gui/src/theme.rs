use std::sync::Arc;

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, FontData, FontDefinitions, FontFamily, IconData, Id,
    Margin, Pos2, Rect, Response, Sense, Shadow, Shape, Stroke, StrokeKind, TextureFilter,
    TextureHandle, TextureOptions, TextureWrapMode, Vec2, epaint::RectShape,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
    #[default]
    SynthTerminal,
    GruvboxPixel,
    TokyoNight,
    DraftingPaper,
}

impl ThemePreset {
    pub const ALL: [Self; 4] = [
        Self::SynthTerminal,
        Self::GruvboxPixel,
        Self::TokyoNight,
        Self::DraftingPaper,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::SynthTerminal => "synth_terminal",
            Self::GruvboxPixel => "gruvbox_pixel",
            Self::TokyoNight => "tokyo_night",
            Self::DraftingPaper => "drafting_paper",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "gruvbox_pixel" | "gruvbox" => Self::GruvboxPixel,
            "tokyo_night" | "tokyo" => Self::TokyoNight,
            "drafting_paper" | "paper" => Self::DraftingPaper,
            _ => Self::SynthTerminal,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::SynthTerminal => "SYNTH TERMINAL",
            Self::GruvboxPixel => "GRUVBOX PIXEL",
            Self::TokyoNight => "TOKYO NIGHT",
            Self::DraftingPaper => "DRAFTING PAPER",
        }
    }

    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::SynthTerminal => "Signature Multi-Hue",
            Self::GruvboxPixel => "Warm Retro Arcade",
            Self::TokyoNight => "Crisp Neon Dusk",
            Self::DraftingPaper => "Clean Light Studio",
        }
    }

    pub const fn is_light(self) -> bool {
        matches!(self, Self::DraftingPaper)
    }

    /// Returns `[bg, focus, bot, player, mr, cf]` for the palette swatch card.
    pub const fn swatch_colors(self) -> [Color32; 6] {
        let p = Palette::for_preset(self);
        [p.bg, p.focus, p.bot, p.player, p.mr, p.cf]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonTone {
    Cta,
    Secondary,
    Subtle,
    Focus,
    Modrinth,
    CurseForge,
    Bot,
    Player,
    Warn,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavIcon {
    Instances,
    Collections,
    Discover,
    Accounts,
    Console,
    Config,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub preset: ThemePreset,

    // Surfaces & structural lines
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub border_hi: Color32,
    pub dot: Color32,

    // Ink / typography
    pub ink: Color32,
    pub ink_dim: Color32,
    pub ink_muted: Color32,

    // 8 Semantic Roles (+ *_bg)
    pub focus: Color32,
    pub focus_bg: Color32,
    pub on_focus: Color32,

    pub cta_bg: Color32,
    pub cta_fg: Color32,

    pub bot: Color32,
    pub bot_bg: Color32,

    pub player: Color32,
    pub player_bg: Color32,

    pub mr: Color32,
    pub mr_bg: Color32,

    pub cf: Color32,
    pub cf_bg: Color32,

    pub warn: Color32,
    pub warn_bg: Color32,

    pub meta: Color32,
    pub meta_bg: Color32,

    pub danger: Color32,
    pub danger_bg: Color32,
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
    pub const fn for_preset(preset: ThemePreset) -> Self {
        match preset {
            ThemePreset::SynthTerminal => Self {
                preset,
                bg: Color32::from_rgb(12, 14, 20),
                panel: Color32::from_rgb(17, 20, 29),
                surface: Color32::from_rgb(22, 27, 38),
                raised: Color32::from_rgb(29, 36, 51),
                hover: Color32::from_rgb(37, 46, 66),
                border: Color32::from_rgb(39, 48, 67),
                border_hi: Color32::from_rgb(59, 72, 99),
                dot: rgba(165, 180, 210, 20),

                ink: Color32::from_rgb(237, 241, 248),
                ink_dim: Color32::from_rgb(168, 179, 199),
                ink_muted: Color32::from_rgb(125, 138, 161),

                focus: Color32::from_rgb(129, 140, 248),
                focus_bg: rgba(129, 140, 248, 31),
                on_focus: Color32::from_rgb(11, 14, 23),

                cta_bg: Color32::from_rgb(237, 241, 248),
                cta_fg: Color32::from_rgb(12, 14, 20),

                bot: Color32::from_rgb(192, 132, 252),
                bot_bg: rgba(192, 132, 252, 31),

                player: Color32::from_rgb(56, 189, 248),
                player_bg: rgba(56, 189, 248, 31),

                mr: Color32::from_rgb(34, 211, 126),
                mr_bg: rgba(34, 211, 126, 28),

                cf: Color32::from_rgb(249, 115, 22),
                cf_bg: rgba(249, 115, 22, 31),

                warn: Color32::from_rgb(245, 183, 49),
                warn_bg: rgba(245, 183, 49, 31),

                meta: Color32::from_rgb(226, 188, 122),
                meta_bg: rgba(226, 188, 122, 28),

                danger: Color32::from_rgb(244, 91, 105),
                danger_bg: rgba(244, 91, 105, 31),
            },
            ThemePreset::GruvboxPixel => Self {
                preset,
                bg: Color32::from_rgb(22, 20, 18),
                panel: Color32::from_rgb(29, 27, 24),
                surface: Color32::from_rgb(37, 34, 30),
                raised: Color32::from_rgb(48, 43, 38),
                hover: Color32::from_rgb(60, 54, 48),
                border: Color32::from_rgb(60, 53, 46),
                border_hi: Color32::from_rgb(87, 77, 67),
                dot: rgba(235, 219, 178, 18),

                ink: Color32::from_rgb(242, 229, 188),
                ink_dim: Color32::from_rgb(199, 184, 153),
                ink_muted: Color32::from_rgb(155, 141, 118),

                focus: Color32::from_rgb(131, 165, 152),
                focus_bg: rgba(131, 165, 152, 33),
                on_focus: Color32::from_rgb(22, 20, 18),

                cta_bg: Color32::from_rgb(242, 229, 188),
                cta_fg: Color32::from_rgb(22, 20, 18),

                bot: Color32::from_rgb(211, 134, 155),
                bot_bg: rgba(211, 134, 155, 33),

                player: Color32::from_rgb(108, 182, 200),
                player_bg: rgba(108, 182, 200, 33),

                mr: Color32::from_rgb(169, 182, 101),
                mr_bg: rgba(169, 182, 101, 33),

                cf: Color32::from_rgb(254, 128, 25),
                cf_bg: rgba(254, 128, 25, 33),

                warn: Color32::from_rgb(250, 189, 47),
                warn_bg: rgba(250, 189, 47, 33),

                meta: Color32::from_rgb(216, 166, 87),
                meta_bg: rgba(216, 166, 87, 31),

                danger: Color32::from_rgb(251, 73, 52),
                danger_bg: rgba(251, 73, 52, 33),
            },
            ThemePreset::TokyoNight => Self {
                preset,
                bg: Color32::from_rgb(14, 17, 27),
                panel: Color32::from_rgb(19, 23, 36),
                surface: Color32::from_rgb(24, 29, 46),
                raised: Color32::from_rgb(32, 39, 61),
                hover: Color32::from_rgb(41, 50, 78),
                border: Color32::from_rgb(41, 49, 74),
                border_hi: Color32::from_rgb(62, 74, 110),
                dot: rgba(122, 162, 247, 20),

                ink: Color32::from_rgb(228, 236, 250),
                ink_dim: Color32::from_rgb(164, 177, 208),
                ink_muted: Color32::from_rgb(122, 136, 168),

                focus: Color32::from_rgb(122, 162, 247),
                focus_bg: rgba(122, 162, 247, 33),
                on_focus: Color32::from_rgb(11, 15, 26),

                cta_bg: Color32::from_rgb(122, 162, 247),
                cta_fg: Color32::from_rgb(11, 15, 26),

                bot: Color32::from_rgb(187, 154, 247),
                bot_bg: rgba(187, 154, 247, 33),

                player: Color32::from_rgb(125, 207, 255),
                player_bg: rgba(125, 207, 255, 31),

                mr: Color32::from_rgb(79, 214, 190),
                mr_bg: rgba(79, 214, 190, 31),

                cf: Color32::from_rgb(255, 158, 100),
                cf_bg: rgba(255, 158, 100, 33),

                warn: Color32::from_rgb(224, 175, 104),
                warn_bg: rgba(224, 175, 104, 33),

                meta: Color32::from_rgb(207, 201, 194),
                meta_bg: rgba(207, 201, 194, 26),

                danger: Color32::from_rgb(247, 118, 142),
                danger_bg: rgba(247, 118, 142, 33),
            },
            ThemePreset::DraftingPaper => Self {
                preset,
                bg: Color32::from_rgb(243, 241, 234),
                panel: Color32::from_rgb(234, 230, 220),
                surface: Color32::from_rgb(250, 248, 243),
                raised: Color32::from_rgb(228, 223, 211),
                hover: Color32::from_rgb(218, 212, 197),
                border: Color32::from_rgb(206, 199, 183),
                border_hi: Color32::from_rgb(171, 162, 142),
                dot: rgba(23, 26, 33, 20),

                ink: Color32::from_rgb(23, 26, 33),
                ink_dim: Color32::from_rgb(68, 77, 92),
                ink_muted: Color32::from_rgb(98, 107, 123),

                focus: Color32::from_rgb(59, 91, 219),
                focus_bg: rgba(59, 91, 219, 26),
                on_focus: Color32::from_rgb(250, 248, 243),

                cta_bg: Color32::from_rgb(23, 26, 33),
                cta_fg: Color32::from_rgb(250, 248, 243),

                bot: Color32::from_rgb(142, 60, 224),
                bot_bg: rgba(142, 60, 224, 26),

                player: Color32::from_rgb(2, 132, 199),
                player_bg: rgba(2, 132, 199, 26),

                mr: Color32::from_rgb(15, 140, 88),
                mr_bg: rgba(15, 140, 88, 26),

                cf: Color32::from_rgb(217, 83, 30),
                cf_bg: rgba(217, 83, 30, 26),

                warn: Color32::from_rgb(176, 109, 0),
                warn_bg: rgba(176, 109, 0, 28),

                meta: Color32::from_rgb(138, 101, 38),
                meta_bg: rgba(138, 101, 38, 26),

                danger: Color32::from_rgb(201, 42, 56),
                danger_bg: rgba(201, 42, 56, 26),
            },
        }
    }

    /// Foreground color used on solid-filled semantic buttons (e.g. hover fill).
    pub const fn on_solid(self) -> Color32 {
        if self.preset.is_light() {
            Color32::from_rgb(250, 248, 243)
        } else {
            Color32::from_rgb(9, 11, 14)
        }
    }
}

fn install_terminal_fonts_once(ctx: &egui::Context) {
    let key = Id::new("mub_terminal_fonts_installed_v1");
    if ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, true));

    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &[
            r"C:\Windows\Fonts\JetBrainsMono-Regular.ttf",
            r"C:\Windows\Fonts\CascadiaMono.ttf",
            r"C:\Windows\Fonts\consola.ttf",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/Library/Fonts/JetBrainsMono-Regular.ttf",
            "/System/Library/Fonts/SFNSMono.ttf",
            "/System/Library/Fonts/Menlo.ttc",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
            "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        ]
    };

    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = FontDefinitions::default();
            fonts
                .font_data
                .insert("mub_mono".to_owned(), Arc::new(FontData::from_owned(bytes)));
            if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
                fam.insert(0, "mub_mono".to_owned());
            }
            if let Some(fam) = fonts.families.get_mut(&FontFamily::Monospace) {
                fam.insert(0, "mub_mono".to_owned());
            }
            ctx.set_fonts(fonts);
            break;
        }
    }
}

pub fn apply_theme(ctx: &egui::Context, preset: ThemePreset) {
    install_terminal_fonts_once(ctx);

    let p = Palette::for_preset(preset);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.interact_size.y = 26.0;
    style.spacing.combo_height = 240.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.handle_min_length = 24.0;
    style.animation_time = 0.08;

    let mut visuals = if preset.is_light() {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    visuals.dark_mode = !preset.is_light();
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = p.raised;
    visuals.extreme_bg_color = p.bg;
    visuals.faint_bg_color = p.surface;
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border_hi);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.hyperlink_color = p.player;

    visuals.widgets.noninteractive.bg_fill = p.surface;
    visuals.widgets.noninteractive.weak_bg_fill = p.bg;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;

    visuals.widgets.inactive.bg_fill = p.raised;
    visuals.widgets.inactive.weak_bg_fill = p.raised;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border_hi);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;

    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.ink_dim);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;

    visuals.widgets.active.bg_fill = p.focus_bg;
    visuals.widgets.active.weak_bg_fill = p.focus_bg;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.focus);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.active.corner_radius = CornerRadius::ZERO;

    visuals.widgets.open.bg_fill = p.raised;
    visuals.widgets.open.weak_bg_fill = p.raised;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.focus);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.ink);
    visuals.widgets.open.corner_radius = CornerRadius::ZERO;

    visuals.selection.bg_fill = p.focus_bg;
    visuals.selection.stroke = Stroke::new(1.0_f32, p.focus);

    style.visuals = visuals;
    ctx.set_style(style);
}

fn dot_grid_texture(ctx: &egui::Context) -> TextureHandle {
    let key = Id::new("mub_dot_grid_tex_16x16_v1");
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(key)) {
        return tex;
    }

    const N: usize = 16;
    let mut pixels = vec![Color32::TRANSPARENT; N * N];
    pixels[0] = Color32::WHITE;

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
    let tex = ctx.load_texture("mub_dot_grid_16", img, opts);
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    tex
}

/// Paints the solid terminal background and a single GPU-tiled 16x16 dot-grid pattern.
pub fn paint_dot_grid_backdrop(ctx: &egui::Context, palette: &Palette) {
    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(rect, CornerRadius::ZERO, palette.bg);

    let tex = dot_grid_texture(ctx);
    let uv = Rect::from_min_max(
        Pos2::ZERO,
        Pos2::new(rect.width() / 16.0, rect.height() / 16.0),
    );
    let dot_rect = RectShape::filled(rect, CornerRadius::ZERO, palette.dot).with_texture(tex.id(), uv);
    painter.add(Shape::Rect(dot_rect));
}

/// Sharp 0px-radius terminal box with 1px inside border.
pub fn term_box<R>(
    ui: &mut egui::Ui,
    bg: Color32,
    border: Color32,
    margin: Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add_contents)
                .inner
        })
}

/// Standard surface container (`palette.surface`, `palette.border`).
pub fn term_surface<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    term_box(ui, palette.surface, palette.border, Margin::same(12), add_contents)
}

/// Raised card / modal / hero container (`palette.raised`, `palette.border_hi`).
pub fn term_raised<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    term_box(ui, palette.raised, palette.border_hi, Margin::same(12), add_contents)
}

/// Interactive list row container that highlights from `surface`/`border` to `raised`/`border_hi` on hover.
pub fn term_row<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let bg_idx = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::NONE
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add_contents)
                .inner
        });
    let rect = inner.response.rect;
    let hovered = ui.rect_contains_pointer(rect);
    let fill = if hovered { palette.raised } else { palette.surface };
    let border = if hovered { palette.border_hi } else { palette.border };
    ui.painter().set(
        bg_idx,
        Shape::Vec(vec![
            Shape::rect_filled(rect, CornerRadius::ZERO, fill),
            Shape::rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0_f32, border),
                StrokeKind::Inside,
            ),
        ]),
    );
    inner
}

/// Sharp 1px rectangular terminal button supporting all semantic tones.
pub fn term_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    tone: ButtonTone,
) -> Response {
    let solid_fg = palette.on_solid();
    let (fill, hover_fill, stroke, hover_stroke, fg, hover_fg) = match tone {
        ButtonTone::Cta => (
            palette.cta_bg,
            lerp_color(palette.cta_bg, palette.bg, 0.14),
            palette.cta_bg,
            palette.cta_bg,
            palette.cta_fg,
            palette.cta_fg,
        ),
        ButtonTone::Secondary => (
            palette.raised,
            palette.hover,
            palette.border_hi,
            palette.ink_dim,
            palette.ink,
            palette.ink,
        ),
        ButtonTone::Subtle => (
            Color32::TRANSPARENT,
            palette.surface,
            palette.border,
            palette.border_hi,
            palette.ink_dim,
            palette.ink,
        ),
        ButtonTone::Focus => (
            palette.focus_bg,
            palette.focus,
            palette.focus,
            palette.focus,
            palette.focus,
            solid_fg,
        ),
        ButtonTone::Modrinth => (
            palette.mr_bg,
            palette.mr,
            palette.mr,
            palette.mr,
            palette.mr,
            solid_fg,
        ),
        ButtonTone::CurseForge => (
            palette.cf_bg,
            palette.cf,
            palette.cf,
            palette.cf,
            palette.cf,
            solid_fg,
        ),
        ButtonTone::Bot => (
            palette.bot_bg,
            palette.bot,
            palette.bot,
            palette.bot,
            palette.bot,
            solid_fg,
        ),
        ButtonTone::Player => (
            palette.player_bg,
            palette.player,
            palette.player,
            palette.player,
            palette.player,
            solid_fg,
        ),
        ButtonTone::Warn => (
            palette.warn_bg,
            palette.warn,
            palette.warn,
            palette.warn,
            palette.warn,
            solid_fg,
        ),
        ButtonTone::Danger => (
            palette.danger_bg,
            palette.danger,
            palette.danger,
            palette.danger,
            palette.danger,
            solid_fg,
        ),
    };

    let font_id = egui::FontId::monospace(12.0);
    let measure = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id.clone(), fg);
    let desired = Vec2::new((measure.size().x + 18.0).max(44.0), 26.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let active = resp.hovered() || resp.is_pointer_button_down_on();
        let bg = if active { hover_fill } else { fill };
        let brd = if active { hover_stroke } else { stroke };
        let txt_col = if active { hover_fg } else { fg };

        let p = ui.painter();
        if bg.a() > 0 {
            p.rect_filled(rect, CornerRadius::ZERO, bg);
        }
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, brd),
            StrokeKind::Inside,
        );

        let galley = p.layout_no_wrap(label.to_string(), font_id, txt_col);
        p.galley(
            Pos2::new(
                rect.center().x - galley.size().x * 0.5,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            txt_col,
        );
    }

    resp
}

/// Sharp 1px rectangular tag badge.
pub fn term_badge(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    term_badge_bordered(ui, text, fg, bg, fg);
}

/// Sharp 1px rectangular tag badge with explicit border color (atomic, never wraps vertically).
pub fn term_badge_bordered(
    ui: &mut egui::Ui,
    text: &str,
    fg: Color32,
    bg: Color32,
    border: Color32,
) {
    let font_id = egui::FontId::monospace(11.0);
    let measure = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), fg);
    let desired = Vec2::new(measure.size().x + 12.0, (measure.size().y + 4.0).max(19.0));
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if bg.a() > 0 {
            p.rect_filled(rect, CornerRadius::ZERO, bg);
        }
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
        let galley = p.layout_no_wrap(text.to_string(), font_id, fg);
        p.galley(
            Pos2::new(
                rect.center().x - galley.size().x * 0.5,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            fg,
        );
    }
}

/// Full-width vertical filter rail option row with single-line truncated monospace text.
pub fn term_rail_option(
    ui: &mut egui::Ui,
    palette: &Palette,
    prefix: &str,
    label: &str,
    active: bool,
    accent: Color32,
) -> Response {
    let desired = Vec2::new(ui.available_width().max(60.0), 22.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if active {
            p.rect_filled(
                rect,
                CornerRadius::ZERO,
                rgba(accent.r(), accent.g(), accent.b(), 22),
            );
        } else if resp.hovered() {
            p.rect_filled(rect, CornerRadius::ZERO, palette.raised);
        }

        let col = if active {
            accent
        } else if resp.hovered() {
            palette.ink
        } else {
            palette.ink_muted
        };

        let text = format!("{prefix} {label}");
        let mut job = egui::text::LayoutJob::single_section(
            text,
            egui::TextFormat {
                font_id: egui::FontId::monospace(11.5),
                color: col,
                ..Default::default()
            },
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 8.0).max(20.0));
        let galley = p.layout_job(job);
        p.galley(
            Pos2::new(rect.left() + 4.0, rect.center().y - galley.size().y * 0.5),
            galley,
            col,
        );
    }

    resp
}

/// Left sidebar navigation row with 2px hard left `palette.focus` bar and crisp pixel-art icon.
#[allow(clippy::too_many_arguments)]
pub fn term_nav_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    num: &str,
    icon: NavIcon,
    label: &str,
    badge_text: Option<&str>,
    badge_col: Color32,
    selected: bool,
) -> Response {
    let desired = Vec2::new(ui.available_width(), 32.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let hovered = resp.hovered();

        if selected {
            p.rect_filled(rect, CornerRadius::ZERO, palette.focus_bg);
            let bar = Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height()));
            p.rect_filled(bar, CornerRadius::ZERO, palette.focus);
        } else if hovered {
            p.rect_filled(rect, CornerRadius::ZERO, palette.surface);
        }

        let num_col = if selected {
            palette.focus
        } else {
            palette.ink_muted
        };
        let icon_col = if selected {
            palette.focus
        } else if hovered {
            palette.ink
        } else {
            palette.ink_muted
        };
        let lbl_col = if selected || hovered {
            palette.ink
        } else {
            palette.ink_dim
        };

        let num_galley = p.layout_no_wrap(num.to_string(), egui::FontId::monospace(11.5), num_col);
        p.galley(
            Pos2::new(rect.left() + 10.0, rect.center().y - num_galley.size().y * 0.5),
            num_galley,
            num_col,
        );

        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 36.0, rect.center().y),
            Vec2::splat(14.0),
        );
        paint_pixel_nav_icon(p, icon_rect, icon, icon_col);

        let lbl_galley =
            p.layout_no_wrap(label.to_string(), egui::FontId::monospace(12.0), lbl_col);
        p.galley(
            Pos2::new(rect.left() + 50.0, rect.center().y - lbl_galley.size().y * 0.5),
            lbl_galley,
            lbl_col,
        );

        if let Some(badge) = badge_text.filter(|b| !b.is_empty()) {
            let b_col = if selected { badge_col } else { palette.ink_muted };
            let b_galley =
                p.layout_no_wrap(badge.to_string(), egui::FontId::monospace(11.0), b_col);
            p.galley(
                Pos2::new(
                    rect.right() - b_galley.size().x - 10.0,
                    rect.center().y - b_galley.size().y * 0.5,
                ),
                b_galley,
                b_col,
            );
        }
    }

    resp
}

/// Terminal tab button with hard 2px bottom `palette.focus` bar when active.
pub fn term_tab(ui: &mut egui::Ui, palette: &Palette, label: &str, active: bool) -> Response {
    let font_id = egui::FontId::monospace(12.0);
    let measure = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id.clone(), palette.ink);
    let desired = Vec2::new(measure.size().x + 8.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let col = if active {
            palette.focus
        } else if resp.hovered() {
            palette.ink
        } else {
            palette.ink_muted
        };
        let galley = p.layout_no_wrap(label.to_string(), font_id, col);
        p.galley(
            Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
            galley,
            col,
        );
        if active {
            let bar = Rect::from_min_max(
                Pos2::new(rect.left(), rect.bottom() - 2.0),
                Pos2::new(rect.right(), rect.bottom()),
            );
            p.rect_filled(bar, CornerRadius::ZERO, palette.focus);
        }
    }

    resp
}

/// Terminal command/search input with `>` or `/` prompt in `palette.focus` and inline `[×]` clear button.
/// Returns `(text_edit_response, cleared, enter_pressed)`.
pub fn term_search_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    query: &mut String,
    prompt: &str,
    hint: &str,
    width: f32,
) -> (Response, bool, bool) {
    let mut cleared = false;
    let mut enter_pressed = false;
    let mut edit_resp: Option<Response> = None;

    let inner_w = (width - 16.0).max(100.0);
    let frame_idx = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::NONE
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_max_width(inner_w);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    egui::RichText::new(prompt)
                        .monospace()
                        .size(12.0)
                        .strong()
                        .color(palette.focus),
                );

                let clear_w = if query.is_empty() { 0.0 } else { 26.0 };
                let text_w = (inner_w - 20.0 - clear_w).max(60.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text(hint)
                        .font(egui::FontId::monospace(12.0))
                        .frame(false)
                        .desired_width(text_w),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    enter_pressed = true;
                }
                edit_resp = Some(resp);

                if !query.is_empty() {
                    let (x_rect, x_resp) =
                        ui.allocate_exact_size(Vec2::new(22.0, 18.0), Sense::click());
                    if ui.is_rect_visible(x_rect) {
                        let col = if x_resp.hovered() {
                            palette.danger
                        } else {
                            palette.ink_muted
                        };
                        let g = ui.painter().layout_no_wrap(
                            "[×]".to_string(),
                            egui::FontId::monospace(11.0),
                            col,
                        );
                        ui.painter().galley(
                            Pos2::new(
                                x_rect.center().x - g.size().x * 0.5,
                                x_rect.center().y - g.size().y * 0.5,
                            ),
                            g,
                            col,
                        );
                    }
                    if x_resp.clicked() {
                        query.clear();
                        cleared = true;
                    }
                }
            });
        });

    let resp = edit_resp.expect("search text edit rendered");
    let border = if resp.has_focus() {
        palette.focus
    } else {
        palette.border_hi
    };
    let rect = inner.response.rect;
    ui.painter().set(
        frame_idx,
        Shape::Vec(vec![
            Shape::rect_filled(rect, CornerRadius::ZERO, palette.bg),
            Shape::rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0_f32, border),
                StrokeKind::Inside,
            ),
        ]),
    );

    (resp, cleared, enter_pressed)
}

/// Crisp terminal checkbox toggle `[×] label` / `[ ] label`.
pub fn term_toggle(
    ui: &mut egui::Ui,
    palette: &Palette,
    on: &mut bool,
    label: &str,
    accent: Color32,
) -> Response {
    let text = if label.is_empty() {
        if *on { "[×]".to_string() } else { "[ ]".to_string() }
    } else if *on {
        format!("[×] {label}")
    } else {
        format!("[ ] {label}")
    };

    let font_id = egui::FontId::monospace(12.0);
    let fg = if *on { accent } else { palette.ink_dim };
    let measure = ui.painter().layout_no_wrap(text.clone(), font_id.clone(), fg);
    let desired = Vec2::new(measure.size().x + 14.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());

    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let bg = if *on {
            rgba(accent.r(), accent.g(), accent.b(), 28)
        } else if resp.hovered() {
            palette.surface
        } else {
            Color32::TRANSPARENT
        };
        let border = if *on {
            accent
        } else if resp.hovered() {
            palette.border_hi
        } else {
            palette.border
        };
        let draw_fg = if *on {
            accent
        } else if resp.hovered() {
            palette.ink
        } else {
            palette.ink_muted
        };

        if bg.a() > 0 {
            p.rect_filled(rect, CornerRadius::ZERO, bg);
        }
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );

        let draw_text = if label.is_empty() {
            if *on { "[×]".to_string() } else { "[ ]".to_string() }
        } else if *on {
            format!("[×] {label}")
        } else {
            format!("[ ] {label}")
        };
        let galley = p.layout_no_wrap(draw_text, font_id, draw_fg);
        p.galley(
            Pos2::new(rect.left() + 7.0, rect.center().y - galley.size().y * 0.5),
            galley,
            draw_fg,
        );
    }

    resp
}

/// 8-segment discrete pixel RAM meter: Player in `palette.player`, Bot in `palette.bot`, free in `palette.border`.
pub fn term_pixel_ram_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    player_frac: f32,
    bot_frac: f32,
    width: f32,
) {
    const SEGMENTS: usize = 8;
    let gap = 2.0_f32;
    let height = 8.0_f32;
    let total_w = width.max(40.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let p_blocks = ((player_frac.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize).min(SEGMENTS);
    let b_blocks = ((bot_frac.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize)
        .min(SEGMENTS.saturating_sub(p_blocks));

    let seg_w = ((total_w - gap * (SEGMENTS as f32 - 1.0)) / SEGMENTS as f32).max(2.0);
    let painter = ui.painter();
    for i in 0..SEGMENTS {
        let x0 = rect.left() + i as f32 * (seg_w + gap);
        let seg_rect = Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(seg_w, height));
        let col = if i < p_blocks {
            palette.player
        } else if i < p_blocks + b_blocks {
            palette.bot
        } else {
            palette.border
        };
        painter.rect_filled(seg_rect, CornerRadius::ZERO, col);
    }
}

/// 10-segment discrete pixel progress bar (`[████████░░]`).
pub fn term_progress_bar(ui: &mut egui::Ui, palette: &Palette, frac: f32, width: f32) {
    const SEGMENTS: usize = 10;
    let gap = 2.0_f32;
    let height = 8.0_f32;
    let total_w = width.max(50.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let filled = ((frac.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize).min(SEGMENTS);
    let active_col = if frac >= 1.0 { palette.mr } else { palette.focus };
    let seg_w = ((total_w - gap * (SEGMENTS as f32 - 1.0)) / SEGMENTS as f32).max(2.0);
    let painter = ui.painter();
    for i in 0..SEGMENTS {
        let x0 = rect.left() + i as f32 * (seg_w + gap);
        let seg_rect = Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(seg_w, height));
        let col = if i < filled { active_col } else { palette.border };
        painter.rect_filled(seg_rect, CornerRadius::ZERO, col);
    }
}

/// Multi-color 10x10 pixel emblem: focus top/left corner frame, warm-sand crown,
/// surface body, cyan left eye + orchid right eye.
pub fn paint_pixel_emblem(ui: &mut egui::Ui, palette: &Palette, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let p = ui.painter();
    let cell = size / 10.0;
    let px = |gx: u8, gy: u8, gw: u8, gh: u8, col: Color32| {
        let r = Rect::from_min_max(
            Pos2::new(rect.left() + gx as f32 * cell, rect.top() + gy as f32 * cell),
            Pos2::new(
                rect.left() + (gx + gw) as f32 * cell,
                rect.top() + (gy + gh) as f32 * cell,
            ),
        );
        p.rect_filled(r, CornerRadius::ZERO, col);
    };

    // Background
    p.rect_filled(rect, CornerRadius::ZERO, palette.bg);
    // Outer frame (top/left focus, bottom/right border_hi)
    px(0, 0, 10, 1, palette.focus);
    px(0, 0, 1, 10, palette.focus);
    px(0, 9, 10, 1, palette.border_hi);
    px(9, 0, 1, 10, palette.border_hi);
    // Warm sand crown + surface cube body
    px(2, 2, 6, 2, palette.meta);
    px(2, 4, 6, 4, palette.surface);
    // Twin dual-tone eyes: Cyan (Player) + Orchid (Bot) + muted mouth bar
    px(3, 5, 1, 1, palette.player);
    px(6, 5, 1, 1, palette.bot);
    px(3, 7, 4, 1, palette.ink_muted);
}

/// Crisp 64x64 pixel-art window/taskbar icon matching the multi-color pixel emblem.
pub fn build_window_icon() -> IconData {
    const N: usize = 64;
    let p = Palette::for_preset(ThemePreset::SynthTerminal);
    let mut rgba_buf = vec![0u8; N * N * 4];

    // Map 10x10 pixel grid cleanly into 60x60 centered inside 64x64 with a 2px outer bg margin
    for y in 0..N {
        for x in 0..N {
            let color = if !(2..62).contains(&x) || !(2..62).contains(&y) {
                p.bg
            } else {
                let gx = (x - 2) / 6; // 0..10
                let gy = (y - 2) / 6; // 0..10

                if gy == 9 || gx == 9 {
                    p.border_hi
                } else if gy == 0 || gx == 0 {
                    p.focus
                } else if (2..8).contains(&gx) && (2..4).contains(&gy) {
                    p.meta
                } else if gx == 3 && gy == 5 {
                    p.player
                } else if gx == 6 && gy == 5 {
                    p.bot
                } else if (3..7).contains(&gx) && gy == 7 {
                    p.ink_muted
                } else if (2..8).contains(&gx) && (4..8).contains(&gy) {
                    p.surface
                } else {
                    p.bg
                }
            };

            let idx = (y * N + x) * 4;
            rgba_buf[idx] = color.r();
            rgba_buf[idx + 1] = color.g();
            rgba_buf[idx + 2] = color.b();
            rgba_buf[idx + 3] = 255;
        }
    }

    IconData {
        rgba: rgba_buf,
        width: N as u32,
        height: N as u32,
    }
}

/// Square 0px-radius avatar tile with 1px border and 2-char uppercase initials.
pub fn paint_avatar_tile(
    ui: &mut egui::Ui,
    label: &str,
    fg: Color32,
    bg: Color32,
    size: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, bg);
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0_f32, fg),
        StrokeKind::Inside,
    );

    let initials: String = label
        .chars()
        .take(2)
        .collect::<String>()
        .to_ascii_uppercase();
    let text = if initials.is_empty() { "?" } else { &initials };
    let galley = p.layout_no_wrap(
        text.to_string(),
        egui::FontId::monospace((size * 0.36).max(10.0)),
        fg,
    );
    p.galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        fg,
    );
}

/// Square 0px-radius project/mod icon placeholder (`palette.bg` fill, `accent` border & initials).
pub fn paint_icon_placeholder(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    accent: Color32,
    size: f32,
) {
    paint_avatar_tile(ui, title, accent, palette.bg, size);
}

/// Theme preset swatch card: 6 pixel color blocks + preset name + subtitle + `[×]` mark.
pub fn theme_swatch_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    preset: ThemePreset,
    selected: bool,
) -> Response {
    let desired = Vec2::new(186.0, 74.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let border = if selected {
            palette.focus
        } else if resp.hovered() {
            palette.ink_dim
        } else {
            palette.border_hi
        };
        let fill = if resp.hovered() && !selected {
            palette.surface
        } else {
            palette.bg
        };

        p.rect_filled(rect, CornerRadius::ZERO, fill);
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );

        let colors = preset.swatch_colors();
        let pad = 10.0;
        let gap = 4.0;
        let inner_w = rect.width() - pad * 2.0;
        let small_w = 16.0;
        let bg_w = (inner_w - small_w * 5.0 - gap * 5.0).max(16.0);
        let block_h = 12.0;
        let top_y = rect.top() + pad;

        let mut x = rect.left() + pad;
        for (i, &col) in colors.iter().enumerate() {
            let w = if i == 0 { bg_w } else { small_w };
            let r = Rect::from_min_size(Pos2::new(x, top_y), Vec2::new(w, block_h));
            p.rect_filled(r, CornerRadius::ZERO, col);
            if i == 0 {
                p.rect_stroke(
                    r,
                    CornerRadius::ZERO,
                    Stroke::new(1.0_f32, palette.border_hi),
                    StrokeKind::Inside,
                );
            }
            x += w + gap;
        }

        let name_galley = p.layout_no_wrap(
            preset.name().to_string(),
            egui::FontId::monospace(12.0),
            palette.ink,
        );
        p.galley(
            Pos2::new(rect.left() + pad, rect.top() + 30.0),
            name_galley,
            palette.ink,
        );

        if selected {
            let mark_g = p.layout_no_wrap(
                "[×]".to_string(),
                egui::FontId::monospace(11.5),
                palette.focus,
            );
            p.galley(
                Pos2::new(rect.right() - pad - mark_g.size().x, rect.top() + 30.0),
                mark_g,
                palette.focus,
            );
        }

        let sub_galley = p.layout_no_wrap(
            preset.subtitle().to_string(),
            egui::FontId::monospace(10.5),
            palette.ink_muted,
        );
        p.galley(
            Pos2::new(rect.left() + pad, rect.top() + 49.0),
            sub_galley,
            palette.ink_muted,
        );
    }

    resp
}

/// Terminal section header: `── TITLE` in uppercase bold monospace.
pub fn section_header(ui: &mut egui::Ui, _palette: &Palette, title: &str, accent: Color32) {
    ui.label(
        egui::RichText::new(format!("── {}", title.to_uppercase()))
            .monospace()
            .size(11.5)
            .strong()
            .color(accent),
    );
}

/// Crisp 1px horizontal terminal divider.
pub fn term_divider(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, palette.border);
    ui.add_space(4.0);
}

fn paint_pixel_nav_icon(p: &egui::Painter, rect: Rect, icon: NavIcon, color: Color32) {
    let cell = rect.width() / 10.0;
    let px = |gx: u8, gy: u8, gw: u8, gh: u8| {
        let r = Rect::from_min_max(
            Pos2::new(rect.left() + gx as f32 * cell, rect.top() + gy as f32 * cell),
            Pos2::new(
                rect.left() + (gx + gw) as f32 * cell,
                rect.top() + (gy + gh) as f32 * cell,
            ),
        );
        p.rect_filled(r, CornerRadius::ZERO, color);
    };

    match icon {
        NavIcon::Instances => {
            px(1, 1, 3, 3);
            px(6, 1, 3, 3);
            px(1, 6, 3, 3);
            px(6, 6, 3, 3);
        }
        NavIcon::Collections => {
            px(2, 1, 6, 1);
            px(1, 3, 8, 1);
            px(1, 5, 8, 4);
        }
        NavIcon::Discover => {
            px(2, 1, 6, 1);
            px(1, 2, 1, 6);
            px(8, 2, 1, 6);
            px(2, 8, 6, 1);
            px(4, 3, 2, 4);
            px(3, 4, 4, 2);
        }
        NavIcon::Accounts => {
            px(3, 1, 4, 4);
            px(1, 6, 8, 3);
        }
        NavIcon::Console => {
            px(1, 1, 8, 1);
            px(1, 8, 8, 1);
            px(1, 1, 1, 8);
            px(8, 1, 1, 8);
            px(2, 3, 1, 1);
            px(3, 4, 1, 1);
            px(2, 5, 1, 1);
            px(5, 6, 3, 1);
        }
        NavIcon::Config => {
            px(1, 2, 8, 1);
            px(3, 1, 2, 3);
            px(1, 5, 8, 1);
            px(6, 4, 2, 3);
            px(1, 8, 8, 1);
            px(2, 7, 2, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_window_icon_are_valid() {
        assert_eq!(ThemePreset::ALL.len(), 4);
        for preset in ThemePreset::ALL {
            assert_eq!(ThemePreset::from_id(preset.id()), preset);
            let sw = preset.swatch_colors();
            let pal = Palette::for_preset(preset);
            assert_eq!(sw[0], pal.bg);
            assert_eq!(sw[1], pal.focus);
            assert_eq!(sw[2], pal.bot);
            assert_eq!(sw[3], pal.player);
            assert_eq!(sw[4], pal.mr);
            assert_eq!(sw[5], pal.cf);
        }
        assert!(!ThemePreset::SynthTerminal.is_light());
        assert!(ThemePreset::DraftingPaper.is_light());

        let icon = build_window_icon();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
    }
}
