//! The `:` command line: a single-line editor on the statusline row with
//! history and fuzzy completion. Opens via `OpenCommandLine`, executes
//! through the shared command parser.

mod history;
mod panel;

use history::append_history;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use plurimus::{Widget, WidgetLayout};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;

use crate::action::{apply_action, complete_command, parse_command};
use crate::keymap::{InputMode, Mode};
use crate::shell::Statusline;
use crate::status::StatusMessage;
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;

pub struct CommandLinePlugin;

impl Plugin for CommandLinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandLineState>();
        app.add_systems(Startup, (history::load_history, spawn_command_line));
        app.add_systems(
            Update,
            (
                sync_mode_visibility,
                refresh_command_line,
                panel::refresh_panel,
            )
                .chain(),
        );
    }
}

#[derive(Resource, Default)]
pub struct CommandLineState {
    pub buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    completions: Vec<String>,
    completion_index: Option<usize>,
}

#[derive(Component)]
struct CommandLine;

#[derive(Clone, Default)]
struct CommandLineRender {
    text: String,
    cursor_column: u16,
    style: Style,
}

fn spawn_command_line(mut commands: Commands) {
    let mut widget =
        Widget::from_render_fn_with_state(render_command_line, CommandLineRender::default());
    widget.set_enabled(false);
    commands.spawn((
        CommandLine,
        widget,
        WidgetLayout::from(layout::statusline_layout()),
    ));
}

fn render_command_line(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut CommandLineRender,
) -> Result {
    frame.render_widget(Paragraph::new(state.text.clone()).style(state.style), area);
    frame.set_cursor_position((area.x.saturating_add(state.cursor_column), area.y));
    Ok(())
}

fn sync_mode_visibility(
    mode: Res<Mode>,
    mut state: ResMut<CommandLineState>,
    mut widgets: ParamSet<(
        Query<&mut Widget, With<CommandLine>>,
        Query<&mut Widget, With<Statusline>>,
    )>,
) {
    if !mode.is_changed() {
        return;
    }
    let show = mode.0 == InputMode::CommandLine;
    if !show {
        state.reset_input();
    }
    set_enabled(&mut widgets.p0(), show);
    set_enabled(&mut widgets.p1(), mode.0 == InputMode::Normal);
}

fn set_enabled(
    widgets: &mut Query<&mut Widget, impl bevy::ecs::query::QueryFilter>,
    enabled: bool,
) {
    for mut widget in widgets {
        widget.set_enabled(enabled);
    }
}

fn refresh_command_line(
    mode: Res<Mode>,
    state: Res<CommandLineState>,
    theme: Res<Theme>,
    mut widgets: Query<&mut Widget, With<CommandLine>>,
) -> Result {
    if mode.0 != InputMode::CommandLine || (!state.is_changed() && !theme.is_changed()) {
        return Ok(());
    }
    for mut widget in &mut widgets {
        widget.set_state(CommandLineRender {
            text: format!(":{}", state.buffer),
            cursor_column: u16::try_from(state.cursor + 1).unwrap_or(u16::MAX),
            style: theme.paper.default.focused.style(),
        })?;
    }
    Ok(())
}

/// Called by the router while the command line owns input; the router is
/// the only key passthrough, so no event is ever double-delivered.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    match key.code {
        KeyCode::Esc => close(world),
        KeyCode::Enter => execute(world),
        KeyCode::Tab => world.resource_mut::<CommandLineState>().cycle_completion(),
        KeyCode::Up => world.resource_mut::<CommandLineState>().history_prev(),
        KeyCode::Down => world.resource_mut::<CommandLineState>().history_next(),
        _ => world.resource_mut::<CommandLineState>().edit(key),
    }
    Ok(())
}

fn close(world: &mut World) {
    world.resource_mut::<Mode>().0 = InputMode::Normal;
}

/// On success the action is emitted and the line closes; on error the
/// line also closes so the statusline can show the themed error.
fn execute(world: &mut World) {
    let buffer = std::mem::take(&mut world.resource_mut::<CommandLineState>().buffer);
    let now = world.resource::<Time>().elapsed_secs_f64();
    if !buffer.trim().is_empty() {
        world
            .resource_mut::<CommandLineState>()
            .push_history(buffer.clone());
        append_history(&buffer);
    }
    match parse_command(&buffer) {
        Ok(action) => {
            // Close before applying so an action that enters another
            // input mode (a prompt) is not clobbered back to Normal.
            close(world);
            apply_action(world, &action);
        }
        Err(error) => {
            world
                .resource_mut::<StatusMessage>()
                .error(format!("{error:#}"), now);
            close(world);
        }
    }
}

impl CommandLineState {
    /// Opens with `text` already typed (plus a trailing space when
    /// non-empty, so arguments continue straight on).
    pub fn prefill(&mut self, text: &str) {
        self.reset_input();
        if !text.is_empty() {
            self.buffer = format!("{text} ");
            self.cursor = self.buffer.chars().count();
        }
    }

    fn reset_input(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
        self.completions.clear();
        self.completion_index = None;
    }

    fn edit(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.buffer.insert(self.byte_cursor(), c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.buffer.remove(self.byte_cursor());
            }
            KeyCode::Left if self.cursor > 0 => self.cursor -= 1,
            KeyCode::Right if self.cursor < self.buffer.chars().count() => self.cursor += 1,
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.buffer.chars().count(),
            _ => return,
        }
        self.completions.clear();
        self.completion_index = None;
        self.history_index = None;
    }

    fn byte_cursor(&self) -> usize {
        self.buffer
            .char_indices()
            .nth(self.cursor)
            .map_or(self.buffer.len(), |(index, _)| index)
    }

    /// Candidates and highlight for the completion panel: the frozen
    /// mid-cycle list when Tab has been pressed, else a live match of
    /// the buffer.
    pub(crate) fn completion_view(&self) -> (Vec<String>, Option<usize>) {
        if self.buffer.contains(char::is_whitespace) {
            return (Vec::new(), None);
        }
        if self.completions.is_empty() {
            (complete_command(&self.buffer), None)
        } else {
            (self.completions.clone(), self.completion_index)
        }
    }

    fn cycle_completion(&mut self) {
        if self.buffer.contains(char::is_whitespace) {
            return;
        }
        if self.completions.is_empty() {
            self.completions = complete_command(&self.buffer);
        }
        let Some(next) = next_index(self.completion_index, self.completions.len()) else {
            return;
        };
        self.completion_index = Some(next);
        self.buffer = self.completions[next].clone();
        self.cursor = self.buffer.chars().count();
    }

    fn push_history(&mut self, command: String) {
        if self.history.last() != Some(&command) {
            self.history.push(command);
        }
    }

    fn history_prev(&mut self) {
        let next = match self.history_index {
            None if !self.history.is_empty() => Some(self.history.len() - 1),
            Some(index) if index > 0 => Some(index - 1),
            other => other,
        };
        self.apply_history(next);
    }

    fn history_next(&mut self) {
        match self.history_index {
            Some(index) if index + 1 < self.history.len() => self.apply_history(Some(index + 1)),
            Some(_) => {
                self.history_index = None;
                self.buffer.clear();
                self.cursor = 0;
            }
            None => {}
        }
    }

    fn apply_history(&mut self, index: Option<usize>) {
        self.history_index = index;
        if let Some(index) = index
            && let Some(entry) = self.history.get(index)
        {
            self.buffer = entry.clone();
            self.cursor = self.buffer.chars().count();
        }
    }
}

fn next_index(current: Option<usize>, len: usize) -> Option<usize> {
    match (current, len) {
        (_, 0) => None,
        (None, _) => Some(0),
        (Some(index), _) => Some((index + 1) % len),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn press_char(state: &mut CommandLineState, c: char) {
        state.edit(KeyEvent::from(KeyCode::Char(c)));
    }

    #[test]
    fn editing_inserts_moves_and_deletes() {
        let mut state = CommandLineState::default();
        press_char(&mut state, 'a');
        press_char(&mut state, 'c');
        state.edit(KeyEvent::from(KeyCode::Left));
        press_char(&mut state, 'b');
        assert_eq!(state.buffer, "abc");
        state.edit(KeyEvent::from(KeyCode::Backspace));
        assert_eq!(state.buffer, "ac");
    }

    #[test]
    fn completion_cycles_candidates() {
        let mut state = CommandLineState::default();
        press_char(&mut state, 't');
        state.cycle_completion();
        let first = state.buffer.clone();
        assert!(first.starts_with("tab-"), "{first}");
        state.cycle_completion();
        assert_ne!(state.buffer, first);
    }

    #[test]
    fn completion_view_tracks_live_frozen_and_argument_states() {
        let mut state = CommandLineState::default();
        let (all, selected) = state.completion_view();
        assert!(all.len() > 30, "empty buffer lists every command");
        assert_eq!(selected, None);

        press_char(&mut state, 't');
        let (live, _) = state.completion_view();
        assert!(live.iter().all(|name| name.contains('t')), "{live:?}");

        state.cycle_completion();
        let (frozen, selected) = state.completion_view();
        assert_eq!(selected, Some(0));
        assert_eq!(frozen[0], state.buffer, "highlight follows the cycle");

        state.edit(KeyEvent::from(KeyCode::Char(' ')));
        state.edit(KeyEvent::from(KeyCode::Char('x')));
        let (with_args, _) = state.completion_view();
        assert!(with_args.is_empty(), "arguments hide the panel");
    }

    #[test]
    fn history_navigates_and_restores_empty_line() {
        let mut state = CommandLineState::default();
        state.push_history("quit".to_owned());
        state.push_history("echo hi".to_owned());
        state.history_prev();
        assert_eq!(state.buffer, "echo hi");
        state.history_prev();
        assert_eq!(state.buffer, "quit");
        state.history_next();
        assert_eq!(state.buffer, "echo hi");
        state.history_next();
        assert_eq!(state.buffer, "");
    }

    #[test]
    fn duplicate_history_entries_collapse() {
        let mut state = CommandLineState::default();
        state.push_history("quit".to_owned());
        state.push_history("quit".to_owned());
        assert_eq!(state.history.len(), 1);
    }
}
