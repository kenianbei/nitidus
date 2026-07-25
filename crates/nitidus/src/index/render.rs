//! Row formatting for the index window: flag cells, smart short dates,
//! and column layout resolved against the actual pane width at draw.

use jiff::Zoned;
use nitidus_mail::{EnvelopeSummary, Flags};
use nitidus_ui_kit::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const FLAGS_WIDTH: usize = 4;
const DATE_WIDTH: usize = 12;
const FROM_PERCENT: usize = 30;
const FROM_MAX: usize = 30;
const ELLIPSIS: char = '…';

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexRow {
    pub flag_cell: String,
    pub date: String,
    pub from: String,
    pub subject: String,
    pub unseen: bool,
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
            marked: theme.base.info.normal.style(),
        }
    }
}

pub(super) fn build_row(
    envelope: &EnvelopeSummary,
    entry: &super::thread_view::OrderEntry,
    selected: bool,
    marked: bool,
    now: &Zoned,
) -> IndexRow {
    IndexRow {
        flag_cell: if marked {
            format!("*{}", flag_cell(envelope.flags))
        } else {
            flag_cell(envelope.flags)
        },
        date: format_date(envelope.date_epoch_secs, now),
        from: envelope.from_display.clone(),
        subject: threaded_subject(&envelope.subject, entry.depth, entry.collapsed_children),
        unseen: !envelope.flags.contains(Flags::SEEN),
        deleted: envelope.flags.contains(Flags::DELETED),
        selected,
        marked,
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

/// `HH:MM` today, `Jul 24` this year, `2024-02-15` otherwise.
pub fn format_date(epoch_secs: i64, now: &Zoned) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_second(epoch_secs) else {
        return String::new();
    };
    let zoned = timestamp.to_zoned(now.time_zone().clone());
    let pattern = if zoned.date() == now.date() {
        "%H:%M"
    } else if zoned.year() == now.year() {
        "%b %d"
    } else {
        "%Y-%m-%d"
    };
    jiff::fmt::strtime::format(pattern, &zoned).unwrap_or_default()
}

pub fn row_line(
    row: &IndexRow,
    width: u16,
    styles: &RowStyles,
    query: Option<&str>,
) -> Line<'static> {
    let style = row_style(row, styles);
    let width = usize::from(width);
    let from_width = ((width * FROM_PERCENT) / 100).min(FROM_MAX);
    let subject_width = width.saturating_sub(FLAGS_WIDTH + DATE_WIDTH + from_width + 3);
    let text = format!(
        "{} {} {} {}",
        fit(&row.flag_cell, FLAGS_WIDTH),
        fit(&row.date, DATE_WIDTH),
        fit(&row.from, from_width),
        fit(&row.subject, subject_width),
    );
    let fitted = fit(&text, width);
    // Highlight the first match in the rendered line — truncated-away
    // matches simply do not light up.
    if let Some(query) = query
        && let Some((start, end)) = super::filter::match_range(&fitted, query)
    {
        return Line::from(vec![
            Span::styled(fitted[..start].to_owned(), style),
            Span::styled(fitted[start..end].to_owned(), style.patch(styles.highlight)),
            Span::styled(fitted[end..].to_owned(), style),
        ]);
    }
    Line::from(Span::styled(fitted, style))
}

fn row_style(row: &IndexRow, styles: &RowStyles) -> Style {
    let mut style = if row.selected {
        styles.selected
    } else if row.marked {
        styles.marked
    } else {
        styles.normal
    };
    if row.unseen {
        style = style.add_modifier(Modifier::BOLD);
    }
    if row.deleted {
        style = style.add_modifier(Modifier::DIM);
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
            deleted: false,
            selected: false,
            marked: false,
        };
        let styles = RowStyles {
            highlight: Style::default().add_modifier(Modifier::BOLD),
            ..RowStyles::default()
        };
        let line = row_line(&row, 60, &styles, Some("report"));
        assert_eq!(line.spans.len(), 3, "prefix, match, suffix");
        assert_eq!(line.spans[1].content.as_ref(), "report");
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));

        let unmatched = row_line(&row, 60, &styles, Some("nowhere"));
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
    fn dates_shorten_by_recency() {
        let now = zoned("2026-07-24T15:00:00+00:00[UTC]");
        let same_day = zoned("2026-07-24T09:30:00+00:00[UTC]");
        let same_year = zoned("2026-02-15T12:00:00+00:00[UTC]");
        let older = zoned("2024-02-15T12:00:00+00:00[UTC]");
        assert_eq!(format_date(same_day.timestamp().as_second(), &now), "09:30");
        assert_eq!(
            format_date(same_year.timestamp().as_second(), &now),
            "Feb 15"
        );
        assert_eq!(
            format_date(older.timestamp().as_second(), &now),
            "2024-02-15"
        );
        assert_eq!(format_date(i64::MAX, &now), "");
    }

    #[test]
    fn fit_pads_and_truncates_to_width() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcd", 4), "abcd");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 0), "");
    }

    #[test]
    fn row_line_fills_exact_width() {
        let row = IndexRow {
            flag_cell: "NF".to_owned(),
            date: "Jul 24".to_owned(),
            from: "Alice Example".to_owned(),
            subject: "a very long subject line that will not fit".to_owned(),
            ..IndexRow::default()
        };
        let line = row_line(&row, 60, &RowStyles::default(), None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text.chars().count(), 60);
        assert!(text.starts_with("NF   Jul 24"), "{text:?}");
    }
}
