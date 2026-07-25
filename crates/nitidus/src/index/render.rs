//! Row formatting for the index window: flag cells, smart short dates,
//! and column layout resolved against the actual pane width at draw.

use jiff::Zoned;
use nitidus_mail::{EnvelopeSummary, Flags};
use nitidus_ui_kit::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::{DateFormat, IndexColumn, IndexUiConfig};

const FLAGS_WIDTH: usize = 4;
const DATE_WIDTH: usize = 12;
const FROM_PERCENT: usize = 30;
const FROM_MAX: usize = 30;
const COLUMN_GAP: usize = 1;
const ELLIPSIS: char = '…';

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexRow {
    pub flag_cell: String,
    pub date: String,
    pub from: String,
    pub subject: String,
    pub unseen: bool,
    pub flagged: bool,
    pub deleted: bool,
    pub selected: bool,
    pub marked: bool,
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
        }
    }
}

pub(super) struct RowBuildContext<'a> {
    pub now: &'a Zoned,
    pub date: DateFormat,
    pub selected: bool,
    pub marked: bool,
}

/// Column order and styles resolved once per refresh, shared by every
/// row of the window.
#[derive(Clone, Debug)]
pub struct RowContext {
    pub styles: RowStyles,
    pub columns: Vec<IndexColumn>,
}

impl Default for RowContext {
    fn default() -> Self {
        Self {
            styles: RowStyles::default(),
            columns: IndexUiConfig::default().columns,
        }
    }
}

pub(super) fn build_row(
    envelope: &EnvelopeSummary,
    entry: &super::thread_view::OrderEntry,
    context: &RowBuildContext<'_>,
) -> IndexRow {
    IndexRow {
        flag_cell: if context.marked {
            format!("*{}", flag_cell(envelope.flags))
        } else {
            flag_cell(envelope.flags)
        },
        date: format_date(envelope.date_epoch_secs, context.now, context.date),
        from: envelope.from_display.clone(),
        subject: threaded_subject(&envelope.subject, entry.depth, entry.collapsed_children),
        unseen: !envelope.flags.contains(Flags::SEEN),
        flagged: envelope.flags.contains(Flags::FLAGGED),
        deleted: envelope.flags.contains(Flags::DELETED),
        selected: context.selected,
        marked: context.marked,
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

pub fn flag_cell(flags: Flags) -> String {
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

const TIME_PATTERN: &str = "%H:%M";
const SHORT_PATTERN: &str = "%b %d";
const ISO_PATTERN: &str = "%Y-%m-%d";

pub fn format_date(epoch_secs: i64, now: &Zoned, format: DateFormat) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_second(epoch_secs) else {
        return String::new();
    };
    let zoned = timestamp.to_zoned(now.time_zone().clone());
    let pattern = match format {
        DateFormat::Time => TIME_PATTERN,
        DateFormat::Short => SHORT_PATTERN,
        DateFormat::Iso => ISO_PATTERN,
        DateFormat::Auto => auto_pattern(&zoned, now),
    };
    jiff::fmt::strtime::format(pattern, &zoned).unwrap_or_default()
}

/// Time today, `Jul 24` this year, ISO otherwise.
fn auto_pattern(zoned: &Zoned, now: &Zoned) -> &'static str {
    if zoned.date() == now.date() {
        TIME_PATTERN
    } else if zoned.year() == now.year() {
        SHORT_PATTERN
    } else {
        ISO_PATTERN
    }
}

fn cell_text(row: &IndexRow, column: IndexColumn) -> &str {
    match column {
        IndexColumn::Flags => &row.flag_cell,
        IndexColumn::Date => &row.date,
        IndexColumn::From => &row.from,
        IndexColumn::Subject => &row.subject,
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

pub fn row_line(
    row: &IndexRow,
    width: u16,
    context: &RowContext,
    query: Option<&str>,
) -> Line<'static> {
    let style = row_style(row, &context.styles);
    let width = usize::from(width);
    let cells: Vec<String> = context
        .columns
        .iter()
        .map(|&column| {
            fit(
                cell_text(row, column),
                cell_width(column, &context.columns, width),
            )
        })
        .collect();
    // The trailing fit pads the line to pane width, so the last column
    // always stretches even without a subject.
    let fitted = fit(&cells.join(&" ".repeat(COLUMN_GAP)), width);
    // Highlight the first match in the rendered line — truncated-away
    // matches simply do not light up.
    if let Some(query) = query
        && let Some((start, end)) = super::filter::match_range(&fitted, query)
    {
        return Line::from(vec![
            Span::styled(fitted[..start].to_owned(), style),
            Span::styled(
                fitted[start..end].to_owned(),
                style.patch(context.styles.highlight),
            ),
            Span::styled(fitted[end..].to_owned(), style),
        ]);
    }
    Line::from(Span::styled(fitted, style))
}

/// Base by selection state, then the theme's role patches in a fixed
/// order so precedence is deterministic: unseen, flagged, deleted.
fn row_style(row: &IndexRow, styles: &RowStyles) -> Style {
    let mut style = if row.selected {
        styles.selected
    } else if row.marked {
        styles.marked
    } else {
        styles.normal
    };
    if row.unseen {
        style = style.patch(styles.unseen);
    }
    if row.flagged {
        style = style.patch(styles.flagged);
    }
    if row.deleted {
        style = style.patch(styles.deleted);
    }
    style
}

/// Pads or truncates (with an ellipsis) to exactly `width` chars.
fn fit(text: &str, width: usize) -> String {
    let length = text.chars().count();
    if length == width {
        return text.to_owned();
    }
    if length < width {
        let mut padded = String::with_capacity(width);
        padded.push_str(text);
        padded.extend(std::iter::repeat_n(' ', width - length));
        return padded;
    }
    if width == 0 {
        return String::new();
    }
    let mut truncated: String = text.chars().take(width - 1).collect();
    truncated.push(ELLIPSIS);
    truncated
}

#[cfg(test)]
mod tests_highlight {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn matching_query_splits_the_row_into_highlight_spans() {
        let row = IndexRow {
            flag_cell: String::new(),
            date: "Jul 25".to_owned(),
            from: "Ada".to_owned(),
            subject: "quarterly report".to_owned(),
            unseen: false,
            flagged: false,
            deleted: false,
            selected: false,
            marked: false,
        };
        let context = RowContext {
            styles: RowStyles {
                highlight: Style::default().add_modifier(Modifier::BOLD),
                ..RowStyles::default()
            },
            ..RowContext::default()
        };
        let line = row_line(&row, 60, &context, Some("report"));
        assert_eq!(line.spans.len(), 3, "prefix, match, suffix");
        assert_eq!(line.spans[1].content.as_ref(), "report");
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));

        let unmatched = row_line(&row, 60, &context, Some("nowhere"));
        assert_eq!(unmatched.spans.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn zoned(fields: &str) -> Zoned {
        fields.parse().unwrap()
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
    fn auto_dates_shorten_by_recency() {
        let now = zoned("2026-07-24T15:00:00+00:00[UTC]");
        let same_day = zoned("2026-07-24T09:30:00+00:00[UTC]");
        let same_year = zoned("2026-02-15T12:00:00+00:00[UTC]");
        let older = zoned("2024-02-15T12:00:00+00:00[UTC]");
        let auto = |epoch| format_date(epoch, &now, DateFormat::Auto);
        assert_eq!(auto(same_day.timestamp().as_second()), "09:30");
        assert_eq!(auto(same_year.timestamp().as_second()), "Feb 15");
        assert_eq!(auto(older.timestamp().as_second()), "2024-02-15");
        assert_eq!(auto(i64::MAX), "");
    }

    #[test]
    fn forced_date_formats_ignore_recency() {
        let now = zoned("2026-07-24T15:00:00+00:00[UTC]");
        let today = zoned("2026-07-24T09:30:00+00:00[UTC]")
            .timestamp()
            .as_second();
        assert_eq!(format_date(today, &now, DateFormat::Time), "09:30");
        assert_eq!(format_date(today, &now, DateFormat::Short), "Jul 24");
        assert_eq!(format_date(today, &now, DateFormat::Iso), "2026-07-24");
    }

    #[test]
    fn flagged_rows_are_tinted_but_keep_the_selected_background() {
        let styles = RowStyles::from_theme(&nitidus_ui_kit::theme::tailwind_dark());
        let flagged = IndexRow {
            flagged: true,
            ..IndexRow::default()
        };
        assert_ne!(row_style(&flagged, &styles), styles.normal);

        let selected_flagged = IndexRow {
            flagged: true,
            selected: true,
            ..IndexRow::default()
        };
        assert_eq!(row_style(&selected_flagged, &styles).bg, styles.selected.bg);
        assert_eq!(
            row_style(&selected_flagged, &styles).fg,
            styles.flagged.fg,
            "the tint patches fg over the selected base"
        );
    }

    #[test]
    fn fit_pads_and_truncates_to_width() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcd", 4), "abcd");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 0), "");
    }

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn default_columns_fill_exact_width_in_the_established_order() {
        let row = IndexRow {
            flag_cell: "NF".to_owned(),
            date: "Jul 24".to_owned(),
            from: "Alice Example".to_owned(),
            subject: "a very long subject line that will not fit".to_owned(),
            ..IndexRow::default()
        };
        let text = line_text(&row_line(&row, 60, &RowContext::default(), None));
        assert_eq!(text.chars().count(), 60);
        assert!(text.starts_with("NF   Jul 24"), "{text:?}");
    }

    #[test]
    fn configured_columns_render_in_order_and_subset() {
        let row = IndexRow {
            flag_cell: "N".to_owned(),
            date: "Jul 24".to_owned(),
            from: "Alice".to_owned(),
            subject: "hello".to_owned(),
            ..IndexRow::default()
        };
        let reordered = RowContext {
            columns: vec![IndexColumn::Date, IndexColumn::Subject, IndexColumn::From],
            ..RowContext::default()
        };
        let text = line_text(&row_line(&row, 40, &reordered, None));
        assert_eq!(text.chars().count(), 40);
        assert!(text.starts_with("Jul 24       hello"), "{text:?}");
        assert!(!text.contains('N'), "flags column was dropped: {text:?}");

        let no_subject = RowContext {
            columns: vec![IndexColumn::Flags, IndexColumn::From],
            ..RowContext::default()
        };
        let text = line_text(&row_line(&row, 40, &no_subject, None));
        assert_eq!(text.chars().count(), 40, "line still fills the pane");
        assert!(!text.contains("hello"), "{text:?}");
    }
}
