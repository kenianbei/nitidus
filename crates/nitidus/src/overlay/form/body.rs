//! The multi-line field's buffer and the pass that recolours it.
//!
//! The widget has no API for styling a range of text, so a field that
//! wants quoted lines dimmed says so with a `BodyStyleFn` and the styles
//! are applied after the widget draws: each rendered row is mapped back
//! to its data line and recoloured. Only cells the widget left at the
//! base style are touched, so selection, search, and the cursor keep
//! their own styling.

use std::sync::{Arc, Mutex, MutexGuard};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui_textarea::TextArea;

/// `TextArea` caches its screen map in `RefCell`s, so it is `Send` but
/// not `Sync` — it can be neither widget state nor a bare resource.
/// Sharing one behind a mutex satisfies both, and lets the renderer
/// borrow the live buffer instead of copying it every frame.
pub type SharedArea = Arc<Mutex<TextArea<'static>>>;

/// A panicking edit would poison the lock; the buffer is still the
/// user's text, so recover it rather than lose what they wrote.
pub fn lock(area: &SharedArea) -> MutexGuard<'_, TextArea<'static>> {
    area.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// An empty value is one empty line, not no lines: a buffer with no
/// lines has nowhere to put the caret.
pub(super) fn area_from(value: &str) -> SharedArea {
    let lines: Vec<String> = if value.is_empty() {
        vec![String::new()]
    } else {
        value.lines().map(str::to_owned).collect()
    };
    Arc::new(Mutex::new(TextArea::new(lines)))
}

/// Recolours the rows of `area` whose data line `styled` gives a style
/// for. `base` is the style the widget draws unstyled text in; a cell
/// that differs from it belongs to the cursor, a selection, or a search
/// hit and is left alone.
pub(crate) fn paint_lines(
    buffer: &mut Buffer,
    area: Rect,
    text: &TextArea<'_>,
    base: Style,
    styled: impl Fn(usize) -> Option<Style>,
) {
    let (top_row, top_col) = text.scroll_offset();
    for y in area.top()..area.bottom() {
        let row = usize::from(top_row.saturating_add(y - area.top()));
        let Some(style) = styled(text.screen_to_data(row, usize::from(top_col)).0) else {
            continue;
        };
        paint_row(buffer, area, y, base, style);
    }
}

fn paint_row(buffer: &mut Buffer, area: Rect, y: u16, base: Style, style: Style) {
    for x in area.left()..area.right() {
        let cell = &mut buffer[(x, y)];
        if cell.fg == base.fg.unwrap_or(Color::Reset) && cell.bg == base.bg.unwrap_or(Color::Reset)
        {
            cell.set_fg(style.fg.unwrap_or(Color::Reset));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::pager::body::LineKind;
    use ratatui::widgets::Widget as _;
    use ratatui_textarea::{CursorMove, WrapMode};

    const DIM: Color = Color::DarkGray;
    const CURSOR_BG: Color = Color::White;

    /// The cursor outranks line styling, so give it an explicit style —
    /// exactly as `apply` does — and it stays out of the pass.
    fn draw(lines: &[&str], width: u16, height: u16) -> (TextArea<'static>, Buffer, Rect) {
        let mut text = TextArea::new(lines.iter().map(|line| (*line).to_owned()).collect());
        text.set_cursor_style(Style::default().fg(Color::Black).bg(CURSOR_BG));
        text.set_cursor_line_style(Style::default());
        text.set_wrap_mode(WrapMode::Word);
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        (&text).render(area, &mut buffer);
        (text, buffer, area)
    }

    fn dim_quotes(lines: &[&str]) -> Vec<Option<Style>> {
        let owned: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        crate::pager::body::classify_lines(&owned)
            .into_iter()
            .map(|kind| match kind {
                LineKind::Normal => None,
                LineKind::Quote(_) | LineKind::Signature => Some(Style::default().fg(DIM)),
            })
            .collect()
    }

    fn foreground(buffer: &Buffer, x: u16, y: u16) -> Color {
        buffer[(x, y)].fg
    }

    #[test]
    fn quoted_lines_dim_and_normal_lines_are_left_alone() {
        let lines = ["> quoted", "my reply"];
        let (text, mut buffer, area) = draw(&lines, 20, 4);
        let styles = dim_quotes(&lines);

        paint_lines(&mut buffer, area, &text, Style::default(), |row| {
            styles.get(row).copied().flatten()
        });

        assert_eq!(foreground(&buffer, 2, 0), DIM, "the quote dims");
        assert_eq!(
            foreground(&buffer, 0, 1),
            Color::Reset,
            "the reply is untouched"
        );
    }

    #[test]
    fn a_signature_dims_from_the_separator_down() {
        let lines = ["body", "-- ", "Norman"];
        let (text, mut buffer, area) = draw(&lines, 20, 5);
        let styles = dim_quotes(&lines);

        paint_lines(&mut buffer, area, &text, Style::default(), |row| {
            styles.get(row).copied().flatten()
        });

        // Column 0 of row 0 holds the cursor, which has its own styling.
        assert_eq!(foreground(&buffer, 1, 0), Color::Reset, "body is untouched");
        assert_eq!(foreground(&buffer, 0, 2), DIM, "the signature dims");
    }

    #[test]
    fn a_wrapped_quote_dims_on_every_display_row() {
        let lines = ["> a quoted line long enough to wrap twice over"];
        let (text, mut buffer, area) = draw(&lines, 12, 6);
        let styles = dim_quotes(&lines);

        paint_lines(&mut buffer, area, &text, Style::default(), |row| {
            styles.get(row).copied().flatten()
        });

        assert_eq!(foreground(&buffer, 2, 0), DIM);
        assert_eq!(
            foreground(&buffer, 0, 1),
            DIM,
            "the wrapped remainder belongs to the same body line"
        );
    }

    #[test]
    fn the_cursor_keeps_its_own_styling() {
        let lines = ["> quoted"];
        let (text, mut buffer, area) = draw(&lines, 20, 3);
        let styles = dim_quotes(&lines);

        paint_lines(&mut buffer, area, &text, Style::default(), |row| {
            styles.get(row).copied().flatten()
        });

        assert_eq!(
            buffer[(0, 0)].bg,
            CURSOR_BG,
            "the cursor cell must survive the pass"
        );
        assert_eq!(foreground(&buffer, 2, 0), DIM, "the rest still dims");
    }

    #[test]
    fn a_selection_outranks_line_styling() {
        let lines = ["> quoted"];
        let mut text = TextArea::new(vec![lines[0].to_owned()]);
        text.set_cursor_style(Style::default().fg(Color::Black).bg(CURSOR_BG));
        text.set_cursor_line_style(Style::default());
        text.set_selection_style(Style::default().bg(Color::LightBlue));
        text.move_cursor(CursorMove::Head);
        text.start_selection();
        text.move_cursor(CursorMove::End);

        let area = Rect::new(0, 0, 20, 3);
        let mut buffer = Buffer::empty(area);
        (&text).render(area, &mut buffer);
        let styles = dim_quotes(&lines);

        paint_lines(&mut buffer, area, &text, Style::default(), |row| {
            styles.get(row).copied().flatten()
        });

        assert_eq!(
            buffer[(2, 0)].bg,
            Color::LightBlue,
            "the selection background survives"
        );
        assert_ne!(
            foreground(&buffer, 2, 0),
            DIM,
            "selected text must not be dimmed"
        );
    }

    #[test]
    fn an_unclassified_body_is_left_entirely_alone() {
        let lines = ["plain", "text"];
        let (text, mut buffer, area) = draw(&lines, 20, 4);
        let before = buffer.clone();

        paint_lines(&mut buffer, area, &text, Style::default(), |_| None);

        assert_eq!(buffer, before, "no styles means no writes");
    }
}
