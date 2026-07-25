use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Modifier, Style};

use super::color::ThemeColor;
use super::palette::{Theme, ThemeIndexStyles, ThemePalette};
use super::states::{ThemeColorStates, ThemeColors};

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
    fn tailwind_seeds_resolve_to_rgb() {
        let bg = tailwind_dark().base.default.normal.bg;
        assert_ne!(
            bg,
            ThemeColor::new(0, 0, 0),
            "tailwind seed should not fall back to black"
        );
    }
}
