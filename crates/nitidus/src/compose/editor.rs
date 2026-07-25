//! `$EDITOR` suspend/resume: drop the terminal context (its `Drop`
//! restores the terminal), run the editor blocking the main thread —
//! mail sync continues on the engine's own runtime — then re-init the
//! context; the next frame repaints everything through ratatui's
//! fresh buffer.

use std::path::Path;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_ratatui::RatatuiContext;

use super::{ComposeStage, ComposeState};
use crate::screen::Screen;
use crate::status::StatusMessage;

const FALLBACK_EDITOR: &str = "vi";

/// Test override: when present, this command runs instead of
/// `$VISUAL`/`$EDITOR` (headless harnesses cannot mutate process env
/// without breaking test isolation).
#[derive(Resource)]
pub struct EditorCommand(pub String);

pub(super) fn edit_body(world: &mut World) {
    let Some(path) = world
        .resource::<ComposeState>()
        .session()
        .map(|session| session.body_path.clone())
    else {
        return;
    };
    if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
        session.stage = ComposeStage::Editing;
    }
    let outcome = run_editor(world, &path);
    {
        let mut compose = world.resource_mut::<ComposeState>();
        if let Some(session) = compose.0.as_mut() {
            session.reload_body();
            session.stage = ComposeStage::Review;
        }
    }
    *world.resource_mut::<Screen>() = Screen::Compose;
    if let Err(error) = outcome {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("editor: {error}"), now);
    }
}

/// The terminal context must be gone while the editor owns the tty; a
/// failed re-init is unrecoverable and exits the app. Headless
/// harnesses (no context resource) skip the terminal dance entirely.
fn run_editor(world: &mut World, path: &Path) -> Result<(), String> {
    let command = editor_command(world);
    let had_context = world.remove_resource::<RatatuiContext>().is_some();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{command} '{}'", path.display()))
        .status();
    if had_context {
        match RatatuiContext::init() {
            Ok(context) => world.insert_resource(context),
            Err(error) => {
                eprintln!("failed to restore the terminal after the editor: {error}");
                world.write_message(AppExit::error());
                return Err(format!("terminal re-init failed: {error}"));
            }
        }
    }
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{command} exited with {status}")),
        Err(error) => Err(format!("running {command}: {error}")),
    }
}

fn editor_command(world: &World) -> String {
    if let Some(command) = world.get_resource::<EditorCommand>() {
        return command.0.clone();
    }
    for variable in ["VISUAL", "EDITOR"] {
        if let Ok(value) = std::env::var(variable)
            && !value.trim().is_empty()
        {
            return value;
        }
    }
    FALLBACK_EDITOR.to_owned()
}
