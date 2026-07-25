//! Command history persistence in the state directory.

use std::fs;

use bevy::prelude::*;

use super::CommandLineState;
use crate::dirs;

const HISTORY_DIR_NAME: &str = "history";
const HISTORY_FILE_NAME: &str = "commands";

fn history_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(dirs::state_dir()?
        .join(HISTORY_DIR_NAME)
        .join(HISTORY_FILE_NAME))
}

pub(super) fn load_history(mut state: ResMut<CommandLineState>) {
    let Ok(path) = history_path() else { return };
    if let Ok(content) = fs::read_to_string(path) {
        state.history = content.lines().map(str::to_owned).collect();
    }
}

pub(super) fn append_history(command: &str) {
    let result = history_path().and_then(|path| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{command}")?;
        Ok(())
    });
    if let Err(error) = result {
        tracing::warn!("failed to append command history: {error:#}");
    }
}
