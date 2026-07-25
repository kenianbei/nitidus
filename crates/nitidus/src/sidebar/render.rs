//! Sidebar drawing: indented tree rows with collapse markers, unread
//! badges, and a selection highlight that reflects focus.

use bevy::prelude::*;
use nitidus_ui_kit::theme::Theme;
use plurimus::Widget;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::tree::{RowKind, SidebarRow};
use super::{SidebarRows, SidebarState, SidebarWidget};

const COLLAPSED_MARKER: &str = "▸ ";
const EXPANDED_MARKER: &str = "▾ ";
const LEAF_MARKER: &str = "  ";

#[derive(Clone, Default)]
pub(super) struct SidebarWindow {
    visible: bool,
    lines: Vec<Line<'static>>,
    top: usize,
    normal: Style,
    pub(super) last_height: u16,
}

pub(super) fn refresh_sidebar(
    theme: Res<Theme>,
    state: Res<SidebarState>,
    screen: Res<crate::screen::Screen>,
    rows: Res<SidebarRows>,
    mut widgets: Query<&mut Widget, With<SidebarWidget>>,
) -> Result {
    if !(theme.is_changed() || state.is_changed() || rows.is_changed() || screen.is_changed()) {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let last_height = widget.get_state::<SidebarWindow>()?.last_height;
    let window = SidebarWindow {
        // The contacts tab owns the whole content region; the mail
        // sidebar must not bleed into it.
        visible: state.visible && *screen != crate::screen::Screen::Contacts,
        lines: rows
            .0
            .iter()
            .enumerate()
            .map(|(index, row)| row_line(row, index == state.selected, &state, &theme))
            .collect(),
        top: state.top,
        normal: theme.base.default.normal.style(),
        last_height,
    };
    widget.set_state(window)?;
    Ok(())
}

fn row_line(
    row: &SidebarRow,
    selected: bool,
    state: &SidebarState,
    theme: &Theme,
) -> Line<'static> {
    let states = &theme.base.default;
    let style = match &row.kind {
        RowKind::AccountHeader => theme.base.info.normal.style().add_modifier(Modifier::BOLD),
        _ if selected && state.focused => states.selected.style(),
        _ if selected => states.selected.style().add_modifier(Modifier::DIM),
        RowKind::Synthetic => states.disabled.style(),
        RowKind::Folder(_) => states.normal.style(),
    };
    let marker = match (row.has_children, row.is_collapsed) {
        (false, _) => LEAF_MARKER,
        (true, true) => COLLAPSED_MARKER,
        (true, false) => EXPANDED_MARKER,
    };
    let indent = "  ".repeat(usize::from(row.depth));
    let mut spans = vec![Span::styled(
        format!("{indent}{marker}{}", row.label),
        style,
    )];
    if row.unread > 0 && !matches!(row.kind, RowKind::AccountHeader) {
        spans.push(Span::styled(
            format!(" ({})", row.unread),
            theme.base.info.normal.style(),
        ));
    }
    Line::from(spans)
}

pub(super) fn render_sidebar(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut SidebarWindow,
) -> Result {
    state.last_height = area.height;
    if !state.visible {
        return Ok(());
    }
    let visible: Vec<Line<'static>> = state
        .lines
        .iter()
        .skip(state.top)
        .take(usize::from(area.height))
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(visible).style(state.normal), area);
    Ok(())
}
