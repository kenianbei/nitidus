//! Helix-style completion panel: while the command line is open, the
//! matching commands appear in a bottom-anchored panel above the
//! statusline — `name  summary` per row, the Tab-cycle selection
//! highlighted. Height follows the row count (capped), applied by
//! swapping the widget's layout, so nothing draws over the index
//! beyond the rows that exist.

use bevy::prelude::*;
use nitidus_ui_kit::theme::Theme;
use nitidus_ui_kit::{layer, layout};
use plurimus::{Widget, WidgetLayout, WidgetOrder};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::CommandLineState;
use crate::command::describe;
use crate::keymap::{InputMode, Mode};

const MAX_PANEL_ROWS: u16 = 8;
const NAME_COLUMN_WIDTH: usize = 22;

#[derive(Component)]
pub(super) struct CompletionPanel;

#[derive(Clone, Default)]
pub(super) struct PanelRender {
    lines: Vec<Line<'static>>,
    background: Style,
}

/// Spawns while candidates exist, despawns otherwise — despawning is
/// what lets the widgets underneath repaint their cells (the same
/// contract the picker overlay relies on).
pub(super) fn refresh_panel(
    mode: Res<Mode>,
    state: Res<CommandLineState>,
    theme: Res<Theme>,
    mut commands: Commands,
    mut widgets: Query<(Entity, &mut Widget), With<CompletionPanel>>,
) -> Result {
    if !(mode.is_changed() || state.is_changed() || theme.is_changed()) {
        return Ok(());
    }
    let (candidates, selected) = if mode.0 == InputMode::CommandLine {
        state.completion_view()
    } else {
        (Vec::new(), None)
    };
    if candidates.is_empty() {
        if let Ok((entity, _)) = widgets.single_mut() {
            commands.entity(entity).despawn();
        }
        return Ok(());
    }
    let (lines, height) = visible_lines(&candidates, selected, &theme);
    let render = PanelRender {
        lines,
        background: theme.base.default.normal.style(),
    };
    let layout = WidgetLayout::from(layout::bottom_panel_layout(height));
    match widgets.single_mut() {
        Ok((entity, mut widget)) => {
            widget.set_state(render)?;
            commands.entity(entity).insert(layout);
        }
        Err(_) => {
            commands.spawn((
                CompletionPanel,
                Widget::from_render_fn_with_state(render_panel, render),
                WidgetOrder(layer::PANEL),
                layout,
            ));
        }
    }
    Ok(())
}

/// The window of rows around the selection that fits the panel cap.
fn visible_lines(
    candidates: &[String],
    selected: Option<usize>,
    theme: &Theme,
) -> (Vec<Line<'static>>, u16) {
    let height = candidates.len().min(usize::from(MAX_PANEL_ROWS));
    let top = selected
        .map(|index| index.saturating_sub(height - 1))
        .unwrap_or(0)
        .min(candidates.len() - height);
    let states = &theme.base.default;
    let lines = candidates
        .iter()
        .enumerate()
        .skip(top)
        .take(height)
        .map(|(index, name)| {
            let style = if Some(index) == selected {
                states.selected.style()
            } else {
                states.normal.style()
            };
            let summary = describe(name).unwrap_or_default();
            Line::from(vec![Span::styled(
                format!(" {name:<NAME_COLUMN_WIDTH$} {summary}"),
                style,
            )])
        })
        .collect();
    (lines, u16::try_from(height).unwrap_or(MAX_PANEL_ROWS))
}

fn render_panel(frame: &mut ratatui::Frame, area: Rect, state: &mut PanelRender) -> Result {
    // The terminal buffer persists across per-widget repaints, so the
    // panel must clear its rect or the index shows through.
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new(state.lines.clone()).style(state.background),
        area,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use nitidus_ui_kit::theme::tailwind_dark;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn visible_lines_cap_height_and_follow_the_selection() {
        let theme = tailwind_dark();
        let candidates = names(&["a", "b", "c"]);
        let (lines, height) = visible_lines(&candidates, None, &theme);
        assert_eq!(lines.len(), 3);
        assert_eq!(height, 3);

        let many: Vec<String> = (0..20).map(|index| format!("cmd{index}")).collect();
        let (lines, height) = visible_lines(&many, Some(15), &theme);
        assert_eq!(height, MAX_PANEL_ROWS);
        assert_eq!(lines.len(), usize::from(MAX_PANEL_ROWS));
        assert!(
            lines
                .iter()
                .any(|line| line.spans[0].content.contains("cmd15")),
            "the selected row must be inside the visible window"
        );
    }
}
