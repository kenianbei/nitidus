#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy_ratatui::crossterm::event::KeyCode;

use super::*;
use crate::overlay::form::spec::{FieldSpec, SelectOption};

const IDS: [&str; 3] = ["one", "two", "three"];

fn single_page(field_count: usize) -> FormState {
    let fields = IDS
        .iter()
        .take(field_count)
        .map(|id| FieldSpec::text(id, *id))
        .collect();
    FormState::new(FormSpec::new("Test", "Create", fields, Box::new(|_, _| {})))
}

/// Two fixed pages plus a third that only exists when `kind` is
/// "custom" — the branching shape the wizard needs.
fn branching(mode: FormMode) -> FormState {
    let pages: PagesFn = Box::new(|values| {
        let mut pages = vec![
            PageSpec::new(
                "account",
                "Account",
                vec![FieldSpec::text("name", "Name").validated(|value| {
                    if value.is_empty() {
                        Err("name is required".to_owned())
                    } else {
                        Ok(())
                    }
                })],
            ),
            PageSpec::new(
                "provider",
                "Provider",
                vec![FieldSpec::select(
                    "kind",
                    "Kind",
                    vec![
                        SelectOption::new("gmail", "Gmail"),
                        SelectOption::new("custom", "Custom"),
                    ],
                )],
            ),
        ];
        if values.get("kind") == "custom" {
            pages.push(PageSpec::new(
                "servers",
                "Servers",
                vec![FieldSpec::text("host", "IMAP host")],
            ));
        }
        pages
    });
    let mut spec = FormSpec::paged("Account", "Create", pages, Box::new(|_, _| {}));
    spec.mode = mode;
    FormState::new(spec)
}

fn press(state: &mut FormState, code: KeyCode) {
    state.edit_focused(KeyEvent::from(code));
}

fn type_str(state: &mut FormState, text: &str) {
    for character in text.chars() {
        press(state, KeyCode::Char(character));
    }
}

fn step_states(state: &FormState) -> Vec<StepState> {
    state.steps().into_iter().map(|(_, step)| step).collect()
}

#[test]
fn focus_starts_on_the_first_field() {
    assert_eq!(single_page(3).focus(), Focus::Field(0));
}

#[test]
fn a_page_without_fields_focuses_the_primary_button() {
    let state = single_page(0);
    assert_eq!(state.focus(), Focus::Button(1));
    assert_eq!(state.role_at(0), Some(ButtonRole::Cancel));
    assert_eq!(state.role_at(1), Some(ButtonRole::Primary));
}

#[test]
fn focus_walks_fields_then_buttons_and_wraps() {
    let mut state = single_page(2);
    let order = [
        Focus::Field(1),
        Focus::Button(0),
        Focus::Button(1),
        Focus::Field(0),
    ];
    for expected in order {
        state.move_focus(true);
        assert_eq!(state.focus(), expected);
    }
}

#[test]
fn typing_lands_in_the_focused_field_only() {
    let mut state = single_page(2);
    type_str(&mut state, "a");
    state.move_focus(true);
    type_str(&mut state, "b");
    let values = state.values();
    assert_eq!(values.get("one"), "a");
    assert_eq!(values.get("two"), "b");
}

#[test]
fn enter_and_escape_never_reach_the_field_state() {
    let mut state = single_page(1);
    type_str(&mut state, "a");
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.fields[0].status(),
        Some(tui_prompts::Status::Pending),
        "the form owns submit and cancel, not the field"
    );
    assert_eq!(state.values().get("one"), "a");
}

#[test]
fn a_single_page_form_has_no_strip_and_no_back_button() {
    let state = single_page(1);
    assert!(!state.has_strip());
    assert_eq!(
        state.button_labels(),
        vec!["Cancel".to_owned(), "Create".to_owned()]
    );
    assert!(!state.primary_advances(), "one page submits immediately");
}

#[test]
fn creating_advances_through_pages_and_saves_on_the_last() {
    let mut state = branching(FormMode::Create);
    assert!(state.has_strip());
    assert!(
        state.primary_advances(),
        "the primary is Next until the end"
    );
    assert_eq!(state.button_labels().last().unwrap(), "Next");

    type_str(&mut state, "work");
    assert!(state.next_page());
    assert_eq!(state.page(), 1);
    assert_eq!(
        state.button_labels()[1],
        "Back",
        "Back appears after page 0"
    );
    assert!(
        !state.primary_advances(),
        "the last page offers the real primary action"
    );
    assert_eq!(state.button_labels().last().unwrap(), "Create");
}

#[test]
fn creating_refuses_to_advance_past_an_invalid_page() {
    let mut state = branching(FormMode::Create);
    assert!(!state.next_page(), "the name is empty");
    assert_eq!(state.page(), 0);
    assert_eq!(state.message(), Some("name is required"));
    assert_eq!(state.error_field(), Some(0));

    type_str(&mut state, "work");
    assert_eq!(state.message(), None, "typing clears the complaint");
    assert!(state.next_page());
}

#[test]
fn creating_cannot_jump_to_a_step_it_has_not_reached() {
    let mut state = branching(FormMode::Create);
    assert_eq!(
        step_states(&state),
        vec![StepState::Current, StepState::Unreached]
    );
    assert!(!state.go_to_page(1), "gated until walked through");
    assert_eq!(state.page(), 0);

    type_str(&mut state, "work");
    state.next_page();
    assert!(state.prev_page(), "Back is always available");
    assert_eq!(state.page(), 0);
    assert_eq!(
        step_states(&state),
        vec![StepState::Current, StepState::Reached],
        "a step already walked stays reachable"
    );
    assert!(state.go_to_page(1));
}

#[test]
fn editing_reaches_every_step_at_once_and_saves_from_anywhere() {
    let mut state = branching(FormMode::Edit);
    assert_eq!(
        step_states(&state),
        vec![StepState::Current, StepState::Reached],
        "editing is not a walk"
    );
    assert!(state.go_to_page(1));
    assert!(
        !state.primary_advances(),
        "editing saves rather than advancing"
    );
}

#[test]
fn a_select_flip_adds_and_removes_a_page() {
    let mut state = branching(FormMode::Edit);
    assert_eq!(state.steps().len(), 2);
    state.go_to_page(1);

    state.move_cursor(Cursor::Right);
    assert_eq!(state.values().get("kind"), "custom");
    assert_eq!(state.steps().len(), 3, "Custom needs a Servers page");
    assert_eq!(state.steps()[2].0, "Servers");

    state.move_cursor(Cursor::Left);
    assert_eq!(state.steps().len(), 2, "and loses it again");
}

#[test]
fn values_survive_a_page_switch_and_a_shape_change() {
    let mut state = branching(FormMode::Edit);
    type_str(&mut state, "work");
    state.go_to_page(1);
    state.move_cursor(Cursor::Right);
    state.go_to_page(2);
    type_str(&mut state, "mail.example.net");
    state.go_to_page(0);

    let values = state.values();
    assert_eq!(values.get("name"), "work", "typed on a page since left");
    assert_eq!(values.get("kind"), "custom");
    assert_eq!(values.get("host"), "mail.example.net");
}

#[test]
fn typing_does_not_rebuild_the_control_set() {
    let mut state = branching(FormMode::Edit);
    let generation = state.generation();
    type_str(&mut state, "work");
    assert_eq!(
        state.generation(),
        generation,
        "the shape did not change, so entities must not churn"
    );

    state.go_to_page(1);
    let generation = state.generation();
    state.move_cursor(Cursor::Right);
    assert_ne!(
        state.generation(),
        generation,
        "a select that adds a page does change the shape"
    );
}

#[test]
fn saving_with_an_error_on_another_page_jumps_back_to_it() {
    let mut state = branching(FormMode::Edit);
    state.go_to_page(1);
    assert_eq!(state.page(), 1);

    assert!(!state.validate_all(), "the name on page 0 is still empty");
    assert_eq!(state.page(), 0, "the offending page comes to the front");
    assert_eq!(state.focus(), Focus::Field(0));
    assert_eq!(state.message(), Some("name is required"));

    type_str(&mut state, "work");
    assert!(state.validate_all());
}

#[test]
fn a_removed_page_never_leaves_the_cursor_out_of_range() {
    let mut state = branching(FormMode::Edit);
    state.go_to_page(1);
    state.move_cursor(Cursor::Right);
    state.go_to_page(2);
    assert_eq!(state.page(), 2);

    state.go_to_page(1);
    state.move_cursor(Cursor::Left);
    assert_eq!(state.steps().len(), 2);
    assert!(state.page() < state.steps().len());
}
