use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Stroke};

use crate::theme::{self, Palette};

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
struct TableCell {
    is_header: bool,
    spans: Vec<InlineSpan>,
}

#[derive(Debug, Clone)]
struct TableBlock {
    rows: Vec<Vec<TableCell>>,
}

#[derive(Debug, Clone)]
enum Block {
    Badges(Vec<BadgeLink>),
    Heading { level: u8, spans: Vec<InlineSpan> },
    Paragraph(Vec<InlineSpan>),
    Bullet(Vec<InlineSpan>),
    Quote(Vec<InlineSpan>),
    Code(String),
    Table(TableBlock),
    Rule,
}

const TABLE_MARKER_PREFIX: &str = "\x1FMUB_TABLE:";
const TABLE_MARKER_SUFFIX: &str = "\x1F";

/// Renders a Modrinth Markdown or CurseForge HTML project body using sharp
/// terminal typography, clickable links, code boxes, badge tags, and tables.
pub fn render_markdown(ui: &mut egui::Ui, palette: &Palette, raw: &str) {
    let blocks = parse_blocks(raw, palette);

    for (idx, block) in blocks.iter().enumerate() {
        if idx > 0 {
            ui.add_space(6.0);
        }
        match block {
            Block::Badges(badges) => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for b in badges {
                        render_badge_link(ui, palette, b);
                    }
                });
            }
            Block::Heading { level, spans } => {
                ui.add_space(4.0);
                let size = match level {
                    1 => 17.0,
                    2 => 15.0,
                    3 => 13.5,
                    _ => 12.5,
                };
                render_inline_spans(ui, palette, spans, size, true);
                if *level <= 2 {
                    ui.add_space(2.0);
                    theme::term_divider(ui, palette);
                }
            }
            Block::Paragraph(spans) => {
                render_inline_spans(ui, palette, spans, 12.2, false);
            }
            Block::Bullet(spans) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("▪")
                            .monospace()
                            .size(12.2)
                            .strong()
                            .color(palette.focus),
                    );
                    ui.vertical(|ui| {
                        render_inline_spans(ui, palette, spans, 12.0, false);
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
                            render_inline_spans(ui, palette, spans, 12.0, false);
                        });
                        let r = resp.response.rect;
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(r.left() - 10.0, r.top()),
                            egui::pos2(r.left() - 8.0, r.bottom()),
                        );
                        ui.painter()
                            .rect_filled(bar, CornerRadius::ZERO, palette.focus);
                    });
            }
            Block::Code(code) => {
                theme::term_box(
                    ui,
                    palette.bg,
                    palette.border_hi,
                    Margin::same(10),
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(code)
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.ink),
                            )
                            .wrap(),
                        );
                    },
                );
            }
            Block::Table(table) => {
                render_table(ui, palette, table);
            }
            Block::Rule => {
                ui.add_space(2.0);
                theme::term_divider(ui, palette);
            }
        }
    }
}

fn render_table(ui: &mut egui::Ui, palette: &Palette, table: &TableBlock) {
    let num_cols = table.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return;
    }

    theme::term_box(
        ui,
        palette.surface,
        palette.border_hi,
        Margin::same(0),
        |ui| {
            ui.set_width(ui.available_width());
            for (row_idx, row) in table.rows.iter().enumerate() {
                let is_header = row.iter().any(|c| c.is_header);
                let row_bg = if is_header {
                    palette.raised
                } else if row_idx % 2 == 1 {
                    palette.panel
                } else {
                    palette.surface
                };

                egui::Frame::new()
                    .fill(row_bg)
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.columns(num_cols, |cols| {
                            for (col_idx, col_ui) in cols.iter_mut().enumerate() {
                                if let Some(cell) = row.get(col_idx) {
                                    if cell.spans.is_empty() {
                                        col_ui.label(
                                            RichText::new(" ")
                                                .monospace()
                                                .size(11.5)
                                                .color(palette.ink_dim),
                                        );
                                    } else {
                                        render_inline_spans(
                                            col_ui,
                                            palette,
                                            &cell.spans,
                                            11.5,
                                            is_header,
                                        );
                                    }
                                }
                            }
                        });
                    });

                if row_idx + 1 < table.rows.len() {
                    let (div_rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 1.0),
                        Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(div_rect, CornerRadius::ZERO, palette.border);
                }
            }
        },
    );
}

fn render_badge_link(ui: &mut egui::Ui, palette: &Palette, badge: &BadgeLink) {
    let bg = if palette.preset.is_light() {
        Color32::from_rgba_unmultiplied(badge.accent.r(), badge.accent.g(), badge.accent.b(), 24)
    } else {
        Color32::from_rgba_unmultiplied(badge.accent.r(), badge.accent.g(), badge.accent.b(), 30)
    };
    let stroke = Stroke::new(1.0_f32, badge.accent);
    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.hyperlink_to(
                RichText::new(&badge.label)
                    .monospace()
                    .size(11.5)
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
    let has_links = spans.iter().any(|s| matches!(s, InlineSpan::Link { .. }));
    if !has_links {
        let mut job = egui::text::LayoutJob::default();
        for span in spans {
            append_span_to_job(&mut job, span, palette, font_size, force_bold);
        }
        ui.add(egui::Label::new(job).wrap());
        return;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.spacing_mut().item_spacing.y = 3.0;
        for span in spans {
            match span {
                InlineSpan::Text(t) => {
                    if !t.is_empty() {
                        let mut rt = RichText::new(t).monospace().size(font_size).color(if force_bold {
                            palette.ink
                        } else {
                            palette.ink_dim
                        });
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
                                .monospace()
                                .size(font_size)
                                .strong()
                                .color(palette.ink),
                        );
                    }
                }
                InlineSpan::Italic(t) => {
                    if !t.is_empty() {
                        ui.label(
                            RichText::new(t)
                                .monospace()
                                .size(font_size)
                                .italics()
                                .color(palette.ink_dim),
                        );
                    }
                }
                InlineSpan::Code(t) => {
                    if !t.is_empty() {
                        ui.label(
                            RichText::new(t)
                                .size(font_size - 0.5)
                                .monospace()
                                .color(palette.meta)
                                .background_color(palette.bg),
                        );
                    }
                }
                InlineSpan::Link { label, url } => {
                    let clean_lbl = label.trim_matches('`');
                    let mut rt = RichText::new(clean_lbl)
                        .monospace()
                        .size(font_size)
                        .color(palette.player);
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
                    font_id: egui::FontId::monospace(font_size),
                    color: if force_bold {
                        palette.ink
                    } else {
                        palette.ink_dim
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
                    font_id: egui::FontId::monospace(font_size),
                    color: palette.ink,
                    ..Default::default()
                },
            );
        }
        InlineSpan::Italic(t) => {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::monospace(font_size),
                    italics: true,
                    color: palette.ink_dim,
                    ..Default::default()
                },
            );
        }
        InlineSpan::Code(t) => {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::monospace(font_size - 0.5),
                    color: palette.meta,
                    background: palette.bg,
                    ..Default::default()
                },
            );
        }
        InlineSpan::Link { label, .. } => {
            job.append(
                label,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::monospace(font_size),
                    color: palette.player,
                    underline: Stroke::new(1.0_f32, palette.player),
                    ..Default::default()
                },
            );
        }
    }
}

fn parse_blocks(raw: &str, palette: &Palette) -> Vec<Block> {
    let (extracted_badges, tables, cleaned) = preprocess_html(raw, palette);

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
            let has_visible = spans.iter().any(|s| match s {
                InlineSpan::Text(t) => !t.trim().is_empty(),
                _ => true,
            });
            if has_visible {
                out.push(Block::Paragraph(spans));
            }
        }
        buf.clear();
    };

    let lines: Vec<&str> = cleaned.lines().collect();
    let mut idx = 0;

    while idx < lines.len() {
        let line = lines[idx].trim_end();

        if line.trim_start().starts_with("```") {
            if in_code_block {
                blocks.push(Block::Code(code_buf.trim_end().to_string()));
                code_buf.clear();
                in_code_block = false;
            } else {
                flush_para(&mut para_buf, &mut blocks);
                in_code_block = true;
            }
            idx += 1;
            continue;
        }

        if in_code_block {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(line);
            idx += 1;
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush_para(&mut para_buf, &mut blocks);
            idx += 1;
            continue;
        }

        // Extracted HTML table placeholder
        if let Some(inner) = trimmed
            .strip_prefix(TABLE_MARKER_PREFIX)
            .and_then(|s| s.strip_suffix(TABLE_MARKER_SUFFIX))
            && let Ok(table_idx) = inner.parse::<usize>()
            && let Some(table) = tables.get(table_idx)
        {
            flush_para(&mut para_buf, &mut blocks);
            if !table.rows.is_empty() {
                blocks.push(Block::Table(table.clone()));
            }
            idx += 1;
            continue;
        }

        // Markdown pipe table: header row + separator row (`| --- | --- |`)
        if trimmed.starts_with('|')
            && idx + 1 < lines.len()
            && is_markdown_table_separator(lines[idx + 1].trim())
        {
            flush_para(&mut para_buf, &mut blocks);
            let mut rows = Vec::new();
            rows.push(parse_pipe_row(trimmed, true));
            idx += 2;
            while idx < lines.len() {
                let row_line = lines[idx].trim();
                if row_line.is_empty() || !row_line.contains('|') {
                    break;
                }
                rows.push(parse_pipe_row(row_line, false));
                idx += 1;
            }
            blocks.push(Block::Table(TableBlock { rows }));
            continue;
        }

        // Horizontal rule
        if is_horizontal_rule(trimmed) {
            flush_para(&mut para_buf, &mut blocks);
            blocks.push(Block::Rule);
            idx += 1;
            continue;
        }

        // Headings (# .. ######)
        if let Some((level, rest)) = parse_heading_line(trimmed) {
            flush_para(&mut para_buf, &mut blocks);
            let spans = parse_inline(rest);
            if !spans.is_empty() {
                blocks.push(Block::Heading { level, spans });
            }
            idx += 1;
            continue;
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            flush_para(&mut para_buf, &mut blocks);
            let spans = parse_inline(rest.trim());
            if !spans.is_empty() {
                blocks.push(Block::Quote(spans));
            }
            idx += 1;
            continue;
        }

        // Unordered or ordered list item
        if let Some(rest) = strip_list_prefix(trimmed) {
            flush_para(&mut para_buf, &mut blocks);
            let spans = parse_inline(rest);
            if !spans.is_empty() {
                blocks.push(Block::Bullet(spans));
            }
            idx += 1;
            continue;
        }

        // Standalone Markdown image line `![alt](url)` -> skip decorative banners
        if is_standalone_markdown_image(trimmed) {
            idx += 1;
            continue;
        }

        // Accumulate paragraph lines
        if !para_buf.is_empty() {
            para_buf.push(' ');
        }
        para_buf.push_str(trimmed);
        idx += 1;
    }

    if in_code_block && !code_buf.trim().is_empty() {
        blocks.push(Block::Code(code_buf.trim_end().to_string()));
    }
    flush_para(&mut para_buf, &mut blocks);

    blocks
}

fn is_standalone_markdown_image(trimmed: &str) -> bool {
    let Some(after_bang) = trimmed.strip_prefix("![") else {
        return false;
    };
    let Some(close_br) = after_bang.find("](") else {
        return false;
    };
    let after_url = &after_bang[close_br + 2..];
    let Some(close_par) = after_url.find(')') else {
        return false;
    };
    close_par + 1 == after_url.len()
}

fn is_markdown_table_separator(line: &str) -> bool {
    if !line.contains('|') || !line.contains('-') {
        return false;
    }
    line.chars()
        .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn parse_pipe_row(line: &str, is_header: bool) -> Vec<TableCell> {
    let trimmed = line.trim().trim_matches('|');
    trimmed
        .split('|')
        .map(|cell| TableCell {
            is_header,
            spans: parse_inline(cell.trim()),
        })
        .collect()
}

/// Converts HTML fragments common in Modrinth & CurseForge descriptions into clean
/// Markdown text plus extracted top-level link badges and structured HTML tables.
fn preprocess_html(raw: &str, palette: &Palette) -> (Vec<BadgeLink>, Vec<TableBlock>, String) {
    let mut badges: Vec<BadgeLink> = Vec::new();
    let mut tables: Vec<TableBlock> = Vec::new();
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after_lt = &rest[lt..];

        // Strip HTML comments `<!-- ... -->`
        if after_lt.starts_with("<!--") {
            if let Some(end_idx) = after_lt.find("-->") {
                rest = &after_lt[end_idx + 3..];
                continue;
            }
            return (badges, tables, decode_entities(&out));
        }

        // Find closing '>'
        let Some(gt) = after_lt.find('>') else {
            out.push_str(after_lt);
            return (badges, tables, decode_entities(&out));
        };

        let tag_full = &after_lt[1..gt];
        let tag_trimmed = tag_full.trim();
        let tag_lower = tag_trimmed.to_ascii_lowercase();
        let tag_name = tag_lower
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');

        // Strip `<style>...</style>` and `<script>...</script>` blocks completely
        if tag_name == "style" || tag_name == "script" {
            let after_open = &after_lt[gt + 1..];
            let close_needle = format!("</{tag_name}>");
            if let Some(close_idx) = find_closing_tag_ci(after_open, &close_needle) {
                rest = &after_open[close_idx + close_needle.len()..];
                continue;
            }
            return (badges, tables, decode_entities(&out));
        }

        // Parse HTML `<table>...</table>` into structured terminal grid
        if tag_name == "table" {
            let after_open = &after_lt[gt + 1..];
            if let Some(close_idx) = find_closing_tag_ci(after_open, "</table>") {
                let table_html = &after_open[..close_idx];
                let table = parse_html_table(table_html);
                if !table.rows.is_empty() {
                    let t_idx = tables.len();
                    tables.push(table);
                    out.push('\n');
                    out.push_str(TABLE_MARKER_PREFIX);
                    out.push_str(&t_idx.to_string());
                    out.push_str(TABLE_MARKER_SUFFIX);
                    out.push('\n');
                }
                rest = &after_open[close_idx + "</table>".len()..];
                continue;
            }
        }

        // Handle `<a href="...">...</a>`
        if tag_name == "a" {
            let after_open = &after_lt[gt + 1..];
            if let Some(close_idx) = find_closing_tag_ci(after_open, "</a>") {
                let inner = &after_open[..close_idx];
                let href = extract_attr(tag_trimmed, "href").unwrap_or_default();
                let inner_lower = inner.to_ascii_lowercase();

                if !href.is_empty() {
                    if inner_lower.contains("<img") {
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
        if tag_name == "br" {
            out.push('\n');
        } else if tag_name == "hr" {
            out.push_str("\n---\n");
        } else if tag_name == "h1" {
            out.push_str("\n# ");
        } else if tag_name == "h2" {
            out.push_str("\n## ");
        } else if tag_name == "h3" {
            out.push_str("\n### ");
        } else if tag_name == "h4" {
            out.push_str("\n#### ");
        } else if tag_name == "h5" {
            out.push_str("\n##### ");
        } else if tag_name == "h6" {
            out.push_str("\n###### ");
        } else if tag_name == "blockquote" {
            out.push_str("\n> ");
        } else if tag_name == "pre" || tag_name == "/pre" {
            out.push_str("\n```\n");
        } else if matches!(
            tag_name,
            "/h1" | "/h2" | "/h3" | "/h4" | "/h5" | "/h6" | "/p" | "/div" | "/summary"
                | "/center" | "/blockquote" | "/ul" | "/ol"
        ) {
            out.push_str("\n\n");
        } else if tag_name == "li" {
            out.push_str("\n- ");
        } else if matches!(tag_name, "b" | "/b" | "strong" | "/strong") {
            out.push_str("**");
        } else if matches!(tag_name, "i" | "/i" | "em" | "/em") {
            out.push('*');
        } else if matches!(tag_name, "code" | "/code") {
            out.push('`');
        }

        rest = &after_lt[gt + 1..];
    }

    out.push_str(rest);
    (badges, tables, decode_entities(&out))
}

fn parse_html_table(table_html: &str) -> TableBlock {
    let mut rows = Vec::new();
    let mut rest = table_html;

    while let Some(tr_start) = find_tag_start_ci(rest, "tr") {
        let after_tr = &rest[tr_start..];
        let Some(tr_gt) = after_tr.find('>') else {
            break;
        };
        let after_tr_open = &after_tr[tr_gt + 1..];
        let tr_end = find_closing_tag_ci(after_tr_open, "</tr>").unwrap_or(after_tr_open.len());
        let row_html = &after_tr_open[..tr_end];

        let cells = parse_html_row_cells(row_html);
        if !cells.is_empty() {
            rows.push(cells);
        }

        let advance = (tr_end + "</tr>".len()).min(after_tr_open.len());
        rest = &after_tr_open[advance..];
    }

    TableBlock { rows }
}

fn parse_html_row_cells(row_html: &str) -> Vec<TableCell> {
    let mut cells = Vec::new();
    let mut rest = row_html;

    loop {
        let next_th = find_tag_start_ci(rest, "th");
        let next_td = find_tag_start_ci(rest, "td");
        let (cell_start, is_header, close_tag) = match (next_th, next_td) {
            (Some(a), Some(b)) if a <= b => (a, true, "</th>"),
            (Some(_), Some(b)) => (b, false, "</td>"),
            (Some(a), None) => (a, true, "</th>"),
            (None, Some(b)) => (b, false, "</td>"),
            (None, None) => break,
        };

        let after_tag = &rest[cell_start..];
        let Some(gt) = after_tag.find('>') else {
            break;
        };
        let after_open = &after_tag[gt + 1..];
        let cell_end = find_closing_tag_ci(after_open, close_tag).unwrap_or(after_open.len());
        let cell_html = &after_open[..cell_end];

        let cleaned = clean_inline_html(cell_html);
        cells.push(TableCell {
            is_header,
            spans: parse_inline(cleaned.trim()),
        });

        let advance = (cell_end + close_tag.len()).min(after_open.len());
        rest = &after_open[advance..];
    }

    cells
}

fn clean_inline_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after_lt = &rest[lt..];
        if after_lt.starts_with("<!--") {
            if let Some(end_idx) = after_lt.find("-->") {
                rest = &after_lt[end_idx + 3..];
                continue;
            }
            break;
        }
        let Some(gt) = after_lt.find('>') else {
            out.push_str(after_lt);
            return decode_entities(&out);
        };
        let tag_trimmed = after_lt[1..gt].trim();
        let tag_lower = tag_trimmed.to_ascii_lowercase();
        let tag_name = tag_lower
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');

        if tag_name == "a" {
            let after_open = &after_lt[gt + 1..];
            if let Some(close_idx) = find_closing_tag_ci(after_open, "</a>") {
                let inner = &after_open[..close_idx];
                let href = extract_attr(tag_trimmed, "href").unwrap_or_default();
                let text = strip_all_tags(inner).trim().to_string();
                if !href.is_empty() && !text.is_empty() {
                    out.push('[');
                    out.push_str(&text);
                    out.push_str("](");
                    out.push_str(&href);
                    out.push(')');
                } else if !text.is_empty() {
                    out.push_str(&text);
                }
                rest = &after_open[close_idx + 4..];
                continue;
            }
        } else if tag_name == "br" || tag_name == "/p" || tag_name == "/div" {
            out.push(' ');
        } else if matches!(tag_name, "b" | "/b" | "strong" | "/strong") {
            out.push_str("**");
        } else if matches!(tag_name, "i" | "/i" | "em" | "/em") {
            out.push('*');
        } else if matches!(tag_name, "code" | "/code") {
            out.push('`');
        }

        rest = &after_lt[gt + 1..];
    }

    out.push_str(rest);
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn find_tag_start_ci(haystack: &str, tag_name_lower: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(pos) = lower[offset..].find('<') {
        let abs = offset + pos;
        let after = &lower[abs + 1..];
        if let Some(rest) = after.strip_prefix(tag_name_lower)
            && rest
                .chars()
                .next()
                .is_some_and(|c| c == '>' || c.is_whitespace() || c == '/')
        {
            return Some(abs);
        }
        offset = abs + 1;
    }
    None
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
        return ("DISCORD".to_string(), palette.focus);
    }
    if u.contains("patreon.com") {
        return ("PATREON".to_string(), palette.cf);
    }
    if u.contains("paypal.me") || u.contains("paypal.com") {
        return ("PAYPAL".to_string(), palette.player);
    }
    if u.contains("ko-fi.com") {
        return ("KO-FI".to_string(), palette.bot);
    }
    if u.contains("github.com") {
        return ("GITHUB".to_string(), palette.ink);
    }
    if u.contains("modrinth.com") {
        return ("MODRINTH".to_string(), palette.mr);
    }
    if u.contains("curseforge.com") {
        return ("CURSEFORGE".to_string(), palette.cf);
    }
    if u.contains("youtube.com") || u.contains("youtu.be") {
        return ("YOUTUBE".to_string(), palette.danger);
    }
    if !alt.trim().is_empty() {
        return (alt.trim().to_ascii_uppercase(), palette.meta);
    }
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("LINK")
        .trim_start_matches("www.");
    (host.to_ascii_uppercase(), palette.player)
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "...")
        .replace("&bull;", "•")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™")
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
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return Some(line[digits + 2..].trim());
    }
    None
}

/// Parses inline Markdown: `[![alt](img)](url)`, `[label](url)`, `![alt](url)`, `**bold**`, `*italic*`, `` `code` ``.
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

        // Linked Markdown image `[![alt](img)](url)` -> render as clickable `[alt](url)` if alt non-empty, else skip
        if rest.starts_with("[![")
            && let Some(img_mid) = rest[3..].find("](")
        {
            let alt = &rest[3..3 + img_mid];
            let after_img_mid = &rest[3 + img_mid + 2..];
            if let Some(img_end) = after_img_mid.find(')') {
                let after_img = &after_img_mid[img_end + 1..];
                if let Some(after_link_open) = after_img.strip_prefix("](")
                    && let Some(link_end) = after_link_open.find(')')
                {
                    let url = &after_link_open[..link_end];
                    if !alt.trim().is_empty()
                        && (url.starts_with("http://") || url.starts_with("https://"))
                    {
                        flush_text(&mut text_buf, &mut spans);
                        spans.push(InlineSpan::Link {
                            label: alt.trim().to_string(),
                            url: url.to_string(),
                        });
                    }
                    let consumed = rest.len() - after_link_open[link_end + 1..].len();
                    i += consumed;
                    continue;
                }
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemePreset;

    #[test]
    fn skips_standalone_markdown_images_and_strips_comments_style_script() {
        let palette = Palette::for_preset(ThemePreset::SynthTerminal);
        let input = r#"
<!-- hidden html comment -->
<style>body { color: red; }</style>
<script>alert("x")</script>
![Banner](https://example.com/banner.png)

Hello **Sodium**!
"#;
        let blocks = parse_blocks(input, &palette);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Paragraph(spans) => {
                assert!(matches!(&spans[0], InlineSpan::Text(t) if t == "Hello "));
                assert!(matches!(&spans[1], InlineSpan::Bold(t) if t == "Sodium"));
                assert!(matches!(&spans[2], InlineSpan::Text(t) if t == "!"));
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn parses_curseforge_html_table_and_badges() {
        let palette = Palette::for_preset(ThemePreset::SynthTerminal);
        let input = r#"
<a href="https://discord.gg/test"><img alt="Discord" src="badge.png"/></a>
<table>
  <thead>
    <tr><th>Feature</th><th>Modrinth</th><th>CurseForge</th></tr>
  </thead>
  <tbody>
    <tr><td>Hash</td><td><code>SHA-1</code></td><td><a href="https://example.com">Murmur2</a></td></tr>
  </tbody>
</table>
"#;
        let blocks = parse_blocks(input, &palette);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::Badges(b) if b.len() == 1 && b[0].label == "DISCORD"));
        match &blocks[1] {
            Block::Table(tbl) => {
                assert_eq!(tbl.rows.len(), 2);
                assert_eq!(tbl.rows[0].len(), 3);
                assert!(tbl.rows[0][0].is_header);
                assert!(!tbl.rows[1][0].is_header);
                assert!(matches!(&tbl.rows[1][1].spans[0], InlineSpan::Code(c) if c == "SHA-1"));
                assert!(matches!(
                    &tbl.rows[1][2].spans[0],
                    InlineSpan::Link { label, url } if label == "Murmur2" && url == "https://example.com"
                ));
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn converts_html_headings_inline_tags_and_pipe_tables() {
        let palette = Palette::for_preset(ThemePreset::TokyoNight);
        let input = r#"
<h2>Overview</h2>
<p>Supports <strong>Fabric</strong> and <code>NeoForge</code>.</p>

| Loader | Status |
| --- | --- |
| Fabric | OK |
"#;
        let blocks = parse_blocks(input, &palette);
        assert!(matches!(&blocks[0], Block::Heading { level: 2, .. }));
        assert!(matches!(&blocks[1], Block::Paragraph(spans) if spans.iter().any(|s| matches!(s, InlineSpan::Bold(b) if b == "Fabric")) && spans.iter().any(|s| matches!(s, InlineSpan::Code(c) if c == "NeoForge"))));
        assert!(matches!(&blocks[2], Block::Table(tbl) if tbl.rows.len() == 2));
    }
}
