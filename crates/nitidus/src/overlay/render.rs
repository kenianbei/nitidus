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
    dim: Style,
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
            dim: states.disabled.style(),
        }
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

    let visible = usize::from(inner.height.saturating_sub(1));
    let top = scrolled_top(state.selected, visible, state.rows.len());
    let mut lines = vec![Line::from(vec![
        Span::styled("/ ", state.dim),
        Span::styled(state.filter.clone(), state.normal),
    ])];
    for row in state.rows.iter().skip(top).take(visible) {
        lines.push(row_line(row, state, inner.width));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    Ok(())
}

fn row_line(row: &PickerRow, state: &PickerWindow, width: u16) -> Line<'static> {
    let style = if row.selected {
        state.selected_style
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
    selected
        .saturating_sub(visible / 2)
        .min(total - visible)
}
