//! XDG directory resolution shared by logging and config loading.

use std::path::PathBuf;

use anyhow::Context;
use etcetera::BaseStrategy;

pub const APP_DIR_NAME: &str = "nitidus";
pub const CONFIG_DIR_ENV: &str = "NITIDUS_CONFIG_DIR";

/// Per-machine state (logs, histories). Falls back to the platform data
/// dir where no state dir exists (macOS/Windows).
pub fn state_dir() -> anyhow::Result<PathBuf> {
    let strategy = base_strategy()?;
    let base = strategy.state_dir().unwrap_or_else(|| strategy.data_dir());
    Ok(base.join(APP_DIR_NAME))
}

/// User configuration. `NITIDUS_CONFIG_DIR` overrides the platform
/// default when set.
pub fn config_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }
    Ok(base_strategy()?.config_dir().join(APP_DIR_NAME))
}

fn base_strategy() -> anyhow::Result<impl BaseStrategy> {
    etcetera::choose_base_strategy().context("failed to resolve XDG base directories")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn state_dir_ends_with_app_name() {
        let dir = state_dir().unwrap();
        assert!(
            dir.ends_with(APP_DIR_NAME),
            "state dir did not end with {APP_DIR_NAME}: {}",
            dir.display()
        );
    }

    #[test]
    fn config_dir_ends_with_app_name_without_override() {
        if std::env::var_os(CONFIG_DIR_ENV).is_none() {
            let dir = config_dir().unwrap();
            assert!(dir.ends_with(APP_DIR_NAME));
        }
    }
}
