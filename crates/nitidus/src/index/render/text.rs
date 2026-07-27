//! Cell text shared by both index layouts: exact-width fitting and the
//! search-match highlight, which can only ever light up what survived
//! truncation.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

const ELLIPSIS: char = '…';

/// Pads or truncates (with an ellipsis) to exactly `width` chars.
pub(super) fn fit(text: &str, width: usize) -> String {
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

/// One rendered line, with the first query match split out so the
/// highlight style can patch it.
pub(super) fn styled_line(
    text: String,
    style: Style,
    highlight: Style,
    query: Option<&str>,
) -> Line<'static> {
    if let Some(query) = query
        && let Some((start, end)) = crate::index::filter::match_range(&text, query)
    {
        return Line::from(vec![
            Span::styled(text[..start].to_owned(), style),
            Span::styled(text[start..end].to_owned(), style.patch(highlight)),
            Span::styled(text[end..].to_owned(), style),
        ]);
    }
    Line::from(Span::styled(text, style))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use ratatui::style::Modifier;

    use super::*;

    #[test]
    fn fit_pads_and_truncates_to_width() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcd", 4), "abcd");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 0), "");
    }

    #[test]
    fn matching_query_splits_the_line_into_highlight_spans() {
        let highlight = Style::default().add_modifier(Modifier::BOLD);
        let line = styled_line(
            "quarterly report".to_owned(),
            Style::default(),
            highlight,
            Some("report"),
        );
        assert_eq!(line.spans.len(), 3, "prefix, match, suffix");
        assert_eq!(line.spans[1].content.as_ref(), "report");
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));

        let unmatched = styled_line(
            "quarterly report".to_owned(),
            Style::default(),
            highlight,
            Some("nowhere"),
        );
        assert_eq!(unmatched.spans.len(), 1);
    }
}
