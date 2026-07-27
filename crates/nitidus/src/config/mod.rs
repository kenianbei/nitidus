//! User configuration: strict TOML schemas, XDG-resolved files,
//! compiled-in defaults.

pub mod account;
mod keymaps;
pub mod keyring;
mod load;
pub mod oauth;
pub mod presets;
mod schema;
pub mod secrets;
pub mod write;

pub use keymaps::{RawKeymaps, parse_key_sequence};
pub use load::{CONFIG_FILE_NAME, KEYS_FILE_NAME, LoadedConfig, load};
pub use schema::{
    Config, DateFormat, IndexColumn, IndexLayout, IndexUiConfig, MarkRead, PagerUiConfig,
    THEME_TAILWIND_DARK, UiConfig,
};
