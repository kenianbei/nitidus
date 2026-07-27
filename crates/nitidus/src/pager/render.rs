//! Pager window construction and drawing: weeded/full headers, the
//! quote-colored body, an attachment footer, and scroll windowing with
//! width/height feedback.

use nitidus_mail::EnvelopeId;
use nitidus_mail::message::PartKind;
use nitidus_ui_kit::surface::{FrameChrome, draw_frame};
use nitidus_ui_kit::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::body::{self, BodyLine, LineKind};
use super::html;
use super::{OpenMessage, PagerState};

const WEEDED_HEADERS: &[&str] = &["From", "To", "Cc", "Date", "Subject"];
const DEFAULT_TITLE: &str = "reading";
const HINT: &str = " Z unzooms ⋅ Esc closes ";

/// What the reading pane is showing, for the zoomed frame's title.
#[derive(Clone, Copy, Debug)]
pub(super) struct WindowChrome {
    pub active: bool,
    pub zoomed: bool,
}

fn subject_of(open: &OpenMessage) -> String {
    open.view
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("subject"))
        .map(|(_, value)| value.clone())
        .filter(|subject| !subject.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TITLE.to_owned())
}
const FALLBACK_WIDTH: u16 = 80;
/// Shown while the pane holds nothing: reading is an explicit act, so
/// the pane has to say how to start it.
const IDLE_HINT: &str = "Enter or → opens the selected message";

#[derive(Clone, Default)]
pub(super) struct PagerWindow {
    pub active: bool,
    /// Drawn over its neighbours rather than in its column, so it has to
    /// clear what is underneath and frame itself.
    pub zoomed: bool,
    pub title: String,
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
    chrome: WindowChrome,
    previous: &PagerWindow,
) -> PagerWindow {
    let normal = theme.base.default.normal.style();
    let mut window = PagerWindow {
        active: chrome.active,
        zoomed: chrome.zoomed,
        title: pager
            .open
            .as_ref()
            .map_or_else(|| DEFAULT_TITLE.to_owned(), subject_of),
        normal,
        last_width: previous.last_width,
        last_height: previous.last_height,
        ..PagerWindow::default()
    };
    let Some(open) = &pager.open else {
        window.message = Some(if pager.loading.is_some() {
            "loading…".to_owned()
        } else {
            IDLE_HINT.to_owned()
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
        Some(part) if part.kind == PartKind::Html => {
            append_html_body(part, theme, width, window);
        }
        Some(part) => {
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

fn append_html_body(
    part: &nitidus_mail::message::PartView,
    theme: &Theme,
    width: usize,
    window: &mut PagerWindow,
) {
    let sanitized = html::sanitize(part.text.as_deref().unwrap_or_default());
    if sanitized.blocked_remote > 0 {
        let plural = if sanitized.blocked_remote == 1 {
            ""
        } else {
            "s"
        };
        window.lines.push(Line::from(Span::styled(
            format!(
                "[{} remote image{plural} blocked]",
                sanitized.blocked_remote
            ),
            theme.base.info.normal.style(),
        )));
        window.kinds.push(LineKind::Normal);
    }
    for line in html::render_html(&sanitized.html, width).lines {
        window
            .lines
            .push(styled_html_line(&line, theme, window.normal));
        window.kinds.push(line.kind);
    }
}

fn styled_html_line(line: &html::HtmlLine, theme: &Theme, normal: Style) -> Line<'static> {
    let base = match line.kind {
        LineKind::Quote(depth) => quote_style(theme, depth),
        _ => normal,
    };
    let spans = line
        .spans
        .iter()
        .map(|(text, tag)| Span::styled(text.clone(), html_span_style(*tag, theme, base)))
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn html_span_style(tag: html::SpanStyleTag, theme: &Theme, base: Style) -> Style {
    let mut style = base;
    if tag.is_code {
        style = theme.base.default.disabled.style();
    }
    if tag.is_link {
        style = theme
            .base
            .info
            .normal
            .style()
            .add_modifier(Modifier::UNDERLINED);
    }
    if tag.is_strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if tag.is_emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if tag.is_strikeout {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
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
    let palettes = [&theme.base.success, &theme.base.info, &theme.base.warning];
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
    if !state.active {
        state.last_width = area.width;
        state.last_height = area.height;
        return Ok(());
    }
    let area = if state.zoomed {
        draw_frame(
            frame.buffer_mut(),
            area,
            FrameChrome {
                title: &state.title,
                hint: Some(HINT),
                style: state.normal,
            },
        )
    } else {
        area
    };
    state.last_width = area.width;
    state.last_height = area.height;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use ratatui::backend::TestBackend;

    use super::*;

    fn window(zoomed: bool) -> PagerWindow {
        PagerWindow {
            active: true,
            zoomed,
            title: "a subject".to_owned(),
            lines: vec![Line::from("short")],
            ..PagerWindow::default()
        }
    }

    /// Draws the pane over a region the neighbouring panes already
    /// filled, and returns one row of the result.
    fn row_after_draw(mut state: PagerWindow, area: Rect, row: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                let buffer = frame.buffer_mut();
                for x in area.x..area.right() {
                    for y in area.y..area.bottom() {
                        buffer[(x, y)].set_symbol("#");
                    }
                }
                render_pager(frame, area, &mut state).unwrap();
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..area.width)
            .map(|x| buffer[(x, row)].symbol().to_owned())
            .collect()
    }

    /// The zoomed pane draws over its neighbours, so it has to clear
    /// what they left rather than letting it show through.
    #[test]
    fn zooming_clears_what_was_underneath() {
        let painted = row_after_draw(window(true), Rect::new(0, 0, 20, 6), 2);

        assert!(
            !painted.contains('#'),
            "the panes beneath still show through: {painted:?}"
        );
    }

    #[test]
    fn zooming_frames_the_pane_with_its_subject() {
        let top = row_after_draw(window(true), Rect::new(0, 0, 24, 6), 0);

        assert!(top.contains("a subject"), "top border was {top:?}");
    }

    /// In its own column nothing is underneath, so the pane keeps every
    /// row for content rather than spending two on a border.
    #[test]
    fn the_unzoomed_pane_draws_no_frame() {
        let top = row_after_draw(window(false), Rect::new(0, 0, 20, 6), 0);

        assert!(
            top.starts_with("short"),
            "content starts at the top: {top:?}"
        );
    }
}
