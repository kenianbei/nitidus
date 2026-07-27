//! Entities for an open confirmation: one frame plus one per button.
//! Buttons are separate entities because that is what plurimus hover,
//! press, and click-to-focus require.

use bevy::prelude::*;
use nitidus_ui_kit::layer;
use nitidus_ui_kit::theme::Theme;
use plurimus::{
    UiDisabled, UiEvent, UiHoverable, UiHovered, UiPressed, Widget, WidgetLayout, WidgetOrder,
};

use super::render::{
    ButtonView, FrameView, button_width, confirm_geometry, render_button, render_frame,
};
use super::{ActiveConfirm, ConfirmState, activate, focus_button};
use crate::mouse::local_event;
use crate::overlay::interaction::Interaction;

const FRAME_ORDER: i32 = layer::MODAL;
const CONTROL_ORDER: i32 = layer::MODAL + 1;

#[derive(Component)]
pub(super) struct ConfirmEntity;

#[derive(Component)]
pub(super) struct ConfirmButton(usize);

pub(super) fn sync_confirm_entities(
    mut commands: Commands,
    confirm: Res<ActiveConfirm>,
    theme: Res<Theme>,
    existing: Query<Entity, With<ConfirmEntity>>,
) {
    if !confirm.is_changed() {
        return;
    }
    let is_spawned = existing.iter().next().is_some();
    match (&confirm.0, is_spawned) {
        (Some(state), false) => spawn(&mut commands, state, &theme),
        (None, true) => {
            for entity in &existing {
                commands.entity(entity).despawn();
            }
        }
        _ => {}
    }
}

fn spawn(commands: &mut Commands, state: &ConfirmState, theme: &Theme) {
    let labels = state.button_labels();
    let width = button_width(&labels);
    let detail_rows = state.detail.len();
    commands.spawn((
        ConfirmEntity,
        Widget::from_render_fn_with_state(
            render_frame,
            FrameView::new(
                state.title.clone(),
                state.question.clone(),
                state.detail.clone(),
                width,
                theme,
            ),
        ),
        WidgetLayout::new(move |area| confirm_geometry(*area, detail_rows, width).frame),
        WidgetOrder(FRAME_ORDER),
    ));
    for (index, label) in labels.into_iter().enumerate() {
        commands.spawn((
            ConfirmEntity,
            ConfirmButton(index),
            Widget::from_render_fn_with_state(render_button, ButtonView::new(label, theme)),
            WidgetLayout::new(move |area| {
                confirm_geometry(*area, detail_rows, width).buttons[index]
            }),
            WidgetOrder(CONTROL_ORDER),
            UiHoverable,
            plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
                handle_button,
            )]),
        ));
    }
}

/// Keyboard focus is the confirmation's own; the pointer only writes
/// `Interaction`. Same split as the form.
pub(super) fn refresh_confirm(
    confirm: Res<ActiveConfirm>,
    mut buttons: Query<(&mut Widget, &ConfirmButton)>,
) -> Result {
    let Some(state) = &confirm.0 else {
        return Ok(());
    };
    for (mut widget, ConfirmButton(index)) in &mut buttons {
        let view = widget.get_state_mut::<ButtonView>()?;
        view.focused = state.focused == *index;
    }
    Ok(())
}

pub(super) fn sync_interaction(
    mut controls: Query<
        (&mut Widget, Has<UiHovered>, Has<UiPressed>, Has<UiDisabled>),
        With<ConfirmButton>,
    >,
) {
    for (mut widget, hovered, pressed, disabled) in &mut controls {
        if let Ok(button) = widget.get_state_mut::<ButtonView>() {
            button.interaction = Interaction {
                hovered,
                pressed,
                disabled,
            };
        }
    }
}

/// Fires on release, and only while the pointer is still over the
/// button it pressed.
fn handle_button(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    let Some(&ConfirmButton(index)) = world.get::<ConfirmButton>(entity) else {
        return Ok(());
    };
    if local.is_left_click() {
        focus_button(world, index);
        return Ok(());
    }
    if local.is_left_release() {
        focus_button(world, index);
        activate(world);
    }
    Ok(())
}
