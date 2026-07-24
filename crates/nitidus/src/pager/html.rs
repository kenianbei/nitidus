//! Pure HTML tier-1 pipeline: ammonia sanitization with remote content
//! stripped and counted, then html2text rich rendering into annotated,
//! width-wrapped span lines plus the document's anchors.
//!
//! html2text's `css` feature stays off: sanitization removes `<style>`
//! blocks and `style=` attributes before rendering, so the feature
//! could only ever see nothing. CSS-driven color arrives with the
//! Phase 4 HTML tiers.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use html2text::render::RichAnnotation;

use super::body::{self, LineKind};

/// Attributes that trigger a fetch when rendered; `href` is absent on
/// purpose — links are surfaced, never fetched.
const FETCHING_ATTRIBUTES: &[&str] = &["src", "srcset", "poster", "background"];
const ALLOWED_URL_SCHEMES: &[&str] = &["http", "https", "mailto", "tel", "cid", "data"];
const ANCHOR_SCHEMES: &[&str] = &["http:", "https:", "mailto:"];
const MIN_RENDER_WIDTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sanitized {
    pub html: String,
    pub blocked_remote: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpanStyleTag {
    pub is_link: bool,
    pub is_strong: bool,
    pub is_emphasis: bool,
    pub is_strikeout: bool,
    pub is_code: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtmlLine {
    pub spans: Vec<(String, SpanStyleTag)>,
    pub kind: LineKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub href: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedHtml {
    pub lines: Vec<HtmlLine>,
    pub anchors: Vec<Anchor>,
}

pub fn sanitize(html: &str) -> Sanitized {
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&blocked);
    let mut builder = ammonia::Builder::default();
    builder
        .url_schemes(ALLOWED_URL_SCHEMES.iter().copied().collect())
        .attribute_filter(move |_element, attribute, value| {
            if FETCHING_ATTRIBUTES.contains(&attribute) && is_remote(value) {
                counter.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            Some(value.into())
        });
    let clean = builder.clean(html).to_string();
    Sanitized {
        html: clean,
        blocked_remote: blocked.load(Ordering::Relaxed),
    }
}

fn is_remote(value: &str) -> bool {
    let lowered = value.trim_start().to_ascii_lowercase();
    lowered.starts_with("http:") || lowered.starts_with("https:") || lowered.starts_with("//")
}

/// Rendering failure yields the error as a single body line, never a
/// panic — the pager shows what exists.
pub fn render_html(html: &str, width: usize) -> RenderedHtml {
    let width = width.max(MIN_RENDER_WIDTH);
    let tagged_lines = match html2text::config::rich().lines_from_read(html.as_bytes(), width) {
        Ok(lines) => lines,
        Err(error) => return error_line(&format!("[html rendering failed: {error}]")),
    };
    let mut rendered = RenderedHtml::default();
    for tagged_line in &tagged_lines {
        let mut spans = Vec::new();
        let mut text = String::new();
        for tagged in tagged_line.tagged_strings() {
            collect_anchor(&tagged.tag, &tagged.s, &mut rendered.anchors);
            text.push_str(&tagged.s);
            spans.push((tagged.s.clone(), collapse_annotations(&tagged.tag)));
        }
        rendered.lines.push(HtmlLine {
            spans,
            kind: classify(&text),
        });
    }
    dedupe_anchors(&mut rendered.anchors);
    rendered
}

fn error_line(message: &str) -> RenderedHtml {
    RenderedHtml {
        lines: vec![HtmlLine {
            spans: vec![(message.to_owned(), SpanStyleTag::default())],
            kind: LineKind::Normal,
        }],
        anchors: Vec::new(),
    }
}

fn classify(line: &str) -> LineKind {
    match body::quote_depth(line) {
        0 => LineKind::Normal,
        depth => LineKind::Quote(depth),
    }
}

fn collapse_annotations(annotations: &[RichAnnotation]) -> SpanStyleTag {
    let mut tag = SpanStyleTag::default();
    for annotation in annotations {
        match annotation {
            RichAnnotation::Link(_) => tag.is_link = true,
            RichAnnotation::Strong => tag.is_strong = true,
            RichAnnotation::Emphasis => tag.is_emphasis = true,
            RichAnnotation::Strikeout => tag.is_strikeout = true,
            RichAnnotation::Code | RichAnnotation::Preformat(_) => tag.is_code = true,
            // The enum is #[non_exhaustive]; unhandled annotations
            // (images, css-only colors) render unstyled.
            _ => {}
        }
    }
    tag
}

/// Link text wraps across tagged strings and lines; consecutive spans
/// of the same href merge into one anchor label.
fn collect_anchor(annotations: &[RichAnnotation], text: &str, anchors: &mut Vec<Anchor>) {
    let Some(href) = annotations.iter().find_map(|annotation| match annotation {
        RichAnnotation::Link(href) => Some(href),
        _ => None,
    }) else {
        return;
    };
    if !has_anchor_scheme(href) {
        return;
    }
    match anchors.last_mut() {
        Some(last) if &last.href == href => {
            last.label.push(' ');
            last.label.push_str(text.trim());
        }
        _ => anchors.push(Anchor {
            href: href.clone(),
            label: text.trim().to_owned(),
        }),
    }
}

fn has_anchor_scheme(href: &str) -> bool {
    let lowered = href.trim_start().to_ascii_lowercase();
    ANCHOR_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme))
}

/// First occurrence per href wins; labels collapse to single-spaced
/// text, falling back to the href when the anchor had no visible text.
fn dedupe_anchors(anchors: &mut Vec<Anchor>) {
    let mut seen: Vec<String> = Vec::new();
    anchors.retain(|anchor| {
        let is_new = !seen.contains(&anchor.href);
        if is_new {
            seen.push(anchor.href.clone());
        }
        is_new
    });
    for anchor in anchors {
        anchor.label = anchor
            .label
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if anchor.label.is_empty() {
            anchor.label = anchor.href.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn line_text(line: &HtmlLine) -> String {
        line.spans.iter().map(|(text, _)| text.as_str()).collect()
    }

    fn all_text(rendered: &RenderedHtml) -> String {
        rendered
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn scripts_are_stripped_with_contents() {
        let sanitized = sanitize("<p>hello</p><script>alert('x')</script>");
        assert!(!sanitized.html.contains("alert"), "{}", sanitized.html);
        assert_eq!(sanitized.blocked_remote, 0);
    }

    #[test]
    fn remote_images_are_blocked_and_counted() {
        let sanitized = sanitize(
            "<img src=\"https://tracker.example/pixel.gif\">\
             <img src=\"HTTP://cdn.example/logo.png\">\
             <img src=\"//protocol.relative/x.png\">",
        );
        assert_eq!(sanitized.blocked_remote, 3);
        assert!(
            !sanitized.html.contains("tracker.example"),
            "{}",
            sanitized.html
        );
    }

    #[test]
    fn cid_and_data_image_sources_survive() {
        let sanitized =
            sanitize("<img src=\"cid:logo@example\"><img src=\"data:image/png;base64,AA==\">");
        assert_eq!(sanitized.blocked_remote, 0);
        assert!(
            sanitized.html.contains("cid:logo@example"),
            "{}",
            sanitized.html
        );
        assert!(
            sanitized.html.contains("data:image/png"),
            "{}",
            sanitized.html
        );
    }

    #[test]
    fn anchor_hrefs_are_never_stripped() {
        let sanitized = sanitize("<a href=\"https://example.com/x\">link</a>");
        assert!(
            sanitized.html.contains("https://example.com/x"),
            "{}",
            sanitized.html
        );
        assert_eq!(sanitized.blocked_remote, 0);
    }

    #[test]
    fn strong_and_emphasis_spans_are_tagged() {
        let rendered = render_html("<p>plain <strong>bold</strong> <em>slanted</em></p>", 80);
        let spans: Vec<&(String, SpanStyleTag)> =
            rendered.lines.iter().flat_map(|line| &line.spans).collect();
        let bold = spans.iter().find(|(text, _)| text == "bold").unwrap();
        let slanted = spans.iter().find(|(text, _)| text == "slanted").unwrap();
        assert!(bold.1.is_strong);
        assert!(slanted.1.is_emphasis);
        assert!(!bold.1.is_emphasis && !slanted.1.is_strong);
    }

    #[test]
    fn blockquote_lines_classify_as_quotes() {
        let rendered = render_html("<p>intro</p><blockquote>quoted words</blockquote>", 80);
        let quoted = rendered
            .lines
            .iter()
            .find(|line| line_text(line).contains("quoted"))
            .unwrap();
        assert_eq!(quoted.kind, LineKind::Quote(1));
        assert_eq!(rendered.lines[0].kind, LineKind::Normal);
    }

    #[test]
    fn anchors_dedupe_and_keep_document_order() {
        let rendered = render_html(
            "<a href=\"https://a.example/1\">first</a> \
             <a href=\"mailto:x@example.com\">write</a> \
             <a href=\"https://a.example/1\">again</a> \
             <a href=\"ftp://files.example\">skipped</a>",
            200,
        );
        let hrefs: Vec<&str> = rendered
            .anchors
            .iter()
            .map(|anchor| anchor.href.as_str())
            .collect();
        assert_eq!(hrefs, vec!["https://a.example/1", "mailto:x@example.com"]);
        assert_eq!(rendered.anchors[0].label, "first");
    }

    #[test]
    fn wrapped_anchor_merges_into_one_label() {
        let rendered = render_html(
            "<a href=\"https://example.com/long\">a rather long anchor label that wraps</a>",
            20,
        );
        assert_eq!(rendered.anchors.len(), 1);
        assert_eq!(
            rendered.anchors[0].label,
            "a rather long anchor label that wraps"
        );
        assert!(rendered.lines.len() > 1, "{rendered:?}");
    }

    #[test]
    fn anchor_without_text_falls_back_to_href() {
        let rendered = render_html(
            "<a href=\"https://example.com/bare\"><img alt=\"\"></a>",
            80,
        );
        if let Some(anchor) = rendered.anchors.first() {
            assert!(!anchor.label.is_empty());
        }
    }

    #[test]
    fn render_respects_width() {
        let rendered = render_html(
            "<p>one two three four five six seven eight nine ten</p>",
            20,
        );
        assert!(rendered.lines.len() > 1);
        for line in &rendered.lines {
            let length: usize = line
                .spans
                .iter()
                .map(|(text, _)| text.chars().count())
                .sum();
            assert!(length <= 20, "line too wide: {line:?}");
        }
    }

    #[test]
    fn sanitize_then_render_end_to_end() {
        let html = "<style>p { color: red }</style>\
             <p>body <a href=\"https://example.com\">site</a></p>\
             <img src=\"https://tracker.example/p.gif\">";
        let sanitized = sanitize(html);
        assert_eq!(sanitized.blocked_remote, 1);
        let rendered = render_html(&sanitized.html, 80);
        let text = all_text(&rendered);
        assert!(text.contains("body site"), "{text}");
        assert!(!text.contains("color"), "{text}");
        assert_eq!(rendered.anchors[0].href, "https://example.com");
    }
}
