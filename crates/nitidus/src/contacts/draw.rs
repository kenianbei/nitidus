//! Frame drawing for the contact book window state: the table pane,
//! the photo, and the property rows.

use bevy::prelude::*;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui_image::StatefulImage;
use ratatui_image::protocol::StatefulProtocol;

use super::photo::PHOTO_ROWS;
use super::render::{COLUMN_SHARES, ContactsWindow, DETAIL_LABEL_WIDTH, TABLE_PANE_PERCENT};
use super::view::PaneFocus;

pub(super) fn render_contacts(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut ContactsWindow,
) -> Result {
    let [table_area, detail_area] = Layout::horizontal([
        Constraint::Percentage(TABLE_PANE_PERCENT),
        Constraint::Fill(1),
    ])
    .areas(area);
    state.table_height = table_area.height;
    state.detail_height = detail_area.height;
    if !state.active {
        return Ok(());
    }
    if let Some(message) = &state.empty_message {
        let paragraph = Paragraph::new(message.as_str())
            .style(state.styles.normal)
            .centered();
        frame.render_widget(paragraph, area);
        return Ok(());
    }
    frame.render_widget(table_paragraph(state, table_area.width), table_area);
    let detail_area = render_photo(frame, detail_area, state);
    frame.render_widget(detail_paragraph(state, detail_area.width), detail_area);
    Ok(())
}

/// Draws the photo above the property rows and returns what remains of
/// the detail pane; a poisoned protocol lock just skips the photo.
fn render_photo(frame: &mut ratatui::Frame, area: Rect, state: &ContactsWindow) -> Rect {
    let Some(cell) = &state.photo else {
        return area;
    };
    let [photo_area, rest] =
        Layout::vertical([Constraint::Length(PHOTO_ROWS), Constraint::Fill(1)]).areas(area);
    if let Ok(mut protocol) = cell.protocol.lock() {
        let image = StatefulImage::<StatefulProtocol>::new();
        frame.render_stateful_widget(image, photo_area, &mut protocol);
    }
    rest
}

fn table_paragraph(state: &ContactsWindow, width: u16) -> Paragraph<'static> {
    let widths = COLUMN_SHARES.map(|share| usize::from(width) * usize::from(share) / 100);
    let lines: Vec<Line<'static>> = state
        .table_rows
        .iter()
        .skip(state.table_top)
        .take(usize::from(state.table_height))
        .map(|row| {
            let style = row_style(row.is_selected, state.focus == PaneFocus::Table, state);
            let cells: Vec<String> = row
                .cells
                .iter()
                .zip(widths)
                .map(|(cell, cell_width)| pad_cell(cell, cell_width))
                .collect();
            Line::from(Span::styled(cells.concat(), style))
        })
        .collect();
    Paragraph::new(lines).style(state.styles.normal)
}

fn detail_paragraph(state: &ContactsWindow, width: u16) -> Paragraph<'static> {
    let value_width = usize::from(width).saturating_sub(DETAIL_LABEL_WIDTH + 1);
    let lines: Vec<Line<'static>> = state
        .detail_rows
        .iter()
        .skip(state.detail_top)
        .take(usize::from(state.detail_height))
        .map(|row| {
            let selected_style =
                row_style(row.is_selected, state.focus == PaneFocus::Detail, state);
            let label_style = if row.is_selected {
                selected_style
            } else if row.is_modeled {
                state.styles.label
            } else {
                state.styles.dim
            };
            Line::from(vec![
                Span::styled(pad_cell(&row.label, DETAIL_LABEL_WIDTH), label_style),
                Span::raw(" "),
                Span::styled(pad_cell(&row.value, value_width), selected_style),
            ])
        })
        .collect();
    Paragraph::new(lines).style(state.styles.normal)
}

fn row_style(is_selected: bool, pane_focused: bool, state: &ContactsWindow) -> Style {
    match (is_selected, pane_focused) {
        (true, true) => state.styles.selected,
        (true, false) => state.styles.unfocused_selected,
        (false, _) => state.styles.normal,
    }
}

/// Truncates with an ellipsis and pads to exactly `width` characters.
fn pad_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count > width {
        let truncated: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        format!("{text}{}", " ".repeat(width - count))
    }
}
