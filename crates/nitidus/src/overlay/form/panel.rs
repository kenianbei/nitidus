//! Completion candidates for the focused field, in a panel directly
//! under it — the answer belongs where the question is, not at the
//! bottom of the screen.
//!
//! Spawns while candidates exist and despawns otherwise, so the widgets
//! underneath repaint their cells.

use bevy::prelude::*;
use nitidus_ui_kit::layer;
use nitidus_ui_kit::theme::Theme;
use plurimus::{Widget, WidgetLayout, WidgetOrder};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::ActiveForm;
use super::geometry::{FormLayout, Slot};
use super::state::{Focus, FormState};

const MAX_PANEL_ROWS: u16 = 8;
/// The panel is at least this wide even under a cramped field, so a
/// candidate is readable rather than clipped to nothing.
const MIN_PANEL_WIDTH: u16 = 24;

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
    let Some(anchor) = form.state().and_then(field_anchor) else {
        return Ok(());
    };
    let panel_layout = WidgetLayout::new(move |area| panel_rect(&anchor, *area, height));
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

/// Where the panel hangs: the layout of the form and the field that
/// asked for it.
fn field_anchor(state: &FormState) -> Option<(FormLayout, usize)> {
    let Focus::Field(index) = state.focus() else {
        return None;
    };
    Some((FormLayout::of(state), index))
}

/// Directly under the field, left-aligned with it. A field near the
/// bottom would push the panel off the screen, so it is clamped back
/// inside rather than flipped above — the list stays where the eye
/// already is.
fn panel_rect(anchor: &(FormLayout, usize), area: Rect, height: u16) -> Rect {
    let (layout, index) = anchor;
    let field = layout.slot(area, Slot::Field(*index));
    if field == Rect::ZERO {
        return Rect::ZERO;
    }
    let width = field.width.max(MIN_PANEL_WIDTH).min(area.width);
    let x = field.x.min(area.width.saturating_sub(width));
    let below = field.bottom();
    let y = below.min(area.height.saturating_sub(height));
    Rect {
        x,
        y,
        width,
        height,
    }
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
