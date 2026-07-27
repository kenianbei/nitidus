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
    hover_style: Style,
    /// Mouse-hovered absolute row; survives refresh, cleared on leave.
    hovered: Option<usize>,
    pub(super) last_height: u16,
}

impl SidebarWindow {
    pub(super) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(super) fn top(&self) -> usize {
        self.top
    }

    pub(super) fn has_hover(&self) -> bool {
        self.hovered.is_some()
    }

    pub(super) fn set_hovered(&mut self, row: Option<usize>) {
        self.hovered = row;
    }
}

pub(super) fn refresh_sidebar(
    theme: Res<Theme>,
    state: Res<SidebarState>,
    tabs: Res<crate::shell::Tabs>,
    focus: Res<crate::focus::PaneFocus>,
    rows: Res<SidebarRows>,
    mut widgets: Query<&mut Widget, With<SidebarWidget>>,
) -> Result {
    if !(theme.is_changed()
        || state.is_changed()
        || rows.is_changed()
        || tabs.is_changed()
        || focus.is_changed())
    {
        return Ok(());
    }
    let is_focused = focus.is(crate::focus::Pane::Folders);
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let (last_height, hovered) = {
        let previous = widget.get_state::<SidebarWindow>()?;
        (previous.last_height, previous.hovered)
    };
    let window = SidebarWindow {
        // The contacts tab owns the whole content region; the mail
        // sidebar must not bleed into it.
        visible: state.visible && !tabs.is_contacts(),
        lines: rows
            .0
            .iter()
            .enumerate()
            .map(|(index, row)| row_line(row, index == state.selected, is_focused, &theme))
            .collect(),
        top: state.top,
        normal: theme.base.default.normal.style(),
        hover_style: theme.base.default.hovered.style(),
        hovered,
        last_height,
    };
    widget.set_state(window)?;
    Ok(())
}

fn row_line(row: &SidebarRow, selected: bool, is_focused: bool, theme: &Theme) -> Line<'static> {
    let states = &theme.base.default;
    let style = match &row.kind {
        RowKind::AccountHeader => theme.base.info.normal.style().add_modifier(Modifier::BOLD),
        _ if selected && is_focused => states.selected.style(),
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
        .enumerate()
        .skip(state.top)
        .take(usize::from(area.height))
        .map(|(index, line)| {
            if state.hovered == Some(index) {
                hovered_line(line, state.hover_style)
            } else {
                line.clone()
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(visible).style(state.normal), area);
    Ok(())
}

/// Patches the hover background under each span's own foreground.
fn hovered_line(line: &Line<'static>, hover: Style) -> Line<'static> {
    let background = Style::new().bg(hover.bg.unwrap_or_default());
    line.spans
        .iter()
        .map(|span| Span::styled(span.content.clone(), span.style.patch(background)))
        .collect::<Vec<_>>()
        .into()
}
