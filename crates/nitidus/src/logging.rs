//! File logging to the XDG state directory. The terminal belongs to the
//! UI, so nothing is ever written to stdout or stderr.

use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use etcetera::BaseStrategy;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

const APP_DIR_NAME: &str = "nitidus";
const LOG_FILE_NAME: &str = "nitidus.log";
const DEFAULT_LOG_FILTER: &str = "info";

pub fn init() -> anyhow::Result<WorkerGuard> {
    let dir = resolve_state_dir()?;
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create state directory {}", dir.display()))?;
    let appender = tracing_appender::rolling::never(&dir, LOG_FILE_NAME);
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Ok(guard)
}

fn resolve_state_dir() -> anyhow::Result<PathBuf> {
    let strategy =
        etcetera::choose_base_strategy().context("failed to resolve XDG base directories")?;
    let base = strategy.state_dir().unwrap_or_else(|| strategy.data_dir());
    Ok(base.join(APP_DIR_NAME))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn state_dir_ends_with_app_name() {
        let dir = resolve_state_dir().unwrap();
        assert!(
            dir.ends_with(APP_DIR_NAME),
            "state dir did not end with {APP_DIR_NAME}: {}",
            dir.display()
        );
    }
}
