//! The prompt line: a labeled single-line input on the statusline row
//! (tui-prompts editing), completing into an `on_submit` closure or
//! cancelling into `on_cancel`. The command line stays its own mode —
//! prompts are for questions ("To:", "Discard? (y/n)"), not commands.
//! A prompt opened `with_completions` gets live candidates in a panel
//! and Tab cycling over the active comma-segment.

mod panel;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;
use plurimus::{Widget, WidgetLayout};
use tui_prompts::{FocusState, Prompt, State, Status, TextPrompt, TextRenderStyle, TextState};

use crate::keymap::{InputMode, Mode};
use nitidus_ui_kit::layout;

pub type SubmitFn = Box<dyn FnOnce(&mut World, String) + Send + Sync>;
pub type CancelFn = Box<dyn FnOnce(&mut World) + Send + Sync>;
pub type CompleteFn = std::sync::Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;

pub struct PromptRequest {
    pub label: String,
    pub initial: String,
    pub is_masked: bool,
    pub on_submit: SubmitFn,
    pub on_cancel: CancelFn,
    pub completions: Option<CompleteFn>,
}

impl PromptRequest {
    pub fn new(label: impl Into<String>, on_submit: SubmitFn) -> Self {
        Self {
            label: label.into(),
            initial: String::new(),
            is_masked: false,
            on_submit,
            on_cancel: Box::new(|_| {}),
            completions: None,
        }
    }

    /// Renders the typed value as `*` per character (secrets).
    pub fn masked(mut self) -> Self {
        self.is_masked = true;
        self
    }

    pub fn with_initial(mut self, initial: impl Into<String>) -> Self {
        self.initial = initial.into();
        self
    }

    pub fn with_cancel(mut self, on_cancel: CancelFn) -> Self {
        self.on_cancel = on_cancel;
        self
    }

    /// Live candidates for the active comma-segment; Tab cycles them.
    pub fn with_completions(mut self, complete: CompleteFn) -> Self {
        self.completions = Some(complete);
        self
    }
}

struct ActivePrompt {
    label: String,
    is_masked: bool,
    text: TextState<'static>,
    on_submit: SubmitFn,
    on_cancel: CancelFn,
    completions: Option<CompleteFn>,
    candidates: Vec<String>,
    /// Index into `candidates` while Tab-cycling; typing resets it.
    cycle: Option<usize>,
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

    pub fn candidates(&self) -> &[String] {
        self.0.as_ref().map_or(&[], |active| &active.candidates)
    }

    pub fn cycle(&self) -> Option<usize> {
        self.0.as_ref().and_then(|active| active.cycle)
    }
}

pub struct PromptPlugin;

impl Plugin for PromptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PromptState>();
        app.add_systems(Startup, spawn_prompt);
        app.add_systems(
            Update,
            (sync_prompt_visibility, refresh_prompt, panel::refresh_panel).chain(),
        );
    }
}

pub fn open_prompt(world: &mut World, request: PromptRequest) {
    let mut text = TextState::new()
        .with_value(request.initial.clone())
        .with_focus(FocusState::Focused);
    text.move_end();
    let candidates = request
        .completions
        .as_ref()
        .map_or_else(Vec::new, |complete| {
            complete(active_segment(&request.initial))
        });
    world.resource_mut::<PromptState>().0 = Some(ActivePrompt {
        label: request.label,
        is_masked: request.is_masked,
        text,
        on_submit: request.on_submit,
        on_cancel: request.on_cancel,
        completions: request.completions,
        candidates,
        cycle: None,
    });
    world.resource_mut::<Mode>().0 = InputMode::Prompt;
}

/// The unit completion works on: everything after the last comma.
fn active_segment(buffer: &str) -> &str {
    buffer
        .rsplit_once(',')
        .map_or(buffer, |(_, segment)| segment)
        .trim_start()
}

fn replace_active_segment(buffer: &str, candidate: &str) -> String {
    match buffer.rsplit_once(',') {
        Some((prefix, _)) => format!("{prefix}, {candidate}"),
        None => candidate.to_owned(),
    }
}

/// Called by the router while a prompt owns input. Enter completes,
/// Esc aborts, Tab cycles completions when the prompt has them;
/// everything else edits through tui-prompts.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let status = {
        let mut prompt = world.resource_mut::<PromptState>();
        let Some(active) = prompt.0.as_mut() else {
            world.resource_mut::<Mode>().0 = InputMode::Normal;
            return Ok(());
        };
        if key.code == bevy_ratatui::crossterm::event::KeyCode::Tab && active.completions.is_some()
        {
            apply_cycle(active);
            return Ok(());
        }
        active.text.handle_key_event(key);
        if let Some(complete) = &active.completions
            && active.text.status() == Status::Pending
        {
            active.candidates = complete(active_segment(active.text.value()));
            active.cycle = None;
        }
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

/// Tab: freeze the candidate list and rotate through it, rewriting
/// only the active segment (the frozen list survives the rewrite —
/// recomputing against the inserted candidate would strand the cycle).
fn apply_cycle(active: &mut ActivePrompt) {
    if active.candidates.is_empty() {
        return;
    }
    let next = active
        .cycle
        .map_or(0, |current| (current + 1) % active.candidates.len());
    active.cycle = Some(next);
    let rewritten = replace_active_segment(active.text.value(), &active.candidates[next]);
    *active.text.value_mut() = rewritten;
    active.text.move_end();
}

#[derive(Component)]
struct PromptLine;

#[derive(Clone, Default)]
struct PromptRender {
    label: String,
    is_masked: bool,
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
            is_masked: active.is_masked,
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
    let style = if state.is_masked {
        TextRenderStyle::Password
    } else {
        TextRenderStyle::Default
    };
    let prompt = TextPrompt::from(state.label.clone()).with_render_style(style);
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
    fn masked_prompt_renders_asterisks_instead_of_the_value() {
        let backend = ratatui::backend::TestBackend::new(30, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut state = PromptRender {
            label: "Password".to_owned(),
            is_masked: true,
            text: TextState::new().with_value("hunter2"),
        };
        terminal
            .draw(|frame| render_prompt(frame, frame.area(), &mut state).unwrap())
            .unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("*******"), "{rendered}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
    }

    fn open_completing_prompt(app: &mut App, initial: &str) {
        let request = PromptRequest::new(
            "To: ",
            Box::new(|world, value| world.resource_mut::<Submitted>().0 = Some(value)),
        )
        .with_initial(initial)
        .with_completions(std::sync::Arc::new(|segment: &str| {
            ["ada@x.example", "adele@y.example"]
                .into_iter()
                .filter(|candidate| candidate.starts_with(segment))
                .map(str::to_owned)
                .collect()
        }));
        open_prompt(app.world_mut(), request);
    }

    #[test]
    fn tab_cycles_candidates_rewriting_only_the_last_segment() {
        let mut app = prompt_app();
        open_completing_prompt(&mut app, "kj@nasa.example, ad");
        assert_eq!(
            app.world().resource::<PromptState>().candidates(),
            ["ada@x.example", "adele@y.example"]
        );

        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.world().resource::<PromptState>().value(),
            Some("kj@nasa.example, ada@x.example")
        );
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.world().resource::<PromptState>().value(),
            Some("kj@nasa.example, adele@y.example"),
            "cycling continues over the frozen list, not the inserted text"
        );
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.world().resource::<PromptState>().value(),
            Some("kj@nasa.example, ada@x.example"),
            "the cycle wraps"
        );

        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.world().resource::<Submitted>().0.as_deref(),
            Some("kj@nasa.example, ada@x.example"),
            "Enter submits the field as it stands"
        );
    }

    #[test]
    fn typing_recomputes_candidates_and_resets_the_cycle() {
        let mut app = prompt_app();
        open_completing_prompt(&mut app, "");
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.world().resource::<PromptState>().candidates().len(), 2);
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(
            app.world().resource::<PromptState>().candidates(),
            ["adele@y.example"]
        );
        assert_eq!(app.world().resource::<PromptState>().cycle(), None);
    }

    #[test]
    fn prompts_without_completions_ignore_tab_and_show_no_candidates() {
        let mut app = prompt_app();
        open_test_prompt(&mut app, "abc");
        assert!(
            app.world()
                .resource::<PromptState>()
                .candidates()
                .is_empty()
        );
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<PromptState>().value(), Some("abc"));
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
