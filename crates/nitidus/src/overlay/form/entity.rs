//! The form's entity set. One entity per control, because plurimus
//! hit-tests `WidgetRect`s and a `WidgetRect` comes from a
//! `WidgetLayout` — per-field entities are what hover, press, and
//! click-to-focus require, not a stylistic choice.

use bevy::prelude::*;
use nitidus_ui_kit::theme::Theme;
use plurimus::{
    UiFocusMessage, UiFocusable, UiHoverable, UiPressable, Widget, WidgetLayout, WidgetOrder,
};

use std::sync::Arc;

use super::geometry::{FormLayout, Slot, step_rects, step_widths};
use super::render::{
    ButtonView, FieldView, FieldViewKind, FrameView, MessageView, StepView, render_button,
    render_field, render_frame, render_message, render_step,
};
use super::state::{Focus, FormState};
use super::{ActiveForm, mouse};

/// Carries the control-set generation it was spawned for, so a change
/// of shape respawns rather than being diffed component by component.
#[derive(Component)]
pub(super) struct FormEntity(u64);

#[derive(Component)]
pub(super) struct FormFieldControl(pub(super) usize);

#[derive(Component)]
pub(super) struct FormButtonControl(pub(super) usize);

#[derive(Component)]
pub(super) struct FormStepControl(pub(super) usize);

#[derive(Component)]
pub(super) struct FormMessageRow;

pub(super) fn sync_form_entities(
    mut commands: Commands,
    form: Res<ActiveForm>,
    theme: Res<Theme>,
    existing: Query<(Entity, &FormEntity)>,
    mut focus: MessageWriter<UiFocusMessage>,
) {
    if !form.is_changed() {
        return;
    }
    let spawned = existing.iter().next().map(|(_, marker)| marker.0);
    let wanted = form.state().map(FormState::generation);
    if spawned == wanted {
        return;
    }
    for (entity, _) in &existing {
        commands.entity(entity).despawn();
    }
    match form.state() {
        Some(state) => spawn_form(&mut commands, state, &theme),
        None => {
            focus.write(UiFocusMessage::clear());
        }
    }
}

fn spawn_form(commands: &mut Commands, state: &FormState, theme: &Theme) {
    let labels = state.button_labels();
    let layout = FormLayout::of(state);
    spawn_frame(commands, state, theme, &layout);
    spawn_steps(commands, state, theme, &layout);
    for (index, field) in state.fields.iter().enumerate() {
        let view = FieldView::new(field.spec.label.clone(), field_view_kind(field), theme);
        let slots = layout.clone();
        commands.spawn((
            FormEntity(state.generation()),
            FormFieldControl(index),
            Widget::from_render_fn_with_state(render_field, view),
            WidgetLayout::new(move |area| slots.slot(*area, Slot::Field(index))),
            WidgetOrder(layout.control_order()),
            UiFocusable::new(index as i32),
            UiHoverable,
            plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
                mouse::handle_field,
            )]),
        ));
    }
    for (index, label) in labels.into_iter().enumerate() {
        spawn_button(
            commands,
            theme,
            &layout,
            ButtonSlot {
                index,
                label,
                generation: state.generation(),
            },
        );
    }
}

fn field_view_kind(field: &super::field::FieldRuntime) -> FieldViewKind {
    if let Some(text) = field.area() {
        return FieldViewKind::Body {
            text,
            styles: Vec::new(),
        };
    }
    if field.spec.is_select() {
        return FieldViewKind::Select;
    }
    if field.spec.is_entries() {
        return FieldViewKind::Entries {
            entries: Vec::new(),
            selected: 0,
            empty_label: field.spec.empty_label().to_owned(),
        };
    }
    FieldViewKind::Text {
        masked: field.spec.is_masked(),
    }
}

/// Steps are entities like any other control, so an unreached one takes
/// `UiDisabled` and plurimus refuses it both focus and the pointer.
fn spawn_steps(commands: &mut Commands, state: &FormState, theme: &Theme, layout: &FormLayout) {
    if !state.has_strip() {
        return;
    }
    let steps = state.steps();
    let widths = Arc::new(step_widths(
        &steps
            .iter()
            .map(|(title, _)| title.clone())
            .collect::<Vec<_>>(),
    ));
    for (index, (title, step)) in steps.into_iter().enumerate() {
        let widths = Arc::clone(&widths);
        let slots = layout.clone();
        let mut entity = commands.spawn((
            FormEntity(state.generation()),
            FormStepControl(index),
            Widget::from_render_fn_with_state(render_step, StepView::new(title, step, theme)),
            WidgetLayout::new(move |area| {
                let strip = slots.geometry(*area).strip;
                step_rects(&strip, &widths)
                    .get(index)
                    .copied()
                    .unwrap_or(ratatui::layout::Rect::ZERO)
            }),
            WidgetOrder(layout.control_order()),
            UiHoverable,
            plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
                mouse::handle_step,
            )]),
        ));
        if step == super::state::StepState::Unreached {
            entity.insert(plurimus::UiDisabled);
        }
    }
}

fn spawn_frame(commands: &mut Commands, state: &FormState, theme: &Theme, layout: &FormLayout) {
    let frame_slots = layout.clone();
    commands.spawn((
        FormEntity(state.generation()),
        Widget::from_render_fn_with_state(render_frame, FrameView::new(state.title.clone(), theme)),
        WidgetLayout::new(move |area| frame_slots.slot(*area, Slot::Frame)),
        WidgetOrder(layout.frame_order()),
        UiHoverable,
    ));
    let message_slots = layout.clone();
    commands.spawn((
        FormEntity(state.generation()),
        FormMessageRow,
        Widget::from_render_fn_with_state(render_message, MessageView::new(None, theme)),
        WidgetLayout::new(move |area| message_slots.slot(*area, Slot::Message)),
        WidgetOrder(layout.control_order()),
    ));
}

struct ButtonSlot {
    index: usize,
    label: String,
    generation: u64,
}

fn spawn_button(commands: &mut Commands, theme: &Theme, layout: &FormLayout, button: ButtonSlot) {
    let ButtonSlot {
        index,
        label,
        generation,
    } = button;
    let tab_index = layout.metrics.fields.len() + index;
    let slots = layout.clone();
    commands.spawn((
        FormEntity(generation),
        FormButtonControl(index),
        Widget::from_render_fn_with_state(render_button, ButtonView::new(label, theme)),
        WidgetLayout::new(move |area| slots.slot(*area, Slot::Button(index))),
        WidgetOrder(layout.control_order()),
        UiFocusable::new(tab_index as i32),
        UiHoverable,
        UiPressable,
        plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
            mouse::handle_button,
        )]),
    ));
}

/// Pushes the form's own focus outward so plurimus styles and hit-tests
/// against the same control the keyboard is editing.
pub(super) fn mirror_focus(
    form: Res<ActiveForm>,
    fields: Query<(Entity, &FormFieldControl)>,
    buttons: Query<(Entity, &FormButtonControl)>,
    mut focus: MessageWriter<UiFocusMessage>,
) {
    if !form.is_changed() {
        return;
    }
    let Some(state) = form.state() else {
        return;
    };
    let target = match state.focus() {
        Focus::Field(index) => entity_at(fields.iter().map(|(e, c)| (e, c.0)), index),
        Focus::Button(index) => entity_at(buttons.iter().map(|(e, c)| (e, c.0)), index),
    };
    if let Some(entity) = target {
        focus.write(UiFocusMessage::set(entity));
    }
}

fn entity_at(controls: impl Iterator<Item = (Entity, usize)>, index: usize) -> Option<Entity> {
    controls
        .filter(|(_, control)| *control == index)
        .map(|(entity, _)| entity)
        .next()
}

pub(super) fn refresh_form(
    form: Res<ActiveForm>,
    theme: Res<Theme>,
    mut controls: Query<(
        &mut Widget,
        Option<&FormFieldControl>,
        Option<&FormButtonControl>,
        Option<&FormStepControl>,
        Has<FormMessageRow>,
    )>,
) -> Result {
    if !form.is_changed() && !theme.is_changed() {
        return Ok(());
    }
    let Some(state) = form.state() else {
        return Ok(());
    };
    for (mut widget, field, button, step, is_message) in &mut controls {
        if let Some(FormFieldControl(index)) = field {
            refresh_field(&mut widget, state, *index, &theme)?;
        } else if let Some(FormButtonControl(index)) = button {
            refresh_button(&mut widget, state, *index)?;
        } else if let Some(FormStepControl(index)) = step {
            refresh_step(&mut widget, state, *index)?;
        } else if is_message {
            widget.set_state(MessageView::new(state.message().map(str::to_owned), &theme))?;
        }
    }
    Ok(())
}

fn refresh_step(widget: &mut Widget, state: &FormState, index: usize) -> Result {
    let Some((title, step)) = state.steps().into_iter().nth(index) else {
        return Ok(());
    };
    let view = widget.get_state_mut::<StepView>()?;
    view.title = title;
    view.state = step;
    Ok(())
}

fn refresh_field(widget: &mut Widget, state: &FormState, index: usize, theme: &Theme) -> Result {
    let Some(field) = state.fields.get(index) else {
        return Ok(());
    };
    let focused = state.focus() == Focus::Field(index);
    let view = widget.get_state_mut::<FieldView>()?;
    match field.selected() {
        Some(option) => {
            view.value = option.label.clone();
            view.detail = option.detail.clone();
        }
        None => {
            view.value = field.value();
            view.cursor = field.cursor();
        }
    }
    if let FieldViewKind::Body { styles, .. } = &mut view.kind {
        *styles = field.line_styles(theme);
    }
    if let (
        FieldViewKind::Entries {
            entries, selected, ..
        },
        Some((live, picked)),
    ) = (&mut view.kind, field.entries())
    {
        *entries = live.to_vec();
        *selected = picked;
    }
    field.apply_theme(theme, focused);
    view.focused = focused;
    view.read_only = field.spec.read_only;
    view.is_error = state.error_field() == Some(index);
    Ok(())
}

fn refresh_button(widget: &mut Widget, state: &FormState, index: usize) -> Result {
    let view = widget.get_state_mut::<ButtonView>()?;
    view.focused = state.focus() == Focus::Button(index);
    Ok(())
}
