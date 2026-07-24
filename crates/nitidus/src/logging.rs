//! File logging to the XDG state directory. The terminal belongs to the
//! UI, so nothing is ever written to stdout or stderr.

use std::fs;

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

use crate::dirs;

const LOG_FILE_NAME: &str = "nitidus.log";
const DEFAULT_LOG_FILTER: &str = "info";

pub fn init() -> anyhow::Result<WorkerGuard> {
    let dir = dirs::state_dir()?;
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
