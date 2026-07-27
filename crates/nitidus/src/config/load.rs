//! Loading and validation. Missing files mean defaults; malformed files
//! are startup errors — the TUI never half-starts on a broken config.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, bail};

use super::keymaps::RawKeymaps;
use super::schema::{Config, THEME_TAILWIND_DARK};
use crate::dirs;

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const KEYS_FILE_NAME: &str = "keys.toml";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadedConfig {
    pub config: Config,
    pub keymaps: RawKeymaps,
    /// Settings that still parse but no longer mean what they used to;
    /// surfaced on startup rather than failing the launch.
    pub notices: Vec<String>,
}

pub fn load() -> anyhow::Result<LoadedConfig> {
    let dir = dirs::config_dir()?;
    let config_path = dir.join(CONFIG_FILE_NAME);
    let config: Config = parse_file(&config_path)?.unwrap_or_default();
    let keymaps: RawKeymaps = parse_file(&dir.join(KEYS_FILE_NAME))?.unwrap_or_default();
    validate(&config)?;
    keymaps.validate()?;
    let notices = std::fs::read_to_string(&config_path)
        .ok()
        .map_or_else(Vec::new, |raw| retired_settings(&raw));
    Ok(LoadedConfig {
        config,
        keymaps,
        notices,
    })
}

/// The mark-read delay is gone. A config still carrying one loads with
/// `mark_read = "open"`, and says so rather than changing behaviour in
/// silence.
pub(crate) fn retired_settings(raw: &str) -> Vec<String> {
    let Ok(value) = toml::from_str::<toml::Value>(raw) else {
        return Vec::new();
    };
    let mark_read = value
        .get("ui")
        .and_then(|ui| ui.get("pager"))
        .and_then(|pager| pager.get("mark_read"));
    if mark_read.is_some_and(|value| value.is_integer() || value.is_float()) {
        return vec![
            "mark_read no longer takes a delay; reading is explicit now, so it was read as \"open\""
                .to_owned(),
        ];
    }
    Vec::new()
}

fn parse_file<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    if !path.exists() {
        tracing::info!("no {} found, using defaults", path.display());
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

pub(crate) fn validate(config: &Config) -> anyhow::Result<()> {
    if config.ui.theme != THEME_TAILWIND_DARK {
        bail!(
            "unknown theme {:?} (available: {THEME_TAILWIND_DARK:?})",
            config.ui.theme
        );
    }
    let mut columns = BTreeSet::new();
    for column in &config.ui.index.columns {
        if !columns.insert(column) {
            bail!("duplicate index column {column:?}");
        }
    }
    let mut names = BTreeSet::new();
    for account in &config.accounts {
        if account.name.is_empty() {
            bail!("account with empty name (every account needs a unique name)");
        }
        if !names.insert(&account.name) {
            bail!("duplicate account name {:?}", account.name);
        }
        validate_signature(account)?;
    }
    Ok(())
}

fn validate_signature(account: &super::account::AccountConfig) -> anyhow::Result<()> {
    if account.signature.is_some() && account.signature_file.is_some() {
        bail!(
            "account {:?} sets both signature and signature_file",
            account.name
        );
    }
    if let Some(path) = &account.signature_file
        && !path.exists()
    {
        bail!(
            "account {:?} signature_file does not exist: {}",
            account.name,
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    #[test]
    fn a_retired_mark_read_delay_is_noticed_rather_than_applied_in_silence() {
        let notices = retired_settings("[ui.pager]\nmark_read = 2\n");
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("mark_read"), "{notices:?}");
        assert!(notices[0].contains("open"), "{notices:?}");
    }

    #[test]
    fn a_current_mark_read_value_notices_nothing() {
        assert!(retired_settings("[ui.pager]\nmark_read = \"never\"\n").is_empty());
        assert!(retired_settings("").is_empty());
        assert!(retired_settings("not = = toml").is_empty());
    }

    use super::*;
    use crate::config::account::AccountConfig;

    fn config_with_accounts(names: &[&str]) -> Config {
        Config {
            accounts: names
                .iter()
                .map(|name| AccountConfig {
                    name: (*name).to_owned(),
                    ..AccountConfig::default()
                })
                .collect(),
            ..Config::default()
        }
    }

    #[test]
    fn default_config_validates() {
        assert!(validate(&Config::default()).is_ok());
        assert!(validate(&config_with_accounts(&["a", "b"])).is_ok());
    }

    #[test]
    fn duplicate_account_names_are_rejected() {
        let message = validate(&config_with_accounts(&["a", "a"]))
            .unwrap_err()
            .to_string();
        assert!(message.contains("duplicate"), "{message}");
    }

    #[test]
    fn empty_account_name_is_rejected() {
        assert!(validate(&config_with_accounts(&[""])).is_err());
    }

    #[test]
    fn duplicate_index_column_is_rejected() {
        use crate::config::schema::IndexColumn;
        let mut config = Config::default();
        config.ui.index.columns = vec![IndexColumn::Date, IndexColumn::Date];
        let message = validate(&config).unwrap_err().to_string();
        assert!(message.contains("duplicate index column"), "{message}");
    }

    #[test]
    fn empty_index_columns_are_allowed() {
        let mut config = Config::default();
        config.ui.index.columns.clear();
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn unknown_theme_is_rejected() {
        let mut config = Config::default();
        config.ui.theme = "neon".to_owned();
        let message = validate(&config).unwrap_err().to_string();
        assert!(message.contains("neon"), "{message}");
    }

    #[test]
    fn both_signature_forms_are_rejected() {
        let mut config = config_with_accounts(&["a"]);
        config.accounts[0].signature = Some("sig".to_owned());
        config.accounts[0].signature_file = Some("/nonexistent".into());
        assert!(validate(&config).is_err());
    }

    #[test]
    fn dangling_signature_file_is_rejected() {
        let mut config = config_with_accounts(&["a"]);
        config.accounts[0].signature_file = Some("/definitely/not/here.txt".into());
        let message = validate(&config).unwrap_err().to_string();
        assert!(message.contains("signature_file"), "{message}");
    }

    #[test]
    fn load_from_temp_dir_covers_missing_present_and_malformed() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: set_var mutates process-global state; all env-dependent
        // cases run inside this single test so no parallel test observes
        // the variable mid-change.
        unsafe { std::env::set_var(dirs::CONFIG_DIR_ENV, dir.path()) };

        let missing = load().unwrap();
        assert_eq!(missing, LoadedConfig::default());

        std::fs::write(
            dir.path().join(CONFIG_FILE_NAME),
            "[[accounts]]\nname = \"work\"\nemail = \"a@b.c\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(KEYS_FILE_NAME),
            "[index]\n\"gg\" = \":top\"\n",
        )
        .unwrap();
        let present = load().unwrap();
        assert_eq!(present.config.accounts[0].name, "work");
        assert_eq!(present.keymaps.0["index"]["gg"], ":top");

        std::fs::write(dir.path().join(CONFIG_FILE_NAME), "accounts = 5\n").unwrap();
        let message = format!("{:#}", load().unwrap_err());
        assert!(message.contains(CONFIG_FILE_NAME), "{message}");

        // SAFETY: same single-test discipline as set_var above.
        unsafe { std::env::remove_var(dirs::CONFIG_DIR_ENV) };
    }
}
