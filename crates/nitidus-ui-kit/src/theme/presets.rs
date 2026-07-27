use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Modifier, Style};

use super::color::ThemeColor;
use super::palette::{Theme, ThemeIndexStyles, ThemePalette};
use super::states::{ThemeColorStates, ThemeColors};

/// Well below the focused state's lift: banding must read as texture,
/// not as an interaction.
const STRIPE_BG_LIGHTEN: f32 = 0.05;
/// The seed foreground is already light, so a small lift is invisible:
/// the sender needs most of the remaining headroom to read as brighter.
/// The date takes the palette's existing dim rather than a third
/// brightness of its own.
const SENDER_FG_LIGHTEN: f32 = 0.6;

pub fn tailwind_dark() -> Theme {
    let base = dark_palette(tailwind::SLATE.c900, tailwind::SLATE.c300);
    Theme {
        base,
        paper: dark_palette(tailwind::SLATE.c800, tailwind::SLATE.c200),
        index: ThemeIndexStyles {
            unseen: Style::new().add_modifier(Modifier::BOLD),
            flagged: Style::new().fg(base.warning.normal.fg.into()),
            deleted: Style::new().add_modifier(Modifier::DIM),
            marked: base.info.normal.style(),
            reading: Style::new()
                .fg(base.success.normal.fg.into())
                .add_modifier(Modifier::BOLD),
            stripe: Style::new().bg(base.default.normal.bg.lighten(STRIPE_BG_LIGHTEN).into()),
            sender: Style::new().fg(base.default.normal.fg.lighten(SENDER_FG_LIGHTEN).into()),
            date: Style::new().fg(base.default.disabled.fg.into()),
        },
    }
}

fn dark_palette(bg: Color, fg: Color) -> ThemePalette {
    let bg = seed(bg);
    ThemePalette {
        default: derive(bg, fg),
        error: derive(bg, tailwind::RED.c400),
        info: derive(bg, tailwind::BLUE.c400),
        success: derive(bg, tailwind::GREEN.c400),
        warning: derive(bg, tailwind::AMBER.c400),
    }
}

fn derive(bg: ThemeColor, fg: Color) -> ThemeColorStates {
    ThemeColorStates::derive(ThemeColors::new(bg, seed(fg)))
}

fn seed(color: Color) -> ThemeColor {
    match color {
        Color::Rgb(r, g, b) => ThemeColor::new(r, g, b),
        _ => ThemeColor::new(0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn preset_surfaces_differ() {
        let theme = tailwind_dark();
        assert_ne!(theme.base.default.normal.bg, theme.paper.default.normal.bg);
    }

    #[test]
    fn preset_palettes_have_distinct_accents() {
        let palette = tailwind_dark().base;
        let accents = [
            palette.default.normal.fg,
            palette.error.normal.fg,
            palette.info.normal.fg,
            palette.success.normal.fg,
            palette.warning.normal.fg,
        ];
        for (i, a) in accents.iter().enumerate() {
            for b in accents.iter().skip(i + 1) {
                assert_ne!(a, b, "palette accents must be distinguishable");
            }
        }
    }

    #[test]
    fn index_roles_match_the_established_look() {
        let theme = tailwind_dark();
        assert_eq!(
            theme.index.unseen,
            Style::new().add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            theme.index.deleted,
            Style::new().add_modifier(Modifier::DIM)
        );
        assert_eq!(theme.index.marked, theme.base.info.normal.style());
        assert_eq!(
            theme.index.flagged.fg,
            Some(theme.base.warning.normal.fg.into()),
            "flagged rows carry the warning tint"
        );
        assert_eq!(theme.index.flagged.bg, None, "a tint patches fg only");
    }

    #[test]
    fn stripe_lifts_the_background_less_than_any_interaction_state() {
        let theme = tailwind_dark();
        let normal = theme.base.default.normal.bg;
        let Some(ratatui::style::Color::Rgb(r, _, _)) = theme.index.stripe.bg else {
            panic!("stripe should carry an rgb background");
        };
        assert!(r > normal.r, "banding must be visible against the pane");
        assert!(
            r < theme.base.default.focused.bg.r,
            "banding must stay quieter than the focused state"
        );
        assert_eq!(theme.index.stripe.fg, None, "banding sets background only");
    }

    #[test]
    fn card_line_emphasis_brackets_the_normal_foreground() {
        let theme = tailwind_dark();
        let normal = theme.base.default.normal.fg;
        let channel = |style: Style| match style.fg {
            Some(ratatui::style::Color::Rgb(r, _, _)) => r,
            other => panic!("expected an rgb foreground, got {other:?}"),
        };

        assert!(
            channel(theme.index.sender) > normal.r,
            "the sender leads the card"
        );
        assert!(
            channel(theme.index.date) < normal.r,
            "the date recedes behind it"
        );
        assert_eq!(
            channel(theme.index.date),
            theme.base.default.disabled.fg.r,
            "the date reuses the palette's dim rather than inventing one"
        );
    }

    #[test]
    fn tailwind_seeds_resolve_to_rgb() {
        let bg = tailwind_dark().base.default.normal.bg;
        assert_ne!(
            bg,
            ThemeColor::new(0, 0, 0),
            "tailwind seed should not fall back to black"
        );
    }
}
