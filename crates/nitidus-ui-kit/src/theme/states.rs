use ratatui::style::Style;

use super::color::ThemeColor;

const DISABLED_FG_DARKEN: f32 = 0.4;
const FOCUSED_BG_LIGHTEN: f32 = 0.125;
const HOVERED_BG_LIGHTEN: f32 = 0.25;
const SELECTED_BG_LIGHTEN: f32 = 0.375;
const SELECTED_FG_LIGHTEN: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeColors {
    pub bg: ThemeColor,
    pub fg: ThemeColor,
}

impl ThemeColors {
    pub const fn new(bg: ThemeColor, fg: ThemeColor) -> Self {
        Self { bg, fg }
    }

    pub fn style(&self) -> Style {
        Style::new().bg(self.bg.into()).fg(self.fg.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeColorStates {
    pub normal: ThemeColors,
    pub disabled: ThemeColors,
    pub focused: ThemeColors,
    pub hovered: ThemeColors,
    pub selected: ThemeColors,
}

impl ThemeColorStates {
    pub fn derive(seed: ThemeColors) -> Self {
        Self {
            normal: seed,
            disabled: ThemeColors::new(seed.bg, seed.fg.darken(DISABLED_FG_DARKEN)),
            focused: ThemeColors::new(seed.bg.lighten(FOCUSED_BG_LIGHTEN), seed.fg),
            hovered: ThemeColors::new(seed.bg.lighten(HOVERED_BG_LIGHTEN), seed.fg),
            selected: ThemeColors::new(
                seed.bg.lighten(SELECTED_BG_LIGHTEN),
                seed.fg.lighten(SELECTED_FG_LIGHTEN),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn seed() -> ThemeColors {
        ThemeColors::new(ThemeColor::new(30, 41, 59), ThemeColor::new(203, 213, 225))
    }

    #[test]
    fn derive_keeps_seed_as_normal() {
        assert_eq!(ThemeColorStates::derive(seed()).normal, seed());
    }

    #[test]
    fn derived_states_are_distinct() {
        let states = ThemeColorStates::derive(seed());
        let backgrounds = [
            states.normal.bg,
            states.focused.bg,
            states.hovered.bg,
            states.selected.bg,
        ];
        for (i, a) in backgrounds.iter().enumerate() {
            for b in backgrounds.iter().skip(i + 1) {
                assert_ne!(a, b, "interaction states must be visually distinct");
            }
        }
        assert_ne!(states.disabled.fg, states.normal.fg);
    }

    #[test]
    fn interaction_backgrounds_lighten_monotonically() {
        let states = ThemeColorStates::derive(seed());
        assert!(states.focused.bg.r > states.normal.bg.r);
        assert!(states.hovered.bg.r > states.focused.bg.r);
        assert!(states.selected.bg.r > states.hovered.bg.r);
    }

    #[test]
    fn style_carries_both_colors() {
        let style = seed().style();
        assert_eq!(style.bg, Some(seed().bg.into()));
        assert_eq!(style.fg, Some(seed().fg.into()));
    }
}
