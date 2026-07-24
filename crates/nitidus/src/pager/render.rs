//! Pager window construction and drawing: weeded/full headers, the
//! quote-colored body, an attachment footer, and scroll windowing with
//! width/height feedback.

use nitidus_mail::EnvelopeId;
use nitidus_mail::message::PartKind;
use nitidus_ui_kit::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::body::{self, BodyLine, LineKind};
use super::{OpenMessage, PagerState};

const WEEDED_HEADERS: &[&str] = &["From", "To", "Cc", "Date", "Subject"];
const FALLBACK_WIDTH: u16 = 80;

#[derive(Clone, Default)]
pub(super) struct PagerWindow {
    pub active: bool,
    pub lines: Vec<Line<'static>>,
    pub kinds: Vec<LineKind>,
    pub scroll: usize,
    pub part_label: Option<String>,
    pub message: Option<String>,
    pub normal: Style,
    for_key: Option<(EnvelopeId, usize, bool)>,
    pub last_width: u16,
    pub last_height: u16,
}

pub(super) fn build_window(
    pager: &PagerState,
    theme: &Theme,
    active: bool,
    previous: &PagerWindow,
) -> PagerWindow {
    let normal = theme.base.default.normal.style();
    let mut window = PagerWindow {
        active,
        normal,
        last_width: previous.last_width,
        last_height: previous.last_height,
        ..PagerWindow::default()
    };
    let Some(open) = &pager.open else {
        window.message = Some(if pager.loading.is_some() {
            "loading…".to_owned()
        } else {
            "no message open".to_owned()
        });
        return window;
    };
    let width = usize::from(if previous.last_width == 0 {
        FALLBACK_WIDTH
    } else {
        previous.last_width
    });
    build_message_lines(open, theme, width, &mut window);
    let key = (open.id.clone(), open.part, open.show_all_headers);
    window.scroll = if previous.for_key == Some(key.clone()) {
        previous.scroll.min(window.lines.len().saturating_sub(1))
    } else {
        0
    };
    window.for_key = Some(key);
    window.part_label = part_label(open);
    window
}

fn build_message_lines(open: &OpenMessage, theme: &Theme, width: usize, window: &mut PagerWindow) {
    let name_style = theme.base.info.normal.style().add_modifier(Modifier::BOLD);
    let normal = window.normal;
    for (name, value) in weeded_headers(open) {
        window.lines.push(Line::from(vec![
            Span::styled(format!("{name}: "), name_style),
            Span::styled(value, normal),
        ]));
        window.kinds.push(LineKind::Normal);
    }
    window.lines.push(Line::default());
    window.kinds.push(LineKind::Normal);

    match open.view.parts.get(open.part) {
        Some(part) => {
            if part.kind == PartKind::Html {
                window.lines.push(Line::from(Span::styled(
                    "[text/html shown raw — styled rendering lands with tier 1]",
                    theme.base.warning.normal.style(),
                )));
                window.kinds.push(LineKind::Normal);
            }
            for line in body::build_body_lines(part, width) {
                window.lines.push(styled_body_line(&line, theme, normal));
                window.kinds.push(line.kind);
            }
        }
        None => {
            window.lines.push(Line::from(Span::styled(
                "[no displayable part]",
                theme.base.warning.normal.style(),
            )));
            window.kinds.push(LineKind::Normal);
        }
    }
    append_attachment_footer(open, theme, window);
}

fn append_attachment_footer(open: &OpenMessage, theme: &Theme, window: &mut PagerWindow) {
    let attachments = open.view.attachment_indices();
    if attachments.is_empty() {
        return;
    }
    window.lines.push(Line::default());
    window.kinds.push(LineKind::Normal);
    let label_style = theme.base.info.normal.style();
    for index in attachments {
        let part = &open.view.parts[index];
        let name = part.filename.as_deref().unwrap_or("(unnamed)");
        window.lines.push(Line::from(Span::styled(
            format!("📎 {name}  {}  {} bytes", part.mime, part.size),
            label_style,
        )));
        window.kinds.push(LineKind::Normal);
    }
}

fn weeded_headers(open: &OpenMessage) -> Vec<(String, String)> {
    if open.show_all_headers {
        return open.view.headers.clone();
    }
    WEEDED_HEADERS
        .iter()
        .filter_map(|wanted| {
            open.view
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
                .map(|(name, value)| (name.clone(), value.clone()))
        })
        .collect()
}

fn styled_body_line(line: &BodyLine, theme: &Theme, normal: Style) -> Line<'static> {
    let style = match line.kind {
        LineKind::Normal => normal,
        LineKind::Signature => theme.base.default.disabled.style(),
        LineKind::Quote(depth) => quote_style(theme, depth),
    };
    Line::from(Span::styled(line.text.clone(), style))
}

fn quote_style(theme: &Theme, depth: u8) -> Style {
    let palettes = [
        &theme.base.success,
        &theme.base.info,
        &theme.base.warning,
    ];
    palettes[usize::from(depth - 1) % palettes.len()]
        .normal
        .style()
}

fn part_label(open: &OpenMessage) -> Option<String> {
    let bodies = open.view.body_part_indices();
    if bodies.len() < 2 {
        return None;
    }
    let position = bodies.iter().position(|&index| index == open.part)? + 1;
    let mime = &open.view.parts.get(open.part)?.mime;
    Some(format!("{mime} {position}/{}", bodies.len()))
}

pub(super) fn render_pager(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut PagerWindow,
) -> bevy::prelude::Result {
    state.last_width = area.width;
    state.last_height = area.height;
    if !state.active {
        return Ok(());
    }
    if let Some(message) = &state.message {
        let paragraph = Paragraph::new(message.as_str())
            .style(state.normal)
            .centered();
        frame.render_widget(paragraph, area);
        return Ok(());
    }
    let visible: Vec<Line<'static>> = state
        .lines
        .iter()
        .skip(state.scroll)
        .take(usize::from(area.height))
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(visible).style(state.normal), area);
    Ok(())
}
