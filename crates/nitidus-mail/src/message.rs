//! Pure MIME view of a fetched message: ordered headers, flattened
//! leaf parts, and decoded text — no rendering, wrapping, or IO.
//! Weeding, styling, and width are UI concerns.

use mail_parser::{MessageParser, MimeHeaders, PartType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Text,
    Html,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartView {
    pub kind: PartKind,
    pub mime: String,
    pub filename: Option<String>,
    /// Decoded text for `Text`/`Html` parts; `None` for binary parts.
    pub text: Option<String>,
    pub size: usize,
    pub is_attachment: bool,
    /// RFC 3676 `format=flowed` (with `delsp`) from the content type.
    pub is_flowed: bool,
    pub delete_space: bool,
    /// Index into the parsed message's part list, for `part_bytes`.
    pub source_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageView {
    /// Original order, raw values — the full-headers display.
    pub headers: Vec<(String, String)>,
    pub parts: Vec<PartView>,
}

impl MessageView {
    /// Unparseable input yields an empty view, never an error — the
    /// pager shows what exists.
    pub fn parse(raw: &[u8]) -> MessageView {
        let Some(message) = MessageParser::default().parse(raw) else {
            return MessageView::default();
        };
        let headers = message
            .headers_raw()
            .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
            .collect();
        let body_ids: Vec<usize> = message
            .text_body
            .iter()
            .chain(message.html_body.iter())
            .map(|&id| id as usize)
            .collect();
        let parts = message
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| !matches!(part.body, PartType::Multipart(_)))
            .filter(|(index, part)| {
                body_ids.contains(index)
                    || part.attachment_name().is_some()
                    || !part.contents().is_empty()
            })
            .map(|(index, part)| build_part(index, part, &body_ids))
            .collect();
        MessageView { headers, parts }
    }

    /// First plain-text body part, else first HTML one.
    pub fn default_part(&self) -> Option<usize> {
        self.parts
            .iter()
            .position(|part| part.kind == PartKind::Text && !part.is_attachment)
            .or_else(|| {
                self.parts
                    .iter()
                    .position(|part| part.kind == PartKind::Html && !part.is_attachment)
            })
    }

    pub fn body_part_indices(&self) -> Vec<usize> {
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, part)| !part.is_attachment && part.text.is_some())
            .map(|(index, _)| index)
            .collect()
    }

    pub fn attachment_indices(&self) -> Vec<usize> {
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.is_attachment)
            .map(|(index, _)| index)
            .collect()
    }
}

/// Decoded bytes of one source part, re-parsed on demand (save/open are
/// rare; keeping every part's bytes resident is not worth it).
pub fn part_bytes(raw: &[u8], source_index: usize) -> Option<Vec<u8>> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.parts.get(source_index)?;
    Some(part.contents().to_vec())
}

fn build_part(index: usize, part: &mail_parser::MessagePart, body_ids: &[usize]) -> PartView {
    let kind = match &part.body {
        PartType::Text(_) => PartKind::Text,
        PartType::Html(_) => PartKind::Html,
        _ => PartKind::Other,
    };
    let mime = part
        .content_type()
        .map(|content_type| match content_type.subtype() {
            Some(subtype) => format!("{}/{subtype}", content_type.ctype()),
            None => content_type.ctype().to_owned(),
        })
        .unwrap_or_else(|| default_mime(kind).to_owned());
    let is_attachment = !body_ids.contains(&index);
    let attribute = |name: &str| {
        part.content_type()
            .and_then(|content_type| content_type.attribute(name))
            .map(str::to_ascii_lowercase)
    };
    PartView {
        kind,
        mime,
        filename: part.attachment_name().map(str::to_owned),
        text: part.text_contents().map(str::to_owned),
        size: part.contents().len(),
        is_attachment,
        is_flowed: attribute("format").as_deref() == Some("flowed"),
        delete_space: attribute("delsp").as_deref() == Some("yes"),
        source_index: index,
    }
}

fn default_mime(kind: PartKind) -> &'static str {
    match kind {
        PartKind::Text => "text/plain",
        PartKind::Html => "text/html",
        PartKind::Other => "application/octet-stream",
    }
}
