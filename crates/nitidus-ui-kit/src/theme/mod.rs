//! Seed-derived theme system: a few seed colors expand into per-state
//! styles for every UI surface.

mod color;
mod palette;
mod presets;
mod states;

pub use color::ThemeColor;
pub use palette::{Theme, ThemeIndexStyles, ThemePalette};
pub use presets::tailwind_dark;
pub use states::{ThemeColorStates, ThemeColors};
