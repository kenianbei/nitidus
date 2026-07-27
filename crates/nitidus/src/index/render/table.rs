//! The one-line-per-message layout: a flag cell, and columns resolved
//! against the actual pane width at draw.

use std::borrow::Cow;

use nitidus_mail::Flags;
use ratatui::style::Style;
use ratatui::text::Line;

use super::text::{fit, styled_line};
use super::{IndexRow, RowContext};
use crate::config::IndexColumn;

const FLAGS_WIDTH: usize = 4;
const DATE_WIDTH: usize = 12;
const FROM_PERCENT: usize = 30;
const FROM_MAX: usize = 30;
const COLUMN_GAP: usize = 1;

fn flag_cell(flags: Flags) -> String {
    let mut cell = String::new();
    if !flags.contains(Flags::SEEN) {
        cell.push('N');
    }
    if flags.contains(Flags::FLAGGED) {
        cell.push('F');
    }
    if flags.contains(Flags::ANSWERED) {
        cell.push('R');
    }
    if flags.contains(Flags::DELETED) {
        cell.push('D');
    }
    if flags.contains(Flags::DRAFT) {
        cell.push('d');
    }
    cell
}

fn cell_text(row: &IndexRow, column: IndexColumn) -> Cow<'_, str> {
    match column {
        IndexColumn::Flags if row.marked => Cow::Owned(format!("*{}", flag_cell(row.flags))),
        IndexColumn::Flags => Cow::Owned(flag_cell(row.flags)),
        IndexColumn::Date => Cow::Borrowed(row.date.as_str()),
        IndexColumn::From => Cow::Borrowed(row.from.as_str()),
        IndexColumn::Subject => Cow::Borrowed(row.subject.as_str()),
    }
}

fn cell_width(column: IndexColumn, columns: &[IndexColumn], pane: usize) -> usize {
    match column {
        IndexColumn::Flags => FLAGS_WIDTH,
        IndexColumn::Date => DATE_WIDTH,
        IndexColumn::From => from_width(pane),
        IndexColumn::Subject => subject_width(columns, pane),
    }
}

fn from_width(pane: usize) -> usize {
    ((pane * FROM_PERCENT) / 100).min(FROM_MAX)
}

/// Subject absorbs whatever the fixed columns and gaps leave over.
fn subject_width(columns: &[IndexColumn], pane: usize) -> usize {
    let gaps = columns.len().saturating_sub(1) * COLUMN_GAP;
    let fixed: usize = columns
        .iter()
        .map(|column| match column {
            IndexColumn::Flags => FLAGS_WIDTH,
            IndexColumn::Date => DATE_WIDTH,
            IndexColumn::From => from_width(pane),
            IndexColumn::Subject => 0,
        })
        .sum();
    pane.saturating_sub(fixed + gaps)
}

pub(super) fn row_line(
    row: &IndexRow,
    width: u16,
    context: &RowContext,
    query: Option<&str>,
) -> Line<'static> {
    let style = super::row_style(row, &context.styles, Style::default());
    let width = usize::from(width);
    let cells: Vec<String> = context
        .columns
        .iter()
        .map(|&column| {
            fit(
                &cell_text(row, column),
                cell_width(column, &context.columns, width),
            )
        })
        .collect();
    // The trailing fit pads the line to pane width, so the last column
    // always stretches even without a subject.
    let fitted = fit(&cells.join(&" ".repeat(COLUMN_GAP)), width);
    styled_line(fitted, style, context.styles.highlight, query)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use ratatui::style::{Modifier, Style};

    use super::*;
    use crate::config::IndexLayout;
    use crate::index::render::RowStyles;

    fn table_context(columns: Vec<IndexColumn>) -> RowContext {
        RowContext {
            columns,
            layout: IndexLayout::Table,
            ..RowContext::default()
        }
    }

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn flag_cell_orders_and_selects_chars() {
        assert_eq!(flag_cell(Flags::default()), "N");
        assert_eq!(flag_cell(Flags::default().with(Flags::SEEN)), "");
        assert_eq!(
            flag_cell(
                Flags::default()
                    .with(Flags::FLAGGED)
                    .with(Flags::ANSWERED)
                    .with(Flags::DELETED)
            ),
            "NFRD"
        );
        assert_eq!(
            flag_cell(Flags::default().with(Flags::SEEN).with(Flags::DRAFT)),
            "d"
        );
    }

    #[test]
    fn a_marked_row_prefixes_its_flag_cell() {
        let row = IndexRow {
            flags: Flags::default().with(Flags::SEEN),
            marked: true,
            ..IndexRow::default()
        };
        assert_eq!(cell_text(&row, IndexColumn::Flags), "*");
    }

    #[test]
    fn default_columns_fill_exact_width_in_the_established_order() {
        let row = IndexRow {
            flags: Flags::default().with(Flags::FLAGGED),
            date: "Jul 24".to_owned(),
            from: "Alice Example".to_owned(),
            subject: "a very long subject line that will not fit".to_owned(),
            ..IndexRow::default()
        };
        let context = table_context(RowContext::default().columns);
        let text = line_text(&row_line(&row, 60, &context, None));
        assert_eq!(text.chars().count(), 60);
        assert!(text.starts_with("NF   Jul 24"), "{text:?}");
    }

    #[test]
    fn configured_columns_render_in_order_and_subset() {
        let row = IndexRow {
            date: "Jul 24".to_owned(),
            from: "Alice".to_owned(),
            subject: "hello".to_owned(),
            ..IndexRow::default()
        };
        let reordered = table_context(vec![
            IndexColumn::Date,
            IndexColumn::Subject,
            IndexColumn::From,
        ]);
        let text = line_text(&row_line(&row, 40, &reordered, None));
        assert_eq!(text.chars().count(), 40);
        assert!(text.starts_with("Jul 24       hello"), "{text:?}");
        assert!(!text.contains('N'), "flags column was dropped: {text:?}");

        let no_subject = table_context(vec![IndexColumn::Flags, IndexColumn::From]);
        let text = line_text(&row_line(&row, 40, &no_subject, None));
        assert_eq!(text.chars().count(), 40, "line still fills the pane");
        assert!(!text.contains("hello"), "{text:?}");
    }

    #[test]
    fn a_query_match_is_highlighted_in_the_rendered_line() {
        let row = IndexRow {
            flags: Flags::default().with(Flags::SEEN),
            date: "Jul 25".to_owned(),
            from: "Ada".to_owned(),
            subject: "quarterly report".to_owned(),
            ..IndexRow::default()
        };
        let context = RowContext {
            styles: RowStyles {
                highlight: Style::default().add_modifier(Modifier::BOLD),
                ..RowStyles::default()
            },
            ..table_context(RowContext::default().columns)
        };
        let line = row_line(&row, 60, &context, Some("report"));
        assert_eq!(line.spans.len(), 3, "prefix, match, suffix");
        assert_eq!(line.spans[1].content.as_ref(), "report");
    }
}
