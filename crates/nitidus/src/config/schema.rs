//! Top-level configuration schema. Strict: unknown fields are errors;
//! every field has a compiled-in default so user files overlay
//! field-by-field.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use super::account::AccountConfig;

pub const THEME_TAILWIND_DARK: &str = "tailwind-dark";

#[derive(Clone, Debug, Default, PartialEq, Resource, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub accounts: Vec<AccountConfig>,
    pub ui: UiConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub theme: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: THEME_TAILWIND_DARK.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn empty_file_is_a_complete_default_config() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.ui.theme, THEME_TAILWIND_DARK);
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn sections_overlay_field_by_field() {
        let config: Config = toml::from_str("[ui]\n").unwrap();
        assert_eq!(config.ui.theme, THEME_TAILWIND_DARK);
    }

    #[test]
    fn unknown_top_level_key_is_rejected() {
        let message = toml::from_str::<Config>("acounts = []\n")
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("acounts"),
            "error should name the typo: {message}"
        );
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let serialized = toml::to_string(&Config::default()).unwrap();
        let reparsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed, Config::default());
    }

    #[test]
    fn documented_example_config_stays_parseable() {
        let example = include_str!("../../../../documentation/example-config.toml");
        let config: Config = toml::from_str(example).expect("example config must parse");
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].name, "personal");
        assert_eq!(config.ui.theme, THEME_TAILWIND_DARK);
    }
}
