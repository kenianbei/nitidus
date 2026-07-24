//! MessageView parsing over hand-built MIME fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use nitidus_mail::message::{MessageView, PartKind, part_bytes};

fn multipart_fixture() -> Vec<u8> {
    concat!(
        "From: Alice <alice@x.com>\r\n",
        "To: Bob <bob@x.com>\r\n",
        "Subject: mixed message\r\n",
        "Date: Thu, 15 Feb 2024 12:00:00 +0000\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
        "\r\n",
        "--outer\r\n",
        "Content-Type: multipart/alternative; boundary=\"inner\"\r\n",
        "\r\n",
        "--inner\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "plain body here\r\n",
        "--inner\r\n",
        "Content-Type: text/html; charset=utf-8\r\n",
        "\r\n",
        "<p>html body here</p>\r\n",
        "--inner--\r\n",
        "--outer\r\n",
        "Content-Type: application/pdf\r\n",
        "Content-Disposition: attachment; filename=\"report.pdf\"\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "\r\n",
        "JVBERi0xLjQ=\r\n",
        "--outer--\r\n",
    )
    .as_bytes()
    .to_vec()
}

#[test]
fn multipart_alternative_prefers_plain_text() {
    let view = MessageView::parse(&multipart_fixture());
    let default = view.default_part().unwrap();
    assert_eq!(view.parts[default].kind, PartKind::Text);
    assert_eq!(view.parts[default].text.as_deref(), Some("plain body here"));
    assert_eq!(view.body_part_indices().len(), 2, "plain and html");
}

#[test]
fn attachments_list_with_names_and_decoded_sizes() {
    let view = MessageView::parse(&multipart_fixture());
    let attachments = view.attachment_indices();
    assert_eq!(attachments.len(), 1);
    let attachment = &view.parts[attachments[0]];
    assert_eq!(attachment.filename.as_deref(), Some("report.pdf"));
    assert_eq!(attachment.mime, "application/pdf");
    assert_eq!(attachment.size, 8, "decoded, not base64 length");
}

#[test]
fn headers_keep_original_order() {
    let view = MessageView::parse(&multipart_fixture());
    let names: Vec<&str> = view.headers.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names[..4],
        ["From", "To", "Subject", "Date"],
        "{names:?}"
    );
    assert_eq!(view.headers[2].1, "mixed message");
}

#[test]
fn part_bytes_returns_decoded_contents() {
    let raw = multipart_fixture();
    let view = MessageView::parse(&raw);
    let attachment = &view.parts[view.attachment_indices()[0]];
    let bytes = part_bytes(&raw, attachment.source_index).unwrap();
    assert_eq!(bytes, b"%PDF-1.4");
}

#[test]
fn html_only_message_falls_back_to_html_part() {
    let raw = concat!(
        "From: A <a@x.com>\r\n",
        "Subject: html only\r\n",
        "Content-Type: text/html\r\n",
        "\r\n",
        "<b>bold</b> body\r\n",
    )
    .as_bytes()
    .to_vec();
    let view = MessageView::parse(&raw);
    let default = view.default_part().unwrap();
    assert_eq!(view.parts[default].kind, PartKind::Html);
    assert!(view.parts[default].text.as_deref().unwrap().contains("<b>bold</b>"));
}

#[test]
fn garbage_does_not_panic_and_empty_input_is_empty() {
    let _parsed_without_panic = MessageView::parse(&[0xff, 0xfe, 0x00]);
    let empty = MessageView::parse(b"");
    assert!(empty.headers.is_empty());
    assert_eq!(empty.default_part(), None);
}
