//! Completion candidates for the focused field, in a bottom-anchored
//! panel above the statusline — the same place the command line puts
//! its own, so completion always appears in one spot.
//!
//! Spawns while candidates exist and despawns otherwise, so the widgets
//! underneath repaint their cells.

use bevy::prelude::*;
use nitidus_ui_kit::theme::Theme;
use nitidus_ui_kit::{layer, layout};
use plurimus::{Widget, WidgetLayout, WidgetOrder};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::ActiveForm;

const MAX_PANEL_ROWS: u16 = 8;

#[derive(Component)]
pub(super) struct FormPanel;

#[derive(Clone, Default)]
pub(super) struct PanelRender {
    lines: Vec<Line<'static>>,
    background: Style,
}

pub(super) fn refresh_panel(
    form: Res<ActiveForm>,
    theme: Res<Theme>,
    mut commands: Commands,
    mut widgets: Query<(Entity, &mut Widget), With<FormPanel>>,
) -> Result {
    if !(form.is_changed() || theme.is_changed()) {
        return Ok(());
    }
    let (candidates, selected) = form
        .state()
        .map_or((&[][..], None), super::state::FormState::candidates);
    if candidates.is_empty() {
        if let Ok((entity, _)) = widgets.single_mut() {
            commands.entity(entity).despawn();
        }
        return Ok(());
    }
    let (lines, height) = visible_lines(candidates, selected, &theme);
    let render = PanelRender {
        lines,
        background: theme.base.default.normal.style(),
    };
    let panel_layout = WidgetLayout::from(layout::bottom_panel_layout(height));
    match widgets.single_mut() {
        Ok((entity, mut widget)) => {
            widget.set_state(render)?;
            commands.entity(entity).insert(panel_layout);
        }
        Err(_) => {
            commands.spawn((
                FormPanel,
                Widget::from_render_fn_with_state(render_panel, render),
                WidgetOrder(layer::PANEL),
                panel_layout,
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
        .map(|(index, entry)| {
            let style = if Some(index) == selected {
                states.selected.style()
            } else {
                states.normal.style()
            };
            Line::styled(format!(" {entry}"), style)
        })
        .collect();
    (lines, height as u16)
}

fn render_panel(frame: &mut ratatui::Frame, area: Rect, state: &mut PanelRender) -> Result {
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new(state.lines.clone()).style(state.background),
        area,
    );
    Ok(())
}
