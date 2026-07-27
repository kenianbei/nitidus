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
    pub index: IndexUiConfig,
    pub pager: PagerUiConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: THEME_TAILWIND_DARK.to_owned(),
            index: IndexUiConfig::default(),
            pager: PagerUiConfig::default(),
        }
    }
}

const DEFAULT_READING_MAX_WIDTH: u16 = 100;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PagerUiConfig {
    pub mark_read: MarkRead,
    /// Widest the reading overlay grows on a large screen; beyond this
    /// a line is more tiring to read than it is informative.
    pub max_width: u16,
    /// Open the next message after a destructive pager verb instead of
    /// closing back to the index.
    pub advance: bool,
}

impl Default for PagerUiConfig {
    fn default() -> Self {
        Self {
            mark_read: MarkRead::default(),
            max_width: DEFAULT_READING_MAX_WIDTH,
            advance: true,
        }
    }
}

/// When a message gains SEEN once its fetch lands in the reading pane
/// or the reading overlay: `"open"` or `"never"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkRead {
    #[default]
    Open,
    Never,
}

const MARK_READ_FORMS: &str = r#""open" or "never""#;

impl Serialize for MarkRead {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            MarkRead::Open => serializer.serialize_str("open"),
            MarkRead::Never => serializer.serialize_str("never"),
        }
    }
}

impl<'de> Deserialize<'de> for MarkRead {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(MarkReadVisitor)
    }
}

struct MarkReadVisitor;

impl serde::de::Visitor<'_> for MarkReadVisitor {
    type Value = MarkRead;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "{MARK_READ_FORMS}")
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<MarkRead, E> {
        match value {
            "open" => Ok(MarkRead::Open),
            "never" => Ok(MarkRead::Never),
            other => Err(E::custom(format!(
                "unknown mark_read {other:?} (expected {MARK_READ_FORMS})"
            ))),
        }
    }

    /// The mark-read delay this replaced took a number of seconds. An
    /// existing `config.toml` keeps working: the delay is gone, so the
    /// value coerces to `"open"` and `Config::notices` says so.
    fn visit_f64<E: serde::de::Error>(self, _value: f64) -> Result<MarkRead, E> {
        Ok(MarkRead::Open)
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<MarkRead, E> {
        self.visit_f64(value as f64)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndexUiConfig {
    pub columns: Vec<IndexColumn>,
    pub date: DateFormat,
}

impl Default for IndexUiConfig {
    fn default() -> Self {
        Self {
            columns: vec![
                IndexColumn::Flags,
                IndexColumn::Date,
                IndexColumn::From,
                IndexColumn::Subject,
            ],
            date: DateFormat::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexColumn {
    Flags,
    Date,
    From,
    Subject,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DateFormat {
    #[default]
    Auto,
    Time,
    Short,
    Iso,
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
        assert_eq!(config.ui.index, IndexUiConfig::default());

        let partial: Config = toml::from_str("[ui.index]\ndate = \"iso\"\n").unwrap();
        assert_eq!(partial.ui.index.date, DateFormat::Iso);
        assert_eq!(
            partial.ui.index.columns,
            IndexUiConfig::default().columns,
            "unset columns keep the default order"
        );
    }

    #[test]
    fn index_columns_parse_subset_in_order() {
        let config: Config =
            toml::from_str("[ui.index]\ncolumns = [\"date\", \"subject\"]\n").unwrap();
        assert_eq!(
            config.ui.index.columns,
            vec![IndexColumn::Date, IndexColumn::Subject]
        );
    }

    #[test]
    fn unknown_index_column_is_rejected_with_the_valid_set() {
        let message = toml::from_str::<Config>("[ui.index]\ncolumns = [\"size\"]\n")
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("size"),
            "error should name the value: {message}"
        );
        assert!(
            message.contains("subject"),
            "error should list options: {message}"
        );
    }

    #[test]
    fn unknown_date_format_is_rejected_with_the_valid_set() {
        let message = toml::from_str::<Config>("[ui.index]\ndate = \"strftime\"\n")
            .unwrap_err()
            .to_string();
        assert!(message.contains("strftime"), "{message}");
        assert!(message.contains("iso"), "{message}");
    }

    #[test]
    fn mark_read_parses_both_shapes() {
        let open: Config = toml::from_str("[ui.pager]\nmark_read = \"open\"\n").unwrap();
        assert_eq!(open.ui.pager.mark_read, MarkRead::Open);
        let never: Config = toml::from_str("[ui.pager]\nmark_read = \"never\"\n").unwrap();
        assert_eq!(never.ui.pager.mark_read, MarkRead::Never);
    }

    /// The mark-read delay is gone; a config still carrying one keeps
    /// working rather than refusing to load.
    #[test]
    fn a_leftover_mark_read_delay_coerces_to_open() {
        let fractional: Config = toml::from_str("[ui.pager]\nmark_read = 1.5\n").unwrap();
        assert_eq!(fractional.ui.pager.mark_read, MarkRead::Open);
        let whole: Config = toml::from_str("[ui.pager]\nmark_read = 2\n").unwrap();
        assert_eq!(whole.ui.pager.mark_read, MarkRead::Open);
    }

    #[test]
    fn an_unknown_mark_read_value_is_rejected_with_the_accepted_forms() {
        let unknown = toml::from_str::<Config>("[ui.pager]\nmark_read = \"fast\"\n")
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("fast"), "{unknown}");
        assert!(unknown.contains("never"), "{unknown}");
    }

    #[test]
    fn pager_defaults_mark_on_open_and_advance() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.ui.pager.mark_read, MarkRead::Open);
        assert_eq!(config.ui.pager.max_width, DEFAULT_READING_MAX_WIDTH);
        assert!(config.ui.pager.advance);
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
