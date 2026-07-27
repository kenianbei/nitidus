//! The three-line layout: sender, subject and date each own a line,
//! behind a gutter carrying the one state that styling cannot show.

use nitidus_mail::Flags;
use ratatui::style::Style;
use ratatui::text::Line;

use super::text::{fit, styled_line};
use super::{IndexRow, RowContext};

/// One glyph and the space that separates it from the text.
const GUTTER_WIDTH: usize = 2;
const BLANK_GUTTER: &str = "  ";
const MARKED_GLYPH: char = '*';
const DELETED_GLYPH: char = 'D';
const DRAFT_GLYPH: char = 'd';
const ANSWERED_GLYPH: char = 'R';

pub(super) fn card_lines(
    row: &IndexRow,
    width: u16,
    context: &RowContext,
    query: Option<&str>,
) -> Vec<Line<'static>> {
    let styles = &context.styles;
    let width = usize::from(width);
    let content = width.saturating_sub(GUTTER_WIDTH);
    let gutter = match gutter_glyph(row) {
        Some(glyph) => format!("{glyph} "),
        None => BLANK_GUTTER.to_owned(),
    };
    [
        (gutter.as_str(), sender(row), styles.sender),
        (BLANK_GUTTER, row.subject.as_str(), Style::default()),
        (BLANK_GUTTER, row.date.as_str(), styles.date),
    ]
    .into_iter()
    .map(|(gutter, text, emphasis)| {
        let fitted = fit(&format!("{gutter}{}", fit(text, content)), width);
        let style = super::row_style(row, styles, emphasis);
        styled_line(fitted, style, styles.highlight, query)
    })
    .collect()
}

/// An address beats the blank line a nameless sender would otherwise
/// leave.
fn sender(row: &IndexRow) -> &str {
    if row.from.is_empty() {
        return &row.from_addr;
    }
    &row.from
}

/// Unseen and flagged are already visible as styling, so the gutter is
/// left for the states that are not: the batch mark first, then the
/// most consequential flag.
fn gutter_glyph(row: &IndexRow) -> Option<char> {
    if row.marked {
        return Some(MARKED_GLYPH);
    }
    if row.flags.contains(Flags::DELETED) {
        return Some(DELETED_GLYPH);
    }
    if row.flags.contains(Flags::DRAFT) {
        return Some(DRAFT_GLYPH);
    }
    if row.flags.contains(Flags::ANSWERED) {
        return Some(ANSWERED_GLYPH);
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use ratatui::style::{Modifier, Style};

    use super::*;
    use crate::config::IndexLayout;
    use crate::index::render::RowStyles;

    const DEFAULT_WIDTH: u16 = 36;
    /// The narrowest list the full date still fits: gutter plus 22.
    const TIGHT_WIDTH: u16 = 24;

    fn card_row() -> IndexRow {
        IndexRow {
            flags: Flags::default().with(Flags::SEEN),
            date: "Mon, 22 Jul 2026 15:04".to_owned(),
            from: "Alice Example".to_owned(),
            from_addr: "alice@example.com".to_owned(),
            subject: "Quarterly report".to_owned(),
            ..IndexRow::default()
        }
    }

    fn lines_of(row: &IndexRow, width: u16) -> Vec<String> {
        card_lines(row, width, &RowContext::default(), None)
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn a_card_is_three_lines_of_sender_subject_and_date() {
        let lines = lines_of(&card_row(), 40);
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines.len(),
            usize::from(IndexLayout::Cards.row_height()),
            "the renderer and the row-height arithmetic must agree"
        );
        assert_eq!(lines[0].trim_end(), "  Alice Example");
        assert_eq!(lines[1].trim_end(), "  Quarterly report");
        assert_eq!(lines[2].trim_end(), "  Mon, 22 Jul 2026 15:04");
    }

    #[test]
    fn every_line_fills_exactly_the_pane_width() {
        for width in [0, 1, 2, 3, 10, DEFAULT_WIDTH, 80] {
            for line in lines_of(&card_row(), width) {
                assert_eq!(
                    line.chars().count(),
                    usize::from(width),
                    "width {width} produced {line:?}"
                );
            }
        }
    }

    #[test]
    fn the_full_date_survives_even_the_tightest_card_intact() {
        let row = IndexRow {
            from: "Alexandra Bartholomew-Winterbourne III".to_owned(),
            ..card_row()
        };
        for width in [TIGHT_WIDTH, DEFAULT_WIDTH] {
            let lines = lines_of(&row, width);
            assert_eq!(
                lines[2].trim_end(),
                "  Mon, 22 Jul 2026 15:04",
                "the date never truncates at width {width}"
            );
        }
        assert_eq!(
            lines_of(&row, TIGHT_WIDTH)[2],
            "  Mon, 22 Jul 2026 15:04",
            "at the tightest width the date fills the content area exactly"
        );
        assert!(
            lines_of(&row, DEFAULT_WIDTH)[0].ends_with('…'),
            "a sender wider than the content area still truncates"
        );
    }

    #[test]
    fn a_nameless_sender_falls_back_to_the_address() {
        let row = IndexRow {
            from: String::new(),
            ..card_row()
        };
        assert_eq!(lines_of(&row, 40)[0].trim_end(), "  alice@example.com");
    }

    #[test]
    fn the_gutter_shows_one_state_by_precedence() {
        let glyph_for = |row: &IndexRow| lines_of(row, 40)[0].chars().next().unwrap();
        let answered = IndexRow {
            flags: Flags::default().with(Flags::SEEN).with(Flags::ANSWERED),
            ..card_row()
        };
        assert_eq!(glyph_for(&answered), 'R');

        let draft = IndexRow {
            flags: answered.flags.with(Flags::DRAFT),
            ..card_row()
        };
        assert_eq!(glyph_for(&draft), 'd', "draft outranks answered");

        let deleted = IndexRow {
            flags: draft.flags.with(Flags::DELETED),
            ..card_row()
        };
        assert_eq!(glyph_for(&deleted), 'D', "deleted outranks draft");

        let marked = IndexRow {
            marked: true,
            ..deleted
        };
        assert_eq!(glyph_for(&marked), '*', "the batch mark outranks all");
    }

    #[test]
    fn unseen_and_flagged_never_claim_the_gutter() {
        let row = IndexRow {
            flags: Flags::default().with(Flags::FLAGGED),
            ..card_row()
        };
        assert!(gutter_glyph(&row).is_none(), "styling carries those two");
        assert!(lines_of(&row, 40)[0].starts_with("  Alice"));
    }

    fn themed_lines(row: &IndexRow) -> Vec<Line<'static>> {
        let context = RowContext {
            styles: RowStyles::from_theme(&nitidus_ui_kit::theme::tailwind_dark()),
            ..RowContext::default()
        };
        card_lines(row, 40, &context, None)
    }

    fn brightness(line: &Line<'static>) -> u16 {
        let Some(ratatui::style::Color::Rgb(r, g, b)) = line.spans[0].style.fg else {
            panic!("a themed card line should carry an rgb foreground");
        };
        u16::from(r) + u16::from(g) + u16::from(b)
    }

    #[test]
    fn the_sender_leads_the_card_and_the_date_recedes() {
        let lines = themed_lines(&card_row());

        assert!(
            brightness(&lines[0]) > brightness(&lines[1]),
            "the sender is brighter than the subject"
        );
        assert!(
            brightness(&lines[2]) < brightness(&lines[1]),
            "the date is dimmer than the subject"
        );
    }

    #[test]
    fn a_flag_tint_outranks_the_line_hierarchy() {
        let flagged = IndexRow {
            flags: Flags::default().with(Flags::SEEN).with(Flags::FLAGGED),
            ..card_row()
        };
        let lines = themed_lines(&flagged);
        let tint = nitidus_ui_kit::theme::tailwind_dark().index.flagged.fg;

        for (index, line) in lines.iter().enumerate() {
            assert_eq!(
                line.spans[0].style.fg, tint,
                "line {index} keeps the flagged tint over its own emphasis"
            );
        }
    }

    #[test]
    fn a_query_match_is_highlighted_on_whichever_line_holds_it() {
        let context = RowContext {
            styles: RowStyles {
                highlight: Style::default().add_modifier(Modifier::BOLD),
                ..RowStyles::default()
            },
            ..RowContext::default()
        };
        let lines = card_lines(&card_row(), 40, &context, Some("quarterly"));
        assert_eq!(lines[0].spans.len(), 1, "no match on the sender line");
        assert_eq!(lines[1].spans.len(), 3, "the subject line splits");
        assert_eq!(lines[1].spans[1].content.as_ref(), "Quarterly");
        assert!(
            lines[1].spans[1]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }
}
