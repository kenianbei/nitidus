//! Picker panel rendering: a cleared, bordered floating block with a
//! filter line and the ranked list, sized to fit within its layout rect.

use nitidus_ui_kit::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::PickerState;

const CHROME_ROWS: u16 = 3;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct PickerRow {
    pub label: String,
    pub detail: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Default)]
pub(super) struct PickerWindow {
    title: String,
    filter: String,
    rows: Vec<PickerRow>,
    selected: usize,
    normal: Style,
    selected_style: Style,
    hover_style: Style,
    dim: Style,
    /// Mouse-hovered absolute row; survives refresh, cleared on leave.
    hovered: Option<usize>,
}

impl PickerWindow {
    pub(super) fn new(picker: &PickerState, rows: Vec<PickerRow>, theme: &Theme) -> Self {
        let states = &theme.paper.default;
        Self {
            title: picker.title.clone(),
            filter: picker.filter.clone(),
            selected: picker.selected,
            rows,
            normal: states.normal.style(),
            selected_style: states.selected.style(),
            hover_style: states.hovered.style(),
            dim: states.disabled.style(),
            hovered: None,
        }
    }
}

/// The rows' scroll window and screen position, shared between the
/// renderer and the mouse handler so click math matches the drawing.
pub(super) struct RowsGeometry {
    pub(super) first_row_y: u16,
    pub(super) top: usize,
    pub(super) visible: usize,
}

pub(super) fn rows_geometry(area: Rect, row_count: usize, selected: usize) -> RowsGeometry {
    let height = (row_count as u16 + CHROME_ROWS).min(area.height);
    let panel_y = area.y + (area.height - height) / 2;
    let visible = usize::from(height.saturating_sub(CHROME_ROWS));
    RowsGeometry {
        // Past the border and the filter line.
        first_row_y: panel_y + 2,
        top: scrolled_top(selected, visible, row_count),
        visible,
    }
}

impl PickerWindow {
    pub(super) fn row_window(&self, area: Rect) -> RowsGeometry {
        rows_geometry(area, self.rows.len(), self.selected)
    }

    pub(super) fn has_hover(&self) -> bool {
        self.hovered.is_some()
    }

    pub(super) fn hovered_row(&self) -> Option<usize> {
        self.hovered
    }

    pub(super) fn set_hovered(&mut self, row: Option<usize>) {
        self.hovered = row;
    }
}

pub(super) fn render_picker(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut PickerWindow,
) -> bevy::prelude::Result {
    let height = (state.rows.len() as u16 + CHROME_ROWS).min(area.height);
    let panel = Rect {
        y: area.y + (area.height - height) / 2,
        height,
        ..area
    };
    frame.render_widget(Clear, panel);
    let block = Block::bordered()
        .title(format!(" {} ", state.title))
        .style(state.normal);
    let inner = block.inner(panel);
    frame.render_widget(block, panel);

    let window = state.row_window(area);
    let mut lines = vec![Line::from(vec![
        Span::styled("/ ", state.dim),
        Span::styled(state.filter.clone(), state.normal),
    ])];
    for (index, row) in state
        .rows
        .iter()
        .enumerate()
        .skip(window.top)
        .take(window.visible)
    {
        lines.push(row_line(
            row,
            state,
            inner.width,
            state.hovered == Some(index),
        ));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    Ok(())
}

fn row_line(row: &PickerRow, state: &PickerWindow, width: u16, hovered: bool) -> Line<'static> {
    let style = if row.selected {
        state.selected_style
    } else if hovered {
        state.hover_style
    } else {
        state.normal
    };
    let mut spans = vec![Span::styled(format!(" {}", row.label), style)];
    if let Some(detail) = &row.detail {
        spans.push(Span::styled(
            format!("  {detail}"),
            if row.selected { style } else { state.dim },
        ));
    }
    let text_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let padding = usize::from(width).saturating_sub(text_len);
    spans.push(Span::styled(" ".repeat(padding), style));
    Line::from(spans)
}

fn scrolled_top(selected: usize, visible: usize, total: usize) -> usize {
    if visible == 0 || total <= visible {
        return 0;
    }
    selected.saturating_sub(visible / 2).min(total - visible)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn rows_geometry_matches_the_rendered_panel() {
        // 5 rows fit: panel = 5 + 3 chrome = 8 tall, centered in 20.
        let area = Rect::new(10, 4, 40, 20);
        let small = rows_geometry(area, 5, 0);
        assert_eq!(small.first_row_y, 4 + 6 + 2, "border + filter offset");
        assert_eq!(small.top, 0);
        assert_eq!(small.visible, 5);

        // 40 rows overflow: panel fills the area, list scrolls around
        // the selection.
        let large = rows_geometry(area, 40, 20);
        assert_eq!(large.first_row_y, 4 + 2);
        assert_eq!(large.visible, 17);
        assert_eq!(large.top, 20 - 17 / 2);
    }
}
