//! Render checks for the index window: the layouts draw exactly the
//! lines their row height claims.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::*;
use crate::config::{IndexLayout, IndexUiConfig};

fn window(layout: IndexLayout, count: usize) -> IndexWindowState {
    let index_config = IndexUiConfig {
        layout,
        ..IndexUiConfig::default()
    };
    let rows = (0..count)
        .map(|row| IndexRow {
            date: format!("Mon, {:02} Jul 2026 15:04", row + 1),
            from: format!("Sender {row}"),
            subject: format!("Subject {row}"),
            ..IndexRow::default()
        })
        .collect();
    IndexWindowState {
        active: true,
        rows,
        empty_message: None,
        context: render::RowContext {
            styles: RowStyles::from_theme(&nitidus_ui_kit::theme::tailwind_dark()),
            layout,
            columns: index_config.columns,
        },
        search: None,
        last_height: 0,
        row_height: layout.row_height(),
        window_top: 0,
        hovered_row: None,
    }
}

fn drawn(mut state: IndexWindowState, area: Rect) -> Vec<String> {
    let mut terminal = ratatui::Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| render_index(frame, area, &mut state).unwrap())
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[test]
fn a_card_layout_stacks_three_lines_per_message() {
    let lines = drawn(window(IndexLayout::Cards, 4), Rect::new(0, 0, 24, 9));

    assert_eq!(lines.len(), 9);
    assert_eq!(lines[0], "  Sender 0");
    assert_eq!(lines[1], "  Subject 0");
    assert_eq!(lines[2], "  Mon, 01 Jul 2026 15:04");
    assert_eq!(lines[3], "  Sender 1", "the next card starts flush");
    assert_eq!(
        lines[6], "  Sender 2",
        "nine lines hold exactly three cards"
    );
}

#[test]
fn a_pane_too_short_for_a_whole_card_draws_none_of_it() {
    let lines = drawn(window(IndexLayout::Cards, 4), Rect::new(0, 0, 24, 2));

    assert!(
        lines.iter().all(String::is_empty),
        "a partial card is not drawn: {lines:?}"
    );
}

#[test]
fn a_table_layout_still_draws_one_line_per_message() {
    let lines = drawn(window(IndexLayout::Table, 4), Rect::new(0, 0, 40, 3));

    assert_eq!(lines.len(), 3);
    for (row, line) in lines.iter().enumerate() {
        assert!(line.contains(&format!("Subject {row}")), "{line:?}");
    }
}
