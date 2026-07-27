//! Index date display: the configured format, plus the smart shortening
//! the table's narrow date column relies on.

use jiff::Zoned;

use crate::config::{DateFormat, IndexLayout};

const TIME_PATTERN: &str = "%H:%M";
const SHORT_PATTERN: &str = "%b %d";
const ISO_PATTERN: &str = "%Y-%m-%d";
/// Always 22 columns wide, whatever the date — the narrowest a card's
/// list pane can usefully be.
const FULL_PATTERN: &str = "%a, %d %b %Y %H:%M";

/// A card gives the date a line of its own, so `auto` there means the
/// unabbreviated form rather than the table's recency tiers.
pub(crate) fn resolve(format: DateFormat, layout: IndexLayout) -> DateFormat {
    match (format, layout) {
        (DateFormat::Auto, IndexLayout::Cards) => DateFormat::Full,
        _ => format,
    }
}

pub(super) fn format_date(epoch_secs: i64, now: &Zoned, format: DateFormat) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_second(epoch_secs) else {
        return String::new();
    };
    let zoned = timestamp.to_zoned(now.time_zone().clone());
    let pattern = match format {
        DateFormat::Time => TIME_PATTERN,
        DateFormat::Short => SHORT_PATTERN,
        DateFormat::Iso => ISO_PATTERN,
        DateFormat::Full => FULL_PATTERN,
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn zoned(fields: &str) -> Zoned {
        fields.parse().unwrap()
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
    fn the_full_form_is_the_same_width_for_every_date() {
        let now = zoned("2026-07-24T15:00:00+00:00[UTC]");
        let single_digit_day = zoned("2026-07-02T15:04:00+00:00[UTC]")
            .timestamp()
            .as_second();
        let wide_day = zoned("2026-12-31T15:04:00+00:00[UTC]")
            .timestamp()
            .as_second();
        let full = |epoch| format_date(epoch, &now, DateFormat::Full);
        assert_eq!(full(wide_day), "Thu, 31 Dec 2026 15:04");
        assert_eq!(
            full(wide_day).chars().count(),
            22,
            "the full form must fit the content area of a 24-wide list"
        );
        assert_eq!(
            full(single_digit_day).chars().count(),
            22,
            "zero padding keeps the column stable"
        );
    }

    #[test]
    fn cards_read_auto_as_the_full_form_and_leave_the_table_alone() {
        assert_eq!(
            resolve(DateFormat::Auto, IndexLayout::Cards),
            DateFormat::Full
        );
        assert_eq!(
            resolve(DateFormat::Auto, IndexLayout::Table),
            DateFormat::Auto
        );
        assert_eq!(
            resolve(DateFormat::Short, IndexLayout::Cards),
            DateFormat::Short,
            "an explicit format still wins in a card"
        );
    }
}
