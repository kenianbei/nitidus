//! The row model for the index window: what one message contributes to
//! the list, the styles a theme lends it, and the dispatch to whichever
//! layout draws it.

mod card;
mod date;
mod table;
mod text;

pub(super) use date::resolve as resolve_date;

use jiff::Zoned;
use nitidus_mail::{EnvelopeSummary, Flags};
use nitidus_ui_kit::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::config::{DateFormat, IndexColumn, IndexLayout, IndexUiConfig};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexRow {
    pub flags: Flags,
    pub date: String,
    pub from: String,
    /// The card's fallback when a sender gave no display name.
    pub from_addr: String,
    pub subject: String,
    pub selected: bool,
    pub marked: bool,
    pub hovered: bool,
    /// This row's message is the one in the reading pane.
    pub reading: bool,
    /// This row takes the banding half of the alternating pair.
    pub striped: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RowStyles {
    pub normal: Style,
    pub selected: Style,
    pub highlight: Style,
    pub marked: Style,
    pub unseen: Style,
    pub flagged: Style,
    pub deleted: Style,
    pub hovered: Style,
    pub reading: Style,
    pub stripe: Style,
    pub sender: Style,
    pub date: Style,
}

impl RowStyles {
    pub fn from_theme(theme: &Theme) -> Self {
        let states = &theme.base.default;
        Self {
            normal: states.normal.style(),
            selected: states.selected.style(),
            highlight: theme
                .base
                .warning
                .normal
                .style()
                .add_modifier(Modifier::BOLD),
            marked: theme.index.marked,
            unseen: theme.index.unseen,
            flagged: theme.index.flagged,
            deleted: theme.index.deleted,
            hovered: states.hovered.style(),
            reading: theme.index.reading,
            stripe: theme.index.stripe,
            sender: theme.index.sender,
            date: theme.index.date,
        }
    }
}

pub(super) struct RowBuildContext<'a> {
    pub now: &'a Zoned,
    pub date: DateFormat,
    pub selected: bool,
    pub marked: bool,
    pub reading: bool,
    pub striped: bool,
}

/// Layout, column order and styles resolved once per refresh, shared by
/// every row of the window.
#[derive(Clone, Debug)]
pub struct RowContext {
    pub styles: RowStyles,
    pub layout: IndexLayout,
    pub columns: Vec<IndexColumn>,
}

impl Default for RowContext {
    fn default() -> Self {
        let index = IndexUiConfig::default();
        Self {
            styles: RowStyles::default(),
            layout: index.layout,
            columns: index.columns,
        }
    }
}

pub(super) fn build_row(
    envelope: &EnvelopeSummary,
    entry: &super::thread_view::OrderEntry,
    context: &RowBuildContext<'_>,
) -> IndexRow {
    IndexRow {
        flags: envelope.flags,
        date: date::format_date(envelope.date_epoch_secs, context.now, context.date),
        from: envelope.from_display.clone(),
        from_addr: envelope.from_addr.clone(),
        subject: threaded_subject(&envelope.subject, entry.depth, entry.collapsed_children),
        selected: context.selected,
        marked: context.marked,
        hovered: false,
        reading: context.reading,
        striped: context.striped,
    }
}

/// `↳ ` marks replies (indented two spaces per extra level); collapsed
/// roots carry their hidden-descendant count.
fn threaded_subject(subject: &str, depth: u8, collapsed_children: u32) -> String {
    if collapsed_children > 0 {
        return format!("[+{collapsed_children}] {subject}");
    }
    if depth == 0 {
        return subject.to_owned();
    }
    let indent = "  ".repeat(usize::from(depth) - 1);
    format!("{indent}↳ {subject}")
}

/// Terminal lines are not rows once a row is taller than one line;
/// every viewport, page and click calculation converts here.
pub fn viewport_rows(height: u16, row_height: u16) -> usize {
    usize::from(height) / usize::from(row_height.max(1))
}

/// Every line one row occupies, in draw order.
pub fn row_lines(
    row: &IndexRow,
    width: u16,
    context: &RowContext,
    query: Option<&str>,
) -> Vec<Line<'static>> {
    match context.layout {
        IndexLayout::Cards => card::card_lines(row, width, context, query),
        IndexLayout::Table => vec![table::row_line(row, width, context, query)],
    }
}

/// Base by selection state (selected > hovered > marked > banding),
/// then a line's own emphasis, then the theme's role patches in a fixed
/// order so precedence is deterministic: unseen, flagged, deleted,
/// reading. Roles land last because a message's state matters more than
/// the hierarchy inside its card.
fn row_style(row: &IndexRow, styles: &RowStyles, emphasis: Style) -> Style {
    let mut style = if row.selected {
        styles.selected
    } else if row.hovered {
        styles.hovered
    } else if row.marked {
        styles.marked
    } else if row.striped {
        styles.normal.patch(styles.stripe)
    } else {
        styles.normal
    }
    .patch(emphasis);
    if !row.flags.contains(Flags::SEEN) {
        style = style.patch(styles.unseen);
    }
    if row.flags.contains(Flags::FLAGGED) {
        style = style.patch(styles.flagged);
    }
    if row.flags.contains(Flags::DELETED) {
        style = style.patch(styles.deleted);
    }
    if row.reading {
        style = style.patch(styles.reading);
    }
    style
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn flagged_rows_are_tinted_but_keep_the_selected_background() {
        let styles = RowStyles::from_theme(&nitidus_ui_kit::theme::tailwind_dark());
        let flagged = IndexRow {
            flags: Flags::default().with(Flags::SEEN).with(Flags::FLAGGED),
            ..IndexRow::default()
        };
        assert_ne!(
            row_style(&flagged, &styles, Style::default()),
            styles.normal
        );

        let selected_flagged = IndexRow {
            selected: true,
            ..flagged
        };
        assert_eq!(
            row_style(&selected_flagged, &styles, Style::default()).bg,
            styles.selected.bg
        );
        assert_eq!(
            row_style(&selected_flagged, &styles, Style::default()).fg,
            styles.flagged.fg,
            "the tint patches fg over the selected base"
        );
    }

    #[test]
    fn hover_beats_marked_and_loses_to_selected() {
        let styles = RowStyles::from_theme(&nitidus_ui_kit::theme::tailwind_dark());
        let hovered_marked = IndexRow {
            hovered: true,
            marked: true,
            ..IndexRow::default()
        };
        assert_eq!(
            row_style(&hovered_marked, &styles, Style::default()).bg,
            styles.hovered.bg,
            "hover highlights over the marked base"
        );
        let hovered_selected = IndexRow {
            hovered: true,
            selected: true,
            ..IndexRow::default()
        };
        assert_eq!(
            row_style(&hovered_selected, &styles, Style::default()).bg,
            styles.selected.bg,
            "the selection stays visually dominant"
        );
    }

    #[test]
    fn banding_is_the_quietest_base_and_yields_to_every_state() {
        let styles = RowStyles::from_theme(&nitidus_ui_kit::theme::tailwind_dark());
        let striped = IndexRow {
            flags: Flags::default().with(Flags::SEEN),
            striped: true,
            ..IndexRow::default()
        };
        assert_eq!(
            row_style(&striped, &styles, Style::default()).bg,
            styles.stripe.bg
        );
        assert_ne!(
            row_style(&striped, &styles, Style::default()).bg,
            styles.normal.bg
        );

        for state in [
            IndexRow {
                selected: true,
                ..striped.clone()
            },
            IndexRow {
                hovered: true,
                ..striped.clone()
            },
            IndexRow {
                marked: true,
                ..striped.clone()
            },
        ] {
            assert_ne!(
                row_style(&state, &styles, Style::default()).bg,
                styles.stripe.bg,
                "an interaction state must paint over the banding"
            );
        }
    }

    #[test]
    fn viewport_rows_divides_lines_by_row_height() {
        assert_eq!(viewport_rows(30, 1), 30);
        assert_eq!(viewport_rows(30, 3), 10);
        assert_eq!(viewport_rows(31, 3), 10, "a partial card is not a row");
        assert_eq!(viewport_rows(2, 3), 0);
        assert_eq!(viewport_rows(10, 0), 10, "a zero height cannot divide");
    }

    #[test]
    fn each_layout_renders_its_own_number_of_lines() {
        let row = IndexRow {
            from: "Ada".to_owned(),
            subject: "hello".to_owned(),
            date: "Jul 24".to_owned(),
            ..IndexRow::default()
        };
        for layout in [IndexLayout::Cards, IndexLayout::Table] {
            let context = RowContext {
                layout,
                ..RowContext::default()
            };
            assert_eq!(
                row_lines(&row, 40, &context, None).len(),
                usize::from(layout.row_height()),
                "{layout:?} must draw exactly the lines its row height claims"
            );
        }
    }

    #[test]
    fn threaded_subjects_indent_replies_and_badge_folds() {
        assert_eq!(threaded_subject("hello", 0, 0), "hello");
        assert_eq!(threaded_subject("hello", 1, 0), "↳ hello");
        assert_eq!(threaded_subject("hello", 3, 0), "    ↳ hello");
        assert_eq!(threaded_subject("hello", 0, 3), "[+3] hello");
    }
}
