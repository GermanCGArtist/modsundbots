use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};

use crate::theme::{self, GlassElevation, Palette};

#[derive(Debug, Clone)]
enum InlineSpan {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Link { label: String, url: String },
}

#[derive(Debug, Clone)]
struct BadgeLink {
    label: String,
    url: String,
    accent: Color32,
}

#[derive(Debug, Clone)]
enum Block {
    Badges(Vec<BadgeLink>),
    Heading { level: u8, spans: Vec<InlineSpan> },
    Paragraph(Vec<InlineSpan>),
    Bullet(Vec<InlineSpan>),
    Quote(Vec<InlineSpan>),
    Code(String),
    Rule,
}

/// Renders a Modrinth project body (Markdown with embedded HTML badges/tags)
/// cleanly using Apple Liquid Glass typography, clickable links, code wells,
/// and frosted badge pills.
pub fn render_markdown(ui: &mut egui::Ui, palette: &Palette, raw: &str) {
    let blocks = parse_blocks(raw, palette);

    for (idx, block) in blocks.iter().enumerate() {
        if idx > 0 {
            ui.add_space(6.0);
        }
        match block {
            Block::Badges(badges) => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    for b in badges {
                        render_badge_link(ui, b);
                    }
                });
            }
            Block::Heading { level, spans } => {
                ui.add_space(4.0);
                let size = match level {
                    1 => 19.0,
                    2 => 16.5,
                    3 => 14.5,
                    _ => 13.5,
                };
                render_inline_spans(ui, palette, spans, size, true);
                if *level <= 2 {
                    ui.add_space(2.0);
                    theme::glass_divider(ui, palette);
                }
            }
            Block::Paragraph(spans) => {
                render_inline_spans(ui, palette, spans, 12.8, false);
            }
            Block::Bullet(spans) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("•")
                            .size(13.0)
                            .strong()
                            .color(palette.accent_primary),
                    );
                    ui.vertical(|ui| {
                        render_inline_spans(ui, palette, spans, 12.6, false);
                    });
                });
            }
            Block::Quote(spans) => {
                egui::Frame::NONE
                    .inner_margin(Margin {
                        left: 12,
                        right: 6,
                        top: 4,
                        bottom: 4,
                    })
                    .show(ui, |ui| {
                        let resp = ui.vertical(|ui| {
                            render_inline_spans(ui, palette, spans, 12.5, false);
                        });
                        let r = resp.response.rect;
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(r.left() - 10.0, r.top()),
                            egui::pos2(r.left() - 7.0, r.bottom()),
                        );
                        ui.painter()
                            .rect_filled(bar, CornerRadius::same(2), palette.accent_primary);
                    });
            }
            Block::Code(code) => {
                theme::glass_box(
                    ui,
                    palette,
                    GlassElevation::Well,
                    12,
                    Margin::same(12),
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(code)
                                    .monospace()
                                    .size(11.8)
                                    .color(palette.text_primary),
                            )
                            .wrap(),
                        );
                    },
                );
            }
            Block::Rule => {
                ui.add_space(2.0);
                theme::glass_divider(ui, palette);
            }
        }
    }
}

fn render_badge_link(ui: &mut egui::Ui, badge: &BadgeLink) {
    let bg = Color32::from_rgba_unmultiplied(badge.accent.r(), badge.accent.g(), badge.accent.b(), 32);
    let stroke = Stroke::new(
        1.0_f32,
        Color32::from_rgba_unmultiplied(badge.accent.r(), badge.accent.g(), badge.accent.b(), 120),
    );
    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(11, 4))
        .show(ui, |ui| {
            ui.hyperlink_to(
                RichText::new(&badge.label)
                    .size(12.0)
                    .strong()
                    .color(badge.accent),
                &badge.url,
            );
        });
}

fn render_inline_spans(
    ui: &mut egui::Ui,
    palette: &Palette,
    spans: &[InlineSpan],
    font_size: f32,
    force_bold: bool,
) {
    // Fast path: if there are no links, render as a single smoothly wrapped LayoutJob
    // so text wraps naturally at any word boundary without awkward widget gaps.
    let has_links = spans.iter().any(|s| matches!(s, InlineSpan::Link { .. }));
    if !has_links {
        let mut job = egui::text::LayoutJob::default();
        for span in spans {
            append_span_to_job(&mut job, span, palette, font_size, force_bold);
        }
        ui.add(egui::Label::new(job).wrap());
        return;
    }

    // Mixed text + clickable links: wrap cleanly with tight horizontal spacing
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.spacing_mut().item_spacing.y = 3.0;
        for span in spans {
            match span {
                InlineSpan::Text(t) => {
                    if !t.is_empty() {
                        let mut rt = RichText::new(t).size(font_size).color(palette.text_primary);
                        if force_bold {
                            rt = rt.strong();
                        }
                        ui.label(rt);
                    }
                }
                InlineSpan::Bold(t) => {
                    if !t.is_empty() {
                        ui.label(
                            RichText::new(t)
                                .size(font_size)
                                .strong()
                                .color(palette.text_primary),
                        );
                    }
                }
                InlineSpan::Italic(t) => {
                    if !t.is_empty() {
                        ui.label(
                            RichText::new(t)
                                .size(font_size)
                                .italics()
                                .color(palette.text_secondary),
                        );
                    }
                }
                InlineSpan::Code(t) => {
                    if !t.is_empty() {
                        ui.label(
                            RichText::new(t)
                                .size(font_size - 0.6)
                                .monospace()
                                .color(palette.accent_cyan)
                                .background_color(palette.well_fill),
                        );
                    }
                }
                InlineSpan::Link { label, url } => {
                    let clean_lbl = label.trim_matches('`');
                    let mut rt = RichText::new(clean_lbl)
                        .size(font_size)
                        .color(palette.accent_primary);
                    if label.starts_with('`') && label.ends_with('`') {
                        rt = rt.monospace();
                    }
                    if force_bold {
                        rt = rt.strong();
                    }
                    ui.hyperlink_to(rt, url);
                }
            }
        }
    });
}

fn append_span_to_job(
    job: &mut egui::text::LayoutJob,
    span: &InlineSpan,
    palette: &Palette,
    font_size: f32,
    force_bold: bool,
) {
    match span {
        InlineSpan::Text(t) => {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(font_size),
                    color: if force_bold {
                        palette.text_primary
                    } else {
                        palette.text_secondary
                    },
                    ..Default::default()
                },
            );
        }
        InlineSpan::Bold(t) => {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(font_size),
                    color: palette.text_primary,
                    ..Default::default()
                },
            );
        }
        InlineSpan::Italic(t) => {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(font_size),
                    italics: true,
                    color: palette.text_secondary,
                    ..Default::default()
                },
            );
        }
        InlineSpan::Code(t) => {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::monospace(font_size - 0.6),
                    color: palette.accent_cyan,
                    background: palette.well_fill,
                    ..Default::default()
                },
            );
        }
        InlineSpan::Link { label, .. } => {
            job.append(
                label,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(font_size),
                    color: palette.accent_primary,
                    underline: Stroke::new(1.0_f32, palette.accent_primary),
                    ..Default::default()
                },
            );
        }
    }
}

fn parse_blocks(raw: &str, palette: &Palette) -> Vec<Block> {
    // Step 1: Preprocess embedded HTML (extract badge links, convert HTML tags to Markdown/newlines)
    let (extracted_badges, cleaned) = preprocess_html(raw, palette);

    let mut blocks = Vec::new();
    if !extracted_badges.is_empty() {
        blocks.push(Block::Badges(extracted_badges));
    }

    let mut in_code_block = false;
    let mut code_buf = String::new();
    let mut para_buf = String::new();

    let flush_para = |buf: &mut String, out: &mut Vec<Block>| {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            let spans = parse_inline(trimmed);
            if !spans.is_empty() {
                out.push(Block::Paragraph(spans));
            }
        }
        buf.clear();
    };

    for raw_line in cleaned.lines() {
        let line = raw_line.trim_end();

        if line.trim_start().starts_with("```") {
            if in_code_block {
                blocks.push(Block::Code(code_buf.trim_end().to_string()));
                code_buf.clear();
                in_code_block = false;
            } else {
                flush_para(&mut para_buf, &mut blocks);
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(line);
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush_para(&mut para_buf, &mut blocks);
            continue;
        }

        // Horizontal rule
        if is_horizontal_rule(trimmed) {
            flush_para(&mut para_buf, &mut blocks);
            blocks.push(Block::Rule);
            continue;
        }

        // Headings (# .. ######)
        if let Some((level, rest)) = parse_heading_line(trimmed) {
            flush_para(&mut para_buf, &mut blocks);
            let spans = parse_inline(rest);
            if !spans.is_empty() {
                blocks.push(Block::Heading { level, spans });
            }
            continue;
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            flush_para(&mut para_buf, &mut blocks);
            let spans = parse_inline(rest.trim());
            if !spans.is_empty() {
                blocks.push(Block::Quote(spans));
            }
            continue;
        }

        // Unordered or ordered list item
        if let Some(rest) = strip_list_prefix(trimmed) {
            flush_para(&mut para_buf, &mut blocks);
            let spans = parse_inline(rest);
            if !spans.is_empty() {
                blocks.push(Block::Bullet(spans));
            }
            continue;
        }

        // Standalone Markdown image line `![alt](url)` -> skip decorative banners
        if trimmed.starts_with("![") && trimmed.ends_with(')') && !trimmed.contains("](") {
            continue;
        }

        // Accumulate paragraph lines
        if !para_buf.is_empty() {
            para_buf.push(' ');
        }
        para_buf.push_str(trimmed);
    }

    if in_code_block && !code_buf.trim().is_empty() {
        blocks.push(Block::Code(code_buf.trim_end().to_string()));
    }
    flush_para(&mut para_buf, &mut blocks);

    blocks
}

/// Converts HTML fragments common in Modrinth descriptions (`<a href><img/></a>`,
/// `<br>`, `<img/>`, `<h1>`, `<b>`, `<i>`, `<code>`, `<li>`, etc.) into clean
/// Markdown text plus extracted top-level link badges.
fn preprocess_html(raw: &str, palette: &Palette) -> (Vec<BadgeLink>, String) {
    let mut badges: Vec<BadgeLink> = Vec::new();
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after_lt = &rest[lt..];

        // Find closing '>'
        let Some(gt) = after_lt.find('>') else {
            out.push_str(after_lt);
            return (badges, decode_entities(&out));
        };

        let tag_full = &after_lt[1..gt];
        let tag_trimmed = tag_full.trim();
        let tag_lower = tag_trimmed.to_ascii_lowercase();

        // Handle `<a href="...">...</a>`
        if tag_lower.starts_with("a ") || tag_lower == "a" {
            let after_open = &after_lt[gt + 1..];
            if let Some(close_idx) = find_closing_tag_ci(after_open, "</a>") {
                let inner = &after_open[..close_idx];
                let href = extract_attr(tag_trimmed, "href").unwrap_or_default();
                let inner_lower = inner.to_ascii_lowercase();

                if !href.is_empty() {
                    if inner_lower.contains("<img") {
                        // Image-wrapped link (e.g. Discord / PayPal / Patreon shields.io badge)
                        let alt = extract_attr(inner, "alt").unwrap_or_default();
                        let (label, accent) = classify_badge(&href, &alt, palette);
                        if !badges.iter().any(|b| b.url == href) {
                            badges.push(BadgeLink {
                                label,
                                url: href,
                                accent,
                            });
                        }
                    } else {
                        let text = strip_all_tags(inner).trim().to_string();
                        if !text.is_empty() {
                            out.push('[');
                            out.push_str(&text);
                            out.push_str("](");
                            out.push_str(&href);
                            out.push(')');
                        }
                    }
                }
                rest = &after_open[close_idx + 4..];
                continue;
            }
        }

        // Convert common structural & inline HTML tags
        if tag_lower.starts_with("br") {
            out.push('\n');
        } else if tag_lower.starts_with("hr") {
            out.push_str("\n---\n");
        } else if tag_lower.starts_with("h1") {
            out.push_str("\n# ");
        } else if tag_lower.starts_with("h2") {
            out.push_str("\n## ");
        } else if tag_lower.starts_with("h3") {
            out.push_str("\n### ");
        } else if tag_lower.starts_with("/h1")
            || tag_lower.starts_with("/h2")
            || tag_lower.starts_with("/h3")
            || tag_lower.starts_with("/p")
            || tag_lower.starts_with("/div")
            || tag_lower.starts_with("/summary")
            || tag_lower.starts_with("/center")
        {
            out.push_str("\n\n");
        } else if tag_lower.starts_with("li") {
            out.push_str("\n- ");
        } else if tag_lower == "b"
            || tag_lower == "/b"
            || tag_lower == "strong"
            || tag_lower == "/strong"
        {
            out.push_str("**");
        } else if tag_lower == "i"
            || tag_lower == "/i"
            || tag_lower == "em"
            || tag_lower == "/em"
        {
            out.push('*');
        } else if tag_lower == "code" || tag_lower == "/code" {
            out.push('`');
        }
        // Ignore `<img>`, `<p>`, `<div>`, `<center>`, `<details>`, `<ul>`, `</ol>`, etc.

        rest = &after_lt[gt + 1..];
    }

    out.push_str(rest);
    (badges, decode_entities(&out))
}

fn find_closing_tag_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower.find(needle_lower)
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{attr_name}=");
    let pos = lower.find(&needle)?;
    let after = tag[pos + needle.len()..].trim_start();
    if let Some(quoted) = after.strip_prefix('"') {
        let end = quoted.find('"')?;
        Some(quoted[..end].to_string())
    } else if let Some(quoted) = after.strip_prefix('\'') {
        let end = quoted.find('\'')?;
        Some(quoted[..end].to_string())
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

fn strip_all_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn classify_badge(url: &str, alt: &str, palette: &Palette) -> (String, Color32) {
    let u = url.to_ascii_lowercase();
    if u.contains("discord.gg") || u.contains("discord.com") {
        return ("Discord".to_string(), Color32::from_rgb(124, 140, 255));
    }
    if u.contains("patreon.com") {
        return ("Patreon".to_string(), Color32::from_rgb(255, 118, 96));
    }
    if u.contains("paypal.me") || u.contains("paypal.com") {
        return ("PayPal".to_string(), palette.accent_blue);
    }
    if u.contains("ko-fi.com") {
        return ("Ko-fi".to_string(), Color32::from_rgb(255, 122, 138));
    }
    if u.contains("github.com") {
        return ("GitHub".to_string(), palette.text_primary);
    }
    if u.contains("modrinth.com") {
        return ("Modrinth".to_string(), palette.accent_emerald);
    }
    if u.contains("curseforge.com") {
        return ("CurseForge".to_string(), palette.accent_amber);
    }
    if u.contains("youtube.com") || u.contains("youtu.be") {
        return ("YouTube".to_string(), palette.accent_red);
    }
    if !alt.trim().is_empty() {
        return (alt.trim().to_string(), palette.accent_primary);
    }
    // Fall back to clean hostname
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("Link")
        .trim_start_matches("www.");
    (host.to_string(), palette.accent_primary)
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let first = trimmed.chars().next().unwrap_or(' ');
    matches!(first, '-' | '*' | '_') && trimmed.chars().all(|c| c == first || c == ' ')
}

fn parse_heading_line(line: &str) -> Option<(u8, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) {
        let rest = line[hashes..].trim_start();
        if !rest.is_empty() {
            return Some((hashes as u8, rest));
        }
    }
    None
}

fn strip_list_prefix(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(rest.trim());
    }
    // Numbered list e.g. "1. Item"
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return Some(line[digits + 2..].trim());
    }
    None
}

/// Parses inline Markdown: `[label](url)`, `![alt](url)`, `**bold**`, `*italic*`, `` `code` ``.
fn parse_inline(input: &str) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    let mut text_buf = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    let flush_text = |buf: &mut String, out: &mut Vec<InlineSpan>| {
        if !buf.is_empty() {
            out.push(InlineSpan::Text(std::mem::take(buf)));
        }
    };

    while i < bytes.len() {
        let rest = &input[i..];

        // Markdown image `![alt](url)` -> skip inline decorative images
        if rest.starts_with("![")
            && let Some(close_br) = rest[2..].find("](")
        {
            let after_br = &rest[2 + close_br + 2..];
            if let Some(close_par) = after_br.find(')') {
                i += 2 + close_br + 2 + close_par + 1;
                continue;
            }
        }

        // Markdown link `[label](url)`
        if rest.starts_with('[')
            && let Some(close_br) = rest[1..].find("](")
        {
            let label = &rest[1..1 + close_br];
            let after_br = &rest[1 + close_br + 2..];
            if let Some(close_par) = after_br.find(')') {
                let url = &after_br[..close_par];
                if !url.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
                    flush_text(&mut text_buf, &mut spans);
                    spans.push(InlineSpan::Link {
                        label: label.to_string(),
                        url: url.to_string(),
                    });
                    i += 1 + close_br + 2 + close_par + 1;
                    continue;
                }
            }
        }

        // Bold `**...**`
        if rest.starts_with("**")
            && let Some(end) = rest[2..].find("**")
            && end > 0
        {
            flush_text(&mut text_buf, &mut spans);
            spans.push(InlineSpan::Bold(rest[2..2 + end].to_string()));
            i += 2 + end + 2;
            continue;
        }

        // Inline code `` `...` ``
        if rest.starts_with('`')
            && let Some(end) = rest[1..].find('`')
            && end > 0
        {
            flush_text(&mut text_buf, &mut spans);
            spans.push(InlineSpan::Code(rest[1..1 + end].to_string()));
            i += 1 + end + 1;
            continue;
        }

        // Italic `*...*`
        if rest.starts_with('*')
            && !rest.starts_with("**")
            && let Some(end) = rest[1..].find('*')
            && end > 0
        {
            flush_text(&mut text_buf, &mut spans);
            spans.push(InlineSpan::Italic(rest[1..1 + end].to_string()));
            i += 1 + end + 1;
            continue;
        }

        // Regular UTF-8 character
        let ch = rest.chars().next().unwrap();
        text_buf.push(ch);
        i += ch.len_utf8();
    }

    flush_text(&mut text_buf, &mut spans);
    spans
}
