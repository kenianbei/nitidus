#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy_ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use nitidus_ui_kit::theme::tailwind_dark;
use plurimus::{UiEvent, WidgetRect};
use ratatui::layout::Rect;

use super::entity::{FormButtonControl, FormFieldControl, FormStepControl};
use super::panel;
use super::render::{ButtonView, FieldView, FieldViewKind};
use super::*;
use crate::config::RawKeymaps;

const TERMINAL: Rect = Rect {
    x: 0,
    y: 0,
    width: 100,
    height: 30,
};

#[derive(Resource, Default)]
struct Submitted(Option<FormValues>);

#[derive(Resource, Default)]
struct Cancelled(bool);

fn form_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(tailwind_dark());
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.init_resource::<Submitted>();
    app.init_resource::<Cancelled>();
    app.add_plugins(FormPlugin);
    app.update();
    app
}

fn spec(fields: Vec<FieldSpec>) -> FormSpec {
    FormSpec::new(
        "Account",
        "Create",
        fields,
        Box::new(|world, values| world.resource_mut::<Submitted>().0 = Some(values)),
    )
    .with_cancel(|world: &mut World| {
        world.resource_mut::<Cancelled>().0 = true;
        CancelOutcome::Close
    })
}

fn two_field_spec() -> FormSpec {
    spec(vec![
        FieldSpec::text("name", "Name"),
        FieldSpec::text("email", "Email"),
    ])
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(app.world_mut(), KeyEvent::from(code)).unwrap();
}

fn press_shift_tab(app: &mut App) {
    let key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    handle_key(app.world_mut(), key).unwrap();
}

fn type_str(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

/// plurimus computes `WidgetRect` while drawing; these tests never draw,
/// so the layout functions are applied directly against a fixed
/// terminal to give the mouse something to hit.
fn apply_layouts(app: &mut App) {
    let entities: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<plurimus::WidgetLayout>>()
        .iter(app.world())
        .collect();
    for entity in entities {
        let layout = app
            .world()
            .get::<plurimus::WidgetLayout>(entity)
            .unwrap()
            .0
            .clone();
        let rect = layout(&TERMINAL);
        app.world_mut().entity_mut(entity).insert(WidgetRect(rect));
    }
}

fn click(app: &mut App, kind: MouseEventKind, entity: Entity) {
    let rect = app.world().get::<WidgetRect>(entity).unwrap().0;
    let event = UiEvent::Mouse(MouseEvent {
        kind,
        column: rect.x + rect.width / 2,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    let handler = if app.world().get::<FormButtonControl>(entity).is_some() {
        mouse::handle_button
    } else {
        mouse::handle_field
    };
    handler(app.world_mut(), entity, event).unwrap();
}

fn field_entity(app: &mut App, index: usize) -> Entity {
    control_entity(app, |control: &FormFieldControl| control.0 == index)
}

fn button_entity(app: &mut App, index: usize) -> Entity {
    control_entity(app, |control: &FormButtonControl| control.0 == index)
}

fn control_entity<C: Component>(app: &mut App, matches: impl Fn(&C) -> bool) -> Entity {
    app.world_mut()
        .query::<(Entity, &C)>()
        .iter(app.world())
        .find(|(_, control)| matches(control))
        .map(|(entity, _)| entity)
        .expect("control not spawned")
}

fn field_view(app: &App, entity: Entity) -> FieldView {
    app.world()
        .get::<plurimus::Widget>(entity)
        .unwrap()
        .get_state::<FieldView>()
        .unwrap()
        .clone()
}

fn rendered(width: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> String {
    let backend = ratatui::backend::TestBackend::new(width, 1);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(draw).unwrap();
    format!("{:?}", terminal.backend().buffer())
}

#[test]
fn opening_spawns_a_control_per_field_plus_two_buttons() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();

    let fields = app
        .world_mut()
        .query::<&FormFieldControl>()
        .iter(app.world())
        .count();
    let buttons = app
        .world_mut()
        .query::<&FormButtonControl>()
        .iter(app.world())
        .count();
    assert_eq!(fields, 2);
    assert_eq!(buttons, 2, "Cancel and the primary button");
}

#[test]
fn cancelling_despawns_every_control() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();
    press(&mut app, KeyCode::Esc);
    app.update();

    assert!(app.world().resource::<Cancelled>().0);
    let remaining = app
        .world_mut()
        .query::<&FormFieldControl>()
        .iter(app.world())
        .count();
    assert_eq!(remaining, 0, "a closed form leaves no entities behind");
}

#[test]
fn tab_walks_the_controls_and_typing_follows_focus() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    type_str(&mut app, "work");
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "me@x.example");
    press(&mut app, KeyCode::Enter);

    let values = app.world().resource::<Submitted>().0.clone().unwrap();
    assert_eq!(values.get("name"), "work");
    assert_eq!(values.get("email"), "me@x.example");
}

/// Terminals send Shift-Tab as `BackTab`, not as Tab with a shift
/// modifier, so this presses what a real terminal actually emits.
#[test]
fn shift_tab_walks_backwards_onto_the_primary_button() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();
    press_shift_tab(&mut app);
    // Discriminating: if focus had not moved, this would type into the
    // first field instead of being swallowed by a button.
    type_str(&mut app, "x");
    assert_eq!(
        app.world().resource::<ActiveForm>().value("name").unwrap(),
        "",
        "focus must have left the first field"
    );
    press(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<Submitted>().0.is_some(),
        "Enter on the primary button submits"
    );
}

#[test]
fn shift_tab_and_tab_are_inverses() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    press(&mut app, KeyCode::Tab);
    press_shift_tab(&mut app);
    type_str(&mut app, "back");
    assert_eq!(
        app.world().resource::<ActiveForm>().value("name").unwrap(),
        "back",
        "Tab then Shift-Tab returns to where it started"
    );
}

#[test]
fn enter_on_the_cancel_button_cancels_instead_of_submitting() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    // Two fields, then Cancel: two tabs from the first field.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);

    assert!(app.world().resource::<Cancelled>().0);
    assert!(app.world().resource::<Submitted>().0.is_none());
}

#[test]
fn a_failing_validator_holds_the_form_open_and_focuses_the_offender() {
    let mut app = form_app();
    let fields = vec![
        FieldSpec::text("name", "Name").with_initial("work"),
        FieldSpec::text("email", "Email").validated(|value| {
            if value.contains('@') {
                Ok(())
            } else {
                Err("email must contain @".to_owned())
            }
        }),
    ];
    open_form(app.world_mut(), spec(fields));
    press(&mut app, KeyCode::Enter);
    app.update();

    assert!(app.world().resource::<Submitted>().0.is_none());
    let form = app.world().resource::<ActiveForm>();
    assert!(form.is_open());
    assert_eq!(form.message(), Some("email must contain @"));

    let entity = field_entity(&mut app, 1);
    let view = field_view(&app, entity);
    assert!(view.focused, "the offending field takes focus");
    assert!(view.is_error, "and is marked so the label can say why");

    type_str(&mut app, "me@x.example");
    press(&mut app, KeyCode::Enter);
    assert!(app.world().resource::<Submitted>().0.is_some());
}

#[test]
fn clicking_a_field_focuses_it() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();
    apply_layouts(&mut app);

    let second = field_entity(&mut app, 1);
    click(&mut app, MouseEventKind::Down(MouseButton::Left), second);
    type_str(&mut app, "clicked");

    let value = app.world().resource::<ActiveForm>().value("email").unwrap();
    assert_eq!(value, "clicked", "typing follows the clicked field");
}

#[test]
fn a_button_fires_on_release_not_on_press() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();
    apply_layouts(&mut app);

    let primary = button_entity(&mut app, 1);
    click(&mut app, MouseEventKind::Down(MouseButton::Left), primary);
    assert!(
        app.world().resource::<Submitted>().0.is_none(),
        "pressing must not commit"
    );
    click(&mut app, MouseEventKind::Up(MouseButton::Left), primary);
    assert!(app.world().resource::<Submitted>().0.is_some());
}

#[test]
fn a_release_that_drifts_off_the_button_does_not_fire_it() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();
    apply_layouts(&mut app);

    let primary = button_entity(&mut app, 1);
    click(&mut app, MouseEventKind::Down(MouseButton::Left), primary);
    let away = UiEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    mouse::handle_button(app.world_mut(), primary, away).unwrap();
    assert!(
        app.world().resource::<Submitted>().0.is_none(),
        "plurimus routes the release to the control that captured the \
         press, so the handler must check the pointer is still over it"
    );
}

#[test]
fn a_masked_field_renders_asterisks_rather_than_the_secret() {
    let mut app = form_app();
    open_form(
        app.world_mut(),
        spec(vec![FieldSpec::text("secret", "Password").masked()]),
    );
    type_str(&mut app, "hunter2");
    app.update();

    let entity = field_entity(&mut app, 0);
    let mut view = field_view(&app, entity);
    let output = rendered(60, |frame| {
        render::render_field(frame, frame.area(), &mut view).unwrap();
    });
    assert!(output.contains("*******"), "{output}");
    assert!(!output.contains("hunter2"), "{output}");
}

#[test]
fn a_field_renders_its_label_beside_the_value() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    type_str(&mut app, "work");
    app.update();

    let entity = field_entity(&mut app, 0);
    let mut view = field_view(&app, entity);
    let output = rendered(60, |frame| {
        render::render_field(frame, frame.area(), &mut view).unwrap();
    });
    assert!(output.contains("Name"), "{output}");
    assert!(output.contains("work"), "{output}");
}

#[test]
fn the_primary_button_renders_the_label_the_spec_gave_it() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();

    let entity = button_entity(&mut app, 1);
    let mut view = app
        .world()
        .get::<plurimus::Widget>(entity)
        .unwrap()
        .get_state::<ButtonView>()
        .unwrap()
        .clone();
    assert_eq!(view.label, "Create");
    let output = rendered(12, |frame| {
        render::render_button(frame, frame.area(), &mut view).unwrap();
    });
    assert!(output.contains("Create"), "{output}");
}

#[test]
fn global_bindings_do_not_leak_through_an_open_form() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    // `q` is the global quit; inside a form it is just a character.
    type_str(&mut app, "q");
    assert!(
        app.world().resource::<Messages<AppExit>>().is_empty(),
        "a modal must not fall through to global bindings"
    );
    assert_eq!(
        app.world().resource::<ActiveForm>().value("name").unwrap(),
        "q"
    );
}

fn provider_field() -> FieldSpec {
    FieldSpec::select(
        "provider",
        "Provider",
        vec![
            SelectOption::new("gmail", "Gmail").with_detail("imap.gmail.com"),
            SelectOption::new("outlook", "Outlook").with_detail("outlook.office365.com"),
            SelectOption::new("custom", "Custom IMAP"),
        ],
    )
}

#[test]
fn a_select_starts_on_its_first_option_and_submits_that_value() {
    let mut app = form_app();
    open_form(app.world_mut(), spec(vec![provider_field()]));
    press(&mut app, KeyCode::Enter);

    let values = app.world().resource::<Submitted>().0.clone().unwrap();
    assert_eq!(values.get("provider"), "gmail");
}

#[test]
fn a_select_honours_an_initial_value_naming_an_option() {
    let mut app = form_app();
    open_form(
        app.world_mut(),
        spec(vec![provider_field().with_initial("custom")]),
    );
    press(&mut app, KeyCode::Enter);
    let values = app.world().resource::<Submitted>().0.clone().unwrap();
    assert_eq!(values.get("provider"), "custom");
}

#[test]
fn an_unknown_initial_value_falls_back_to_the_first_option() {
    let mut app = form_app();
    open_form(
        app.world_mut(),
        spec(vec![provider_field().with_initial("fastmail")]),
    );
    press(&mut app, KeyCode::Enter);
    let values = app.world().resource::<Submitted>().0.clone().unwrap();
    assert_eq!(values.get("provider"), "gmail");
}

#[test]
fn right_and_left_cycle_the_options_and_wrap_both_ways() {
    let mut app = form_app();
    open_form(app.world_mut(), spec(vec![provider_field()]));

    let value = |app: &App| {
        app.world()
            .resource::<ActiveForm>()
            .value("provider")
            .unwrap()
    };
    press(&mut app, KeyCode::Right);
    assert_eq!(value(&app), "outlook");
    press(&mut app, KeyCode::Right);
    assert_eq!(value(&app), "custom");
    press(&mut app, KeyCode::Right);
    assert_eq!(value(&app), "gmail", "cycling wraps forward");
    press(&mut app, KeyCode::Left);
    assert_eq!(value(&app), "custom", "and backward");
}

#[test]
fn typing_into_a_select_changes_nothing() {
    let mut app = form_app();
    open_form(app.world_mut(), spec(vec![provider_field()]));
    type_str(&mut app, "outlook");
    assert_eq!(
        app.world()
            .resource::<ActiveForm>()
            .value("provider")
            .unwrap(),
        "gmail",
        "a select is cycled, not typed into"
    );
}

#[test]
fn a_select_renders_its_label_and_detail_not_its_stored_value() {
    let mut app = form_app();
    open_form(app.world_mut(), spec(vec![provider_field()]));
    app.update();

    let entity = field_entity(&mut app, 0);
    let mut view = field_view(&app, entity);
    assert!(matches!(view.kind, FieldViewKind::Select));
    let output = rendered(70, |frame| {
        render::render_field(frame, frame.area(), &mut view).unwrap();
    });
    assert!(output.contains("Gmail"), "{output}");
    assert!(
        output.contains("imap.gmail.com"),
        "the detail explains it: {output}"
    );
}

#[test]
fn a_narrow_row_drops_the_detail_rather_than_wrapping_it() {
    let mut app = form_app();
    open_form(app.world_mut(), spec(vec![provider_field()]));
    app.update();

    let entity = field_entity(&mut app, 0);
    let mut view = field_view(&app, entity);
    let output = rendered(28, |frame| {
        render::render_field(frame, frame.area(), &mut view).unwrap();
    });
    assert!(
        output.contains("Gmail"),
        "the choice always survives: {output}"
    );
    assert!(!output.contains("imap.gmail.com"), "{output}");
}

/// Account → Servers, where Servers only exists for a custom provider.
fn paged_spec(mode: FormMode) -> FormSpec {
    let pages: PagesFn = Box::new(|values| {
        let mut pages = vec![PageSpec::new(
            "account",
            "Account",
            vec![FieldSpec::text("name", "Name")],
        )];
        if values.get("name") == "custom" {
            pages.push(PageSpec::new(
                "servers",
                "Servers",
                vec![FieldSpec::text("host", "IMAP host")],
            ));
        }
        pages
    });
    let mut spec = FormSpec::paged(
        "Account",
        "Create",
        pages,
        Box::new(|world, values| {
            world.resource_mut::<Submitted>().0 = Some(values);
        }),
    );
    spec.mode = mode;
    spec
}

fn step_entities(app: &mut App) -> Vec<(usize, bool)> {
    let mut steps: Vec<(usize, bool)> = app
        .world_mut()
        .query::<(&FormStepControl, Has<plurimus::UiDisabled>)>()
        .iter(app.world())
        .map(|(control, disabled)| (control.0, disabled))
        .collect();
    steps.sort_by_key(|(index, _)| *index);
    steps
}

#[test]
fn a_single_page_form_spawns_no_step_controls() {
    let mut app = form_app();
    open_form(app.world_mut(), two_field_spec());
    app.update();
    assert!(step_entities(&mut app).is_empty(), "no strip for one page");
}

#[test]
fn a_derived_page_appears_as_a_step_control_and_is_disabled_until_reached() {
    let mut app = form_app();
    open_form(app.world_mut(), paged_spec(FormMode::Create));
    app.update();
    assert!(step_entities(&mut app).is_empty(), "one page so far");

    type_str(&mut app, "custom");
    app.update();
    assert_eq!(
        step_entities(&mut app),
        vec![(0, false), (1, true)],
        "the new step exists but creation has not reached it"
    );
    assert_eq!(
        app.world().resource::<ActiveForm>().step_titles(),
        vec!["Account".to_owned(), "Servers".to_owned()]
    );
}

#[test]
fn editing_leaves_every_step_enabled_from_the_start() {
    let mut app = form_app();
    open_form(app.world_mut(), paged_spec(FormMode::Edit));
    type_str(&mut app, "custom");
    app.update();
    assert_eq!(
        step_entities(&mut app),
        vec![(0, false), (1, false)],
        "editing reaches any step at once"
    );
}

#[test]
fn enter_advances_during_creation_and_only_submits_on_the_last_page() {
    let mut app = form_app();
    open_form(app.world_mut(), paged_spec(FormMode::Create));
    type_str(&mut app, "custom");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<Submitted>().0.is_none(),
        "Enter is Next while pages remain"
    );
    assert_eq!(app.world().resource::<ActiveForm>().page(), Some(1));

    type_str(&mut app, "mail.example.net");
    press(&mut app, KeyCode::Enter);
    let values = app.world().resource::<Submitted>().0.clone().unwrap();
    assert_eq!(values.get("name"), "custom");
    assert_eq!(values.get("host"), "mail.example.net");
}

#[test]
fn the_back_button_returns_to_the_previous_page() {
    let mut app = form_app();
    open_form(app.world_mut(), paged_spec(FormMode::Create));
    type_str(&mut app, "custom");
    press(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(app.world().resource::<ActiveForm>().page(), Some(1));

    // One field, then Cancel, then Back.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<ActiveForm>().page(), Some(0));
    assert_eq!(
        app.world().resource::<ActiveForm>().value("host").unwrap(),
        "",
        "going back keeps what the far page held"
    );
}

#[test]
fn clicking_a_reached_step_jumps_to_it() {
    let mut app = form_app();
    open_form(app.world_mut(), paged_spec(FormMode::Edit));
    type_str(&mut app, "custom");
    app.update();
    apply_layouts(&mut app);

    let second = control_entity(&mut app, |control: &FormStepControl| control.0 == 1);
    let rect = app.world().get::<WidgetRect>(second).unwrap().0;
    let event = UiEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    mouse::handle_step(app.world_mut(), second, event).unwrap();
    assert_eq!(app.world().resource::<ActiveForm>().page(), Some(1));
}

#[test]
fn page_down_and_page_up_walk_the_steps() {
    let mut app = form_app();
    open_form(app.world_mut(), paged_spec(FormMode::Edit));
    type_str(&mut app, "custom");
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.world().resource::<ActiveForm>().page(), Some(1));
    press(&mut app, KeyCode::PageUp);
    assert_eq!(app.world().resource::<ActiveForm>().page(), Some(0));
}

fn completing_spec() -> FormSpec {
    spec(vec![
        FieldSpec::text("to", "To").completed(|segment| {
            ["ada@example.com", "adam@example.com", "bob@example.com"]
                .into_iter()
                .filter(|candidate| candidate.starts_with(segment))
                .map(str::to_owned)
                .collect()
        }),
        FieldSpec::text("subject", "Subject"),
    ])
}

fn press_ctrl(app: &mut App, code: KeyCode) {
    let event = KeyEvent::new(code, KeyModifiers::CONTROL);
    handle_key(app.world_mut(), event).unwrap();
}

/// Address completion was the one thing the bottom-row prompt could do
/// that forms could not; the compose headers depend on it.
#[test]
fn a_completed_field_cycles_candidates_without_stealing_tab() {
    let mut app = form_app();
    open_form(app.world_mut(), completing_spec());
    type_str(&mut app, "ad");

    press_ctrl(&mut app, KeyCode::Char('n'));
    assert_eq!(
        app.world().resource::<ActiveForm>().value("to").unwrap(),
        "ada@example.com"
    );
    press_ctrl(&mut app, KeyCode::Char('n'));
    assert_eq!(
        app.world().resource::<ActiveForm>().value("to").unwrap(),
        "adam@example.com"
    );
    press_ctrl(&mut app, KeyCode::Char('p'));
    assert_eq!(
        app.world().resource::<ActiveForm>().value("to").unwrap(),
        "ada@example.com"
    );
}

#[test]
fn tab_still_moves_focus_on_a_completed_field() {
    let mut app = form_app();
    open_form(app.world_mut(), completing_spec());
    type_str(&mut app, "ad");

    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "hi");

    assert_eq!(
        app.world().resource::<ActiveForm>().value("to").unwrap(),
        "ad",
        "Tab must leave the address alone"
    );
    assert_eq!(
        app.world()
            .resource::<ActiveForm>()
            .value("subject")
            .unwrap(),
        "hi"
    );
}

/// Completion works one address at a time, so a second recipient
/// completes against its own segment and keeps the first.
#[test]
fn completion_rewrites_only_the_address_being_typed() {
    let mut app = form_app();
    open_form(app.world_mut(), completing_spec());
    type_str(&mut app, "ada@example.com, bo");

    press_ctrl(&mut app, KeyCode::Char('n'));

    assert_eq!(
        app.world().resource::<ActiveForm>().value("to").unwrap(),
        "ada@example.com, bob@example.com"
    );
}

#[test]
fn a_field_without_candidates_ignores_the_completion_keys() {
    let mut app = form_app();
    open_form(app.world_mut(), completing_spec());
    type_str(&mut app, "zz");

    press_ctrl(&mut app, KeyCode::Char('n'));

    assert_eq!(
        app.world().resource::<ActiveForm>().value("to").unwrap(),
        "zz",
        "nothing matched, so nothing is inserted"
    );
}

fn body_spec() -> FormSpec {
    spec(vec![
        FieldSpec::text("subject", "Subject"),
        FieldSpec::body("body", "Body").with_initial("first\nsecond"),
    ])
}

#[test]
fn a_body_field_opens_holding_its_initial_lines() {
    let mut app = form_app();
    open_form(app.world_mut(), body_spec());

    assert_eq!(
        app.world().resource::<ActiveForm>().value("body").unwrap(),
        "first\nsecond"
    );
}

#[test]
fn typing_into_a_focused_body_edits_it_rather_than_the_headers() {
    let mut app = form_app();
    open_form(app.world_mut(), body_spec());
    press(&mut app, KeyCode::Tab);

    type_str(&mut app, "!");

    let form = app.world().resource::<ActiveForm>();
    assert_eq!(
        form.value("body").unwrap(),
        "!first\nsecond",
        "a body opens with the caret at the top, where a reply's quote begins"
    );
    assert_eq!(form.value("subject").unwrap(), "");
}

/// The whole point of the layering: Enter breaks the line instead of
/// firing the primary button, and Tab still leaves.
#[test]
fn enter_breaks_the_line_and_tab_still_leaves_the_body() {
    let mut app = form_app();
    open_form(app.world_mut(), body_spec());
    press(&mut app, KeyCode::Tab);

    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.world().resource::<ActiveForm>().value("body").unwrap(),
        "\nfirst\nsecond"
    );
    assert!(
        app.world().resource::<Submitted>().0.is_none(),
        "Enter in a body must never submit the form"
    );

    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "x");

    assert_eq!(
        app.world().resource::<ActiveForm>().value("body").unwrap(),
        "\nfirst\nsecond",
        "Tab left the body, so the keystroke went elsewhere"
    );
}

/// Down moves the caret inside a body, where on any other field it
/// would move focus.
#[test]
fn the_body_keeps_the_arrows_the_form_would_otherwise_take() {
    let mut app = form_app();
    open_form(app.world_mut(), body_spec());
    press(&mut app, KeyCode::Tab);

    press(&mut app, KeyCode::Down);
    type_str(&mut app, "!");

    assert_eq!(
        app.world().resource::<ActiveForm>().value("body").unwrap(),
        "first\n!second",
        "Down moved the caret, not the focus"
    );
}

#[test]
fn a_body_field_fills_the_frame_and_the_others_do_not() {
    let spec = body_spec();
    let pages = (spec.pages)(&FormValues::default());
    let fields = &pages[0].fields;

    assert_eq!(fields[0].height, FieldHeight::Row);
    assert_eq!(fields[1].height, FieldHeight::Fill);
}

/// Completion belongs under the field that asked for it, not at the
/// bottom of the screen — and a field near the bottom clamps back
/// inside rather than hanging off the end.
#[test]
fn the_completion_panel_sits_under_its_field_and_stays_on_screen() {
    let mut app = form_app();
    open_form(app.world_mut(), completing_spec());
    type_str(&mut app, "a");
    app.update();
    apply_layouts(&mut app);

    let panel = app
        .world_mut()
        .query_filtered::<&WidgetRect, With<panel::FormPanel>>()
        .iter(app.world())
        .next()
        .copied()
        .expect("a panel spawns while there are candidates");
    let entity = field_entity(&mut app, 0);
    let field = *app.world().get::<WidgetRect>(entity).unwrap();

    assert_eq!(
        panel.0.y,
        field.0.bottom(),
        "the panel hangs directly under the field"
    );
    assert!(panel.0.bottom() <= TERMINAL.height, "{:?}", panel.0);
    assert!(panel.0.right() <= TERMINAL.width, "{:?}", panel.0);
}
