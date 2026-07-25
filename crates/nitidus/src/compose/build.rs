//! `ComposeSession` → RFC 5322 bytes + SMTP envelope. Bcc recipients
//! ride the envelope only — the transmitted headers never name them.

use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;

use super::ComposeSession;
use nitidus_mail::send::SendEnvelope;

pub struct BuiltMessage {
    pub bytes: Vec<u8>,
    pub envelope: SendEnvelope,
}

pub fn build(session: &ComposeSession) -> anyhow::Result<BuiltMessage> {
    let to = parse_address_list(&session.to);
    if to.is_empty() {
        anyhow::bail!("To has no valid recipients");
    }
    let cc = parse_address_list(&session.cc);
    let bcc = parse_address_list(&session.bcc);
    let (from_name, from_addr) = split_display(&session.from)
        .ok_or_else(|| anyhow::anyhow!("account has no usable from address"))?;

    let body = std::fs::read_to_string(&session.body_path)?;
    let mut builder = MessageBuilder::new()
        .message_id(generate_message_id(&from_addr))
        .from(address(from_name.clone(), from_addr.clone()))
        .to(address_list(&to))
        .subject(session.subject.clone())
        .text_body(body);
    if !cc.is_empty() {
        builder = builder.cc(address_list(&cc));
    }
    if let Some(in_reply_to) = &session.in_reply_to {
        builder = builder.in_reply_to(in_reply_to.clone());
    }
    if !session.references.is_empty() {
        builder = builder.references(session.references.clone());
    }
    let bytes = builder.write_to_vec()?;

    let recipients = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .map(|(_, addr)| addr.clone())
        .collect();
    Ok(BuiltMessage {
        bytes,
        envelope: SendEnvelope {
            from: from_addr,
            recipients,
        },
    })
}

/// Comma-separated `addr` / `Name <addr>` items; entries without an
/// `@` are dropped (full validation is the 1e.23 item).
pub(crate) fn parse_address_list(input: &str) -> Vec<(Option<String>, String)> {
    input
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let (name, addr) = match split_display(item) {
                Some(parsed) => parsed,
                None => (None, item.to_owned()),
            };
            addr.contains('@').then_some((name, addr))
        })
        .collect()
}

/// `Name <addr>` → `(Some(name), addr)`; bare `addr` → `(None, addr)`.
fn split_display(item: &str) -> Option<(Option<String>, String)> {
    let item = item.trim();
    if let Some((name, rest)) = item.split_once('<') {
        let addr = rest.strip_suffix('>')?.trim();
        let name = name.trim().trim_matches('"');
        return Some(((!name.is_empty()).then(|| name.to_owned()), addr.to_owned()));
    }
    item.contains('@').then(|| (None, item.to_owned()))
}

fn address(name: Option<String>, addr: String) -> Address<'static> {
    match name {
        Some(name) => Address::new_address(Some(name), addr),
        None => Address::new_address(None::<String>, addr),
    }
}

fn address_list(items: &[(Option<String>, String)]) -> Address<'static> {
    Address::new_list(
        items
            .iter()
            .map(|(name, addr)| address(name.clone(), addr.clone()))
            .collect(),
    )
}

fn generate_message_id(from_addr: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let domain = from_addr.rsplit_once('@').map_or("nitidus", |(_, d)| d);
    format!("{stamp}.{}@{domain}", std::process::id())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::super::ComposeStage;
    use super::*;
    use nitidus_mail::AccountId;

    fn session(dir: &std::path::Path) -> ComposeSession {
        let body_path = dir.join("body.md");
        std::fs::write(&body_path, "hello there\n").unwrap();
        ComposeSession {
            account: AccountId::new("local"),
            from: "Norman <norman@example.com>".to_owned(),
            to: "bob@example.com, Carol <carol@example.com>".to_owned(),
            cc: "dave@example.com".to_owned(),
            bcc: "secret@example.com".to_owned(),
            subject: "greetings".to_owned(),
            body_path,
            body: Vec::new(),
            stage: ComposeStage::Review,
            in_reply_to: None,
            references: Vec::new(),
            reply_source: None,
        }
    }

    #[test]
    fn builds_headers_with_bcc_in_envelope_only() {
        let dir = tempfile::tempdir().unwrap();
        let built = build(&session(dir.path())).unwrap();
        let text = String::from_utf8_lossy(&built.bytes);
        assert!(
            text.contains("From: \"Norman\" <norman@example.com>"),
            "{text}"
        );
        assert!(text.contains("carol@example.com"), "{text}");
        assert!(text.contains("Subject: greetings"), "{text}");
        assert!(text.contains("Message-ID:"), "{text}");
        assert!(text.contains("hello there"), "{text}");
        assert!(
            !text.contains("secret@example.com"),
            "bcc must never appear in headers: {text}"
        );
        assert_eq!(
            built.envelope.recipients,
            vec![
                "bob@example.com".to_owned(),
                "carol@example.com".to_owned(),
                "dave@example.com".to_owned(),
                "secret@example.com".to_owned(),
            ]
        );
        assert_eq!(built.envelope.from, "norman@example.com");
    }

    #[test]
    fn empty_or_invalid_to_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut bad = session(dir.path());
        bad.to = "not-an-address".to_owned();
        assert!(build(&bad).is_err());
        bad.to = String::new();
        assert!(build(&bad).is_err());
    }

    #[test]
    fn address_list_parsing_handles_forms_and_noise() {
        let parsed = parse_address_list(" a@x.com ,, Bob <b@y.org>, junk , \"Q\" <q@z.io>");
        assert_eq!(
            parsed,
            vec![
                (None, "a@x.com".to_owned()),
                (Some("Bob".to_owned()), "b@y.org".to_owned()),
                (Some("Q".to_owned()), "q@z.io".to_owned()),
            ]
        );
    }
}
