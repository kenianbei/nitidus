//! Pure body-text pipeline: format=flowed reflow, quote-depth
//! classification, width wrapping with quote prefixes preserved,
//! signature detection, link extraction, and skip-quoted targets.

use nitidus_mail::message::PartView;

const SIGNATURE_MARKER: &str = "-- ";
const LINK_TRAILING_TRIM: &[char] = &['.', ',', ')', ']', '>', ';', ':', '\'', '"'];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
    #[default]
    Normal,
    Quote(u8),
    Signature,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyLine {
    pub text: String,
    pub kind: LineKind,
}

pub fn build_body_lines(part: &PartView, width: usize) -> Vec<BodyLine> {
    let text = part.text.as_deref().unwrap_or_default();
    let logical = if part.is_flowed {
        reflow_flowed(text, part.delete_space)
    } else {
        text.lines().map(str::to_owned).collect()
    };
    let width = width.max(16);
    let kinds = classify_lines(&logical);
    let mut lines = Vec::new();
    for (logical_line, kind) in logical.iter().zip(kinds) {
        for wrapped in wrap_line(logical_line, kind, width) {
            lines.push(BodyLine {
                text: wrapped,
                kind,
            });
        }
    }
    lines
}

/// Classifies a whole body, carrying signature state forward: everything
/// after the `-- ` separator is signature, whatever it looks like. Shared
/// with the composer so a quote reads the same on both sides of the app.
pub(crate) fn classify_lines(lines: &[String]) -> Vec<LineKind> {
    let mut in_signature = false;
    lines
        .iter()
        .map(|line| {
            if line == SIGNATURE_MARKER || line.trim_end() == "--" {
                in_signature = true;
            }
            classify(line, in_signature)
        })
        .collect()
}

/// RFC 3676: a line ending in a space flows into the next line of the
/// same quote depth; space-stuffing is removed; `delsp` drops the
/// trailing flow space itself.
fn reflow_flowed(text: &str, delete_space: bool) -> Vec<String> {
    let mut logical: Vec<String> = Vec::new();
    let mut flowing = false;
    for raw_line in text.lines() {
        let depth = quote_depth(raw_line);
        let unstuffed = unstuff(strip_quotes(raw_line));
        let is_flow = unstuffed.ends_with(' ') && unstuffed != SIGNATURE_MARKER;
        let mut content = unstuffed.to_owned();
        if is_flow && delete_space {
            content.pop();
        }
        match logical.last_mut() {
            Some(previous) if flowing && quote_depth(previous) == depth => {
                previous.push_str(&content);
            }
            _ => logical.push(format!("{}{content}", quote_prefix(depth))),
        }
        flowing = is_flow;
    }
    logical
}

fn classify(line: &str, in_signature: bool) -> LineKind {
    if in_signature {
        return LineKind::Signature;
    }
    match quote_depth(line) {
        0 => LineKind::Normal,
        depth => LineKind::Quote(depth),
    }
}

/// `> ` runs, tolerating the common `>>` and `> >` shapes.
pub fn quote_depth(line: &str) -> u8 {
    let mut depth = 0u8;
    let mut rest = line;
    loop {
        rest = rest.trim_start_matches(' ');
        match rest.strip_prefix('>') {
            Some(after) => {
                depth = depth.saturating_add(1);
                rest = after;
            }
            None => return depth,
        }
    }
}

fn strip_quotes(line: &str) -> &str {
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start_matches(' ');
        match trimmed.strip_prefix('>') {
            Some(after) => rest = after,
            None => return rest.strip_prefix(' ').unwrap_or(rest),
        }
    }
}

/// Space-stuffed lines (RFC 3676 §4.4) start with an escape space.
fn unstuff(line: &str) -> &str {
    line.strip_prefix(' ').unwrap_or(line)
}

fn quote_prefix(depth: u8) -> String {
    "> ".repeat(usize::from(depth))
}

fn wrap_line(line: &str, kind: LineKind, width: usize) -> Vec<String> {
    if line.chars().count() <= width {
        return vec![line.to_owned()];
    }
    let prefix = match kind {
        LineKind::Quote(depth) => quote_prefix(depth),
        _ => String::new(),
    };
    let options = textwrap::Options::new(width).subsequent_indent(&prefix);
    textwrap::wrap(line, options)
        .into_iter()
        .map(|piece| piece.into_owned())
        .collect()
}

pub fn extract_links(lines: &[BodyLine]) -> Vec<String> {
    let mut links = Vec::new();
    for line in lines {
        for scheme_start in line.text.match_indices("http").map(|(index, _)| index) {
            let candidate = &line.text[scheme_start..];
            if !candidate.starts_with("http://") && !candidate.starts_with("https://") {
                continue;
            }
            let end = candidate
                .find(|c: char| c.is_whitespace() || c == '<' || c == '(')
                .unwrap_or(candidate.len());
            let url = candidate[..end].trim_end_matches(LINK_TRAILING_TRIM);
            if !url.is_empty() && !links.iter().any(|existing| existing == url) {
                links.push(url.to_owned());
            }
        }
    }
    links
}

/// First non-quoted line after the quote block at or below `from`.
pub fn skip_quoted_target(kinds: &[LineKind], from: usize) -> Option<usize> {
    let mut index = from;
    while index < kinds.len() && !matches!(kinds[index], LineKind::Quote(_)) {
        index += 1;
    }
    while index < kinds.len() && matches!(kinds[index], LineKind::Quote(_)) {
        index += 1;
    }
    (index < kinds.len() && index != from).then_some(index)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn part(text: &str, flowed: bool, delete_space: bool) -> PartView {
        PartView {
            kind: nitidus_mail::message::PartKind::Text,
            mime: "text/plain".to_owned(),
            filename: None,
            text: Some(text.to_owned()),
            size: text.len(),
            is_attachment: false,
            is_flowed: flowed,
            delete_space,
            source_index: 0,
        }
    }

    #[test]
    fn flowed_lines_merge_within_a_quote_depth() {
        let text = "This is a flowed \nparagraph of text.\n> quoted flow \n> continues here\nafter";
        let lines = build_body_lines(&part(text, true, false), 80);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "This is a flowed paragraph of text.",
                "> quoted flow continues here",
                "after"
            ]
        );
        assert_eq!(lines[1].kind, LineKind::Quote(1));
    }

    #[test]
    fn delsp_drops_the_flow_space() {
        let text = "hyphen \nated";
        let lines = build_body_lines(&part(text, true, true), 80);
        assert_eq!(lines[0].text, "hyphenated");
    }

    #[test]
    fn wrapping_preserves_quote_prefixes() {
        let text = "> a quoted line that is definitely much longer than the wrap width in play";
        let lines = build_body_lines(&part(text, false, false), 30);
        assert!(lines.len() > 1);
        assert!(
            lines.iter().all(|line| line.text.starts_with('>')),
            "{lines:?}"
        );
        assert!(lines.iter().all(|line| line.kind == LineKind::Quote(1)));
    }

    #[test]
    fn signature_marker_dims_the_rest() {
        let text = "body\n-- \nAlice\nalice@example.com";
        let lines = build_body_lines(&part(text, false, false), 80);
        assert_eq!(lines[0].kind, LineKind::Normal);
        assert!(
            lines[1..]
                .iter()
                .all(|line| line.kind == LineKind::Signature),
            "{lines:?}"
        );
    }

    #[test]
    fn quote_depth_tolerates_spacing_styles() {
        assert_eq!(quote_depth("no quote"), 0);
        assert_eq!(quote_depth("> one"), 1);
        assert_eq!(quote_depth(">> two"), 2);
        assert_eq!(quote_depth("> > two"), 2);
    }

    #[test]
    fn links_extract_deduped_and_trimmed() {
        let lines = build_body_lines(
            &part(
                "see https://example.com/a. and (http://b.example/x) plus https://example.com/a",
                false,
                false,
            ),
            200,
        );
        assert_eq!(
            extract_links(&lines),
            vec!["https://example.com/a", "http://b.example/x"]
        );
    }

    #[test]
    fn skip_quoted_jumps_past_the_block() {
        let text = "intro\n> q1\n> q2\nreply here\n> more\nend";
        let lines = build_body_lines(&part(text, false, false), 80);
        let kinds: Vec<LineKind> = lines.iter().map(|line| line.kind).collect();
        assert_eq!(skip_quoted_target(&kinds, 0), Some(3));
        assert_eq!(skip_quoted_target(&kinds, 3), Some(5));
        assert_eq!(skip_quoted_target(&kinds, 5), None);
    }
}
