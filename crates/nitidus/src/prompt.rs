//! The prompt line: a labeled single-line input on the statusline row
//! (tui-prompts editing), completing into an `on_submit` closure or
//! cancelling into `on_cancel`. The command line stays its own mode —
//! prompts are for questions ("To:", "Discard? (y/n)"), not commands.

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;
use plurimus::{Widget, WidgetLayout};
use tui_prompts::{FocusState, Prompt, State, Status, TextPrompt, TextState};

use crate::keymap::{InputMode, Mode};
use nitidus_ui_kit::layout;

pub type SubmitFn = Box<dyn FnOnce(&mut World, String) + Send + Sync>;
pub type CancelFn = Box<dyn FnOnce(&mut World) + Send + Sync>;

pub struct PromptRequest {
    pub label: String,
    pub initial: String,
    pub on_submit: SubmitFn,
    pub on_cancel: CancelFn,
}

impl PromptRequest {
    pub fn new(label: impl Into<String>, on_submit: SubmitFn) -> Self {
        Self {
            label: label.into(),
            initial: String::new(),
            on_submit,
            on_cancel: Box::new(|_| {}),
        }
    }

    pub fn with_initial(mut self, initial: impl Into<String>) -> Self {
        self.initial = initial.into();
        self
    }

    pub fn with_cancel(mut self, on_cancel: CancelFn) -> Self {
        self.on_cancel = on_cancel;
        self
    }
}

struct ActivePrompt {
    label: String,
    text: TextState<'static>,
    on_submit: SubmitFn,
    on_cancel: CancelFn,
}

#[derive(Resource, Default)]
pub struct PromptState(Option<ActivePrompt>);

impl PromptState {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }

    pub fn label(&self) -> Option<&str> {
        self.0.as_ref().map(|active| active.label.as_str())
    }

    pub fn value(&self) -> Option<&str> {
        self.0.as_ref().map(|active| active.text.value())
    }
}

pub struct PromptPlugin;

impl Plugin for PromptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PromptState>();
        app.add_systems(Startup, spawn_prompt);
        app.add_systems(Update, (sync_prompt_visibility, refresh_prompt).chain());
    }
}

pub fn open_prompt(world: &mut World, request: PromptRequest) {
    let mut text = TextState::new()
        .with_value(request.initial)
        .with_focus(FocusState::Focused);
    text.move_end();
    world.resource_mut::<PromptState>().0 = Some(ActivePrompt {
        label: request.label,
        text,
        on_submit: request.on_submit,
        on_cancel: request.on_cancel,
    });
    world.resource_mut::<Mode>().0 = InputMode::Prompt;
}

/// Called by the router while a prompt owns input. Enter completes,
/// Esc aborts; everything else edits through tui-prompts.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let status = {
        let mut prompt = world.resource_mut::<PromptState>();
        let Some(active) = prompt.0.as_mut() else {
            world.resource_mut::<Mode>().0 = InputMode::Normal;
            return Ok(());
        };
        active.text.handle_key_event(key);
        active.text.status()
    };
    if status == Status::Pending {
        return Ok(());
    }
    let Some(active) = world.resource_mut::<PromptState>().0.take() else {
        return Ok(());
    };
    world.resource_mut::<Mode>().0 = InputMode::Normal;
    match status {
        Status::Done => (active.on_submit)(world, active.text.value().to_owned()),
        Status::Aborted => (active.on_cancel)(world),
        Status::Pending => {}
    }
    Ok(())
}

#[derive(Component)]
struct PromptLine;

#[derive(Clone, Default)]
struct PromptRender {
    label: String,
    text: TextState<'static>,
}

fn spawn_prompt(mut commands: Commands) {
    let mut widget = Widget::from_render_fn_with_state(render_prompt, PromptRender::default());
    widget.set_enabled(false);
    commands.spawn((
        PromptLine,
        widget,
        WidgetLayout::from(layout::statusline_layout()),
    ));
}

fn sync_prompt_visibility(mode: Res<Mode>, mut widgets: Query<&mut Widget, With<PromptLine>>) {
    if !mode.is_changed() {
        return;
    }
    for mut widget in &mut widgets {
        widget.set_enabled(mode.0 == InputMode::Prompt);
    }
}

fn refresh_prompt(
    mode: Res<Mode>,
    prompt: Res<PromptState>,
    mut widgets: Query<&mut Widget, With<PromptLine>>,
) -> Result {
    if mode.0 != InputMode::Prompt || !prompt.is_changed() {
        return Ok(());
    }
    let Some(active) = prompt.0.as_ref() else {
        return Ok(());
    };
    for mut widget in &mut widgets {
        widget.set_state(PromptRender {
            label: active.label.clone(),
            text: active.text.clone(),
        })?;
    }
    Ok(())
}

fn render_prompt(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut PromptRender,
) -> Result {
    frame.render_widget(ratatui::widgets::Clear, area);
    let prompt = TextPrompt::from(state.label.clone());
    let mut text = state.text.clone();
    prompt.draw(frame, area, &mut text);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::KeyCode;

    use super::*;

    #[derive(Resource, Default)]
    struct Submitted(Option<String>);

    #[derive(Resource, Default)]
    struct Cancelled(bool);

    fn prompt_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Mode>();
        app.init_resource::<PromptState>();
        app.init_resource::<Submitted>();
        app.init_resource::<Cancelled>();
        app
    }

    fn open_test_prompt(app: &mut App, initial: &str) {
        let request = PromptRequest::new(
            "To: ",
            Box::new(|world, value| world.resource_mut::<Submitted>().0 = Some(value)),
        )
        .with_initial(initial)
        .with_cancel(Box::new(|world| world.resource_mut::<Cancelled>().0 = true));
        open_prompt(app.world_mut(), request);
    }

    fn press(app: &mut App, code: KeyCode) {
        handle_key(app.world_mut(), KeyEvent::from(code)).unwrap();
    }

    #[test]
    fn typing_appends_after_the_initial_value_and_enter_submits() {
        let mut app = prompt_app();
        open_test_prompt(&mut app, "bob@");
        assert_eq!(app.world().resource::<Mode>().0, InputMode::Prompt);
        for c in "x.com".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.world().resource::<Submitted>().0.as_deref(),
            Some("bob@x.com")
        );
        assert_eq!(app.world().resource::<Mode>().0, InputMode::Normal);
        assert!(!app.world().resource::<PromptState>().is_open());
    }

    #[test]
    fn escape_cancels_without_submitting() {
        let mut app = prompt_app();
        open_test_prompt(&mut app, "");
        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Esc);
        assert!(app.world().resource::<Cancelled>().0);
        assert_eq!(app.world().resource::<Submitted>().0, None);
        assert_eq!(app.world().resource::<Mode>().0, InputMode::Normal);
    }
}
