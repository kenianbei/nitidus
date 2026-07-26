//! Attachment tokens: `[[attach: <path>]]`, optionally carrying
//! attributes as `[[attach: <path> | key=value ...]]`.
//!
//! The body is the source of truth for what is attached. `[[…]]` is rare
//! in prose and, unlike `![alt](path)`, does not capture markdown people
//! actually type. `|` terminates the path, so paths may contain spaces
//! without quoting.
//!
//! No attribute is defined yet. The list is parsed and round-tripped
//! verbatim so sizing and inline styling can be added later without a
//! syntax migration.

use std::path::{Path, PathBuf};

const OPEN: &str = "[[attach:";
const CLOSE: &str = "]]";
const ATTRIBUTE_SEPARATOR: char = '|';

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachToken {
    pub path: PathBuf,
    /// `key=value` pairs in source order, preserved even when unknown.
    pub attributes: Vec<(String, String)>,
}

impl AttachToken {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            attributes: Vec::new(),
        }
    }

    /// Renders the canonical form. Round-trips through [`parse`].
    pub fn render(&self) -> String {
        let mut text = format!("{OPEN} {}", self.path.display());
        if !self.attributes.is_empty() {
            let attributes: Vec<String> = self
                .attributes
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            text.push_str(&format!(" {ATTRIBUTE_SEPARATOR} {}", attributes.join(" ")));
        }
        text.push_str(CLOSE);
        text
    }
}

/// Parses a line that is entirely one token, or `None` for ordinary text.
///
/// Tokens occupy a whole line by construction, so anything with text
/// around it is prose that happens to contain brackets.
pub fn parse(line: &str) -> Option<AttachToken> {
    let trimmed = line.trim();
    let body = trimmed.strip_prefix(OPEN)?.strip_suffix(CLOSE)?;
    let (path, attributes) = match body.split_once(ATTRIBUTE_SEPARATOR) {
        Some((path, rest)) => (path, parse_attributes(rest)),
        None => (body, Vec::new()),
    };
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    Some(AttachToken {
        path: PathBuf::from(path),
        attributes,
    })
}

fn parse_attributes(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// Every attachment named by the body, in order.
pub fn tokens(body: &[String]) -> Vec<AttachToken> {
    body.iter().filter_map(|line| parse(line)).collect()
}

/// The attachment paths a body declares — what `ComposeSession` caches.
pub fn paths(body: &[String]) -> Vec<PathBuf> {
    tokens(body).into_iter().map(|token| token.path).collect()
}

/// The body with every token line removed, for building the outgoing
/// message: token text must never reach the wire.
pub fn strip(body: &[String]) -> Vec<String> {
    body.iter()
        .filter(|line| parse(line).is_none())
        .cloned()
        .collect()
}

/// Removes the token naming `path`, leaving everything else untouched.
pub fn remove(body: &[String], path: &Path) -> Vec<String> {
    body.iter()
        .filter(|line| parse(line).is_none_or(|token| token.path != path))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_a_bare_token() {
        let token = parse("[[attach: photos/diagram.png]]").unwrap();
        assert_eq!(token.path, PathBuf::from("photos/diagram.png"));
        assert!(token.attributes.is_empty());
    }

    #[test]
    fn a_path_may_contain_spaces() {
        let token = parse("[[attach: my holiday photo.png]]").unwrap();
        assert_eq!(token.path, PathBuf::from("my holiday photo.png"));
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(
            parse("   [[attach: a.png]]  ").unwrap().path,
            PathBuf::from("a.png")
        );
    }

    #[test]
    fn ordinary_prose_is_not_a_token() {
        assert!(parse("see the attached diagram").is_none());
        assert!(parse("![alt](photo.png)").is_none());
        assert!(
            parse("a [[attach: x.png]] mid-sentence").is_none(),
            "a token owns its whole line"
        );
        assert!(parse("[[attach: ]]").is_none(), "the path is required");
        assert!(parse("[[attach: a.png").is_none(), "unterminated");
    }

    #[test]
    fn attributes_parse_and_round_trip_even_when_unknown() {
        let token = parse("[[attach: a.png | width=40 height=20]]").unwrap();
        assert_eq!(token.path, PathBuf::from("a.png"));
        assert_eq!(
            token.attributes,
            vec![
                ("width".to_owned(), "40".to_owned()),
                ("height".to_owned(), "20".to_owned())
            ]
        );
        assert_eq!(parse(&token.render()).unwrap(), token);
    }

    #[test]
    fn rendering_round_trips_through_parsing() {
        let token = AttachToken::new("notes.txt");
        assert_eq!(token.render(), "[[attach: notes.txt]]");
        assert_eq!(parse(&token.render()).unwrap(), token);
    }

    #[test]
    fn paths_are_collected_in_body_order() {
        let body = vec![
            "hello".to_owned(),
            "[[attach: one.png]]".to_owned(),
            "text".to_owned(),
            "[[attach: two.png]]".to_owned(),
        ];
        assert_eq!(
            paths(&body),
            vec![PathBuf::from("one.png"), PathBuf::from("two.png")]
        );
    }

    #[test]
    fn stripping_removes_only_token_lines() {
        let body = vec![
            "hello".to_owned(),
            "[[attach: one.png]]".to_owned(),
            "bye".to_owned(),
        ];
        assert_eq!(strip(&body), vec!["hello".to_owned(), "bye".to_owned()]);
    }

    #[test]
    fn removing_one_token_leaves_the_others() {
        let body = vec![
            "[[attach: one.png]]".to_owned(),
            "[[attach: two.png]]".to_owned(),
        ];
        assert_eq!(
            remove(&body, Path::new("one.png")),
            vec!["[[attach: two.png]]".to_owned()]
        );
    }
}
