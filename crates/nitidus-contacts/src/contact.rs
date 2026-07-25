//! One contact = one vCard. Mutations go through single-line vCard
//! syntax re-parsed by calcard, so every edit is validated the same way
//! a loaded file is — and entries the UI never touches survive
//! byte-for-byte in structure and order.

use calcard::vcard::{VCard, VCardEntry, VCardProperty, VCardValue, VCardVersion};
use calcard::{Entry, Parser};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContactError {
    #[error("not a vCard: {0}")]
    NotAVCard(String),
    #[error("invalid property line: {0}")]
    InvalidProperty(String),
    #[error("the UID property cannot be edited")]
    UidImmutable,
    #[error("no property at index {0}")]
    NoSuchProperty(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoSource<'a> {
    Bytes(&'a [u8]),
    Uri(&'a str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    card: VCard,
    /// File stem the contact was loaded from when it differs from the
    /// UID — saves must overwrite the original file, never orphan it.
    pub(crate) source_stem: Option<String>,
}

impl Contact {
    pub fn new(display_name: &str) -> Self {
        let uid = uuid::Uuid::new_v4().to_string();
        let card = VCard {
            entries: vec![
                VCardEntry::new(VCardProperty::Uid).with_value(VCardValue::Text(uid)),
                VCardEntry::new(VCardProperty::Fn)
                    .with_value(VCardValue::Text(display_name.to_owned())),
            ],
        };
        Self {
            card,
            source_stem: None,
        }
    }

    pub fn from_vcf(input: &str) -> Result<Self, ContactError> {
        let entry = Parser::new(input).entry();
        let Entry::VCard(mut card) = entry else {
            return Err(ContactError::NotAVCard(format!("{entry:?}")));
        };
        if card.uid().is_none() {
            let uid = uuid::Uuid::new_v4().to_string();
            card.entries
                .push(VCardEntry::new(VCardProperty::Uid).with_value(VCardValue::Text(uid)));
        }
        Ok(Self {
            card,
            source_stem: None,
        })
    }

    pub fn to_vcf(&self) -> String {
        let mut out = String::new();
        // Infallible: writing into a String cannot fail.
        let _ = self.card.write_to(&mut out, VCardVersion::V4_0);
        out
    }

    pub fn uid(&self) -> &str {
        self.card.uid().unwrap_or_default()
    }

    pub fn display_name(&self) -> &str {
        self.property_text(&VCardProperty::Fn).unwrap_or_default()
    }

    pub fn primary_email(&self) -> Option<&str> {
        self.property_text(&VCardProperty::Email)
    }

    pub fn primary_phone(&self) -> Option<&str> {
        self.property_text(&VCardProperty::Tel)
    }

    pub fn organization(&self) -> Option<&str> {
        self.card.property(&VCardProperty::Org).and_then(first_text)
    }

    fn property_text(&self, property: &VCardProperty) -> Option<&str> {
        self.card.property(property).and_then(first_text)
    }

    pub fn sort_key(&self) -> (String, String) {
        (self.display_name().to_lowercase(), self.uid().to_owned())
    }

    /// The first PHOTO: inline bytes (vCard BINARY / data: URI) or a
    /// URI left for the caller to resolve.
    pub fn photo(&self) -> Option<PhotoSource<'_>> {
        let entry = self.card.property(&VCardProperty::Photo)?;
        match entry.values.first()? {
            VCardValue::Binary(data) => Some(PhotoSource::Bytes(&data.data)),
            VCardValue::Text(uri) => Some(PhotoSource::Uri(uri)),
            _ => None,
        }
    }

    /// Indices into the raw entry list; `Begin`/`End`/`Version` are
    /// bookkeeping the writer regenerates and are skipped.
    pub fn entry_indices(&self) -> Vec<usize> {
        self.card
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !is_bookkeeping(&entry.name))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn entry_at(&self, index: usize) -> Option<&VCardEntry> {
        self.card.entries.get(index)
    }

    /// The entry as one unfolded vCard line, e.g.
    /// `EMAIL;TYPE=work:nk@example.com`.
    pub fn entry_line(&self, index: usize) -> Option<String> {
        let entry = self.card.entries.get(index)?;
        let mut out = String::new();
        let _ = entry.write_to(&mut out, true);
        Some(unfold(&out))
    }

    /// The value portion of the entry line (after the first colon
    /// outside quoted parameters), in raw vCard syntax.
    pub fn entry_value_text(&self, index: usize) -> Option<String> {
        let line = self.entry_line(index)?;
        Some(line[value_offset(&line)..].to_owned())
    }

    /// Replaces the entry's value, keeping its name and parameters;
    /// `value_text` is raw vCard value syntax and is re-parsed.
    pub fn edit_entry(&mut self, index: usize, value_text: &str) -> Result<(), ContactError> {
        let line = self
            .entry_line(index)
            .ok_or(ContactError::NoSuchProperty(index))?;
        let prefix = &line[..value_offset(&line)];
        self.replace_entry_line(index, &format!("{prefix}{value_text}"))
    }

    /// Replaces the whole entry from a raw vCard line (the `E` editor).
    pub fn replace_entry_line(&mut self, index: usize, line: &str) -> Result<(), ContactError> {
        let current = self
            .card
            .entries
            .get(index)
            .ok_or(ContactError::NoSuchProperty(index))?;
        if current.name == VCardProperty::Uid {
            return Err(ContactError::UidImmutable);
        }
        let parsed = parse_entry_line(line)?;
        self.card.entries[index] = parsed;
        Ok(())
    }

    /// Appends a new property from a raw vCard line.
    pub fn add_entry_line(&mut self, line: &str) -> Result<usize, ContactError> {
        let parsed = parse_entry_line(line)?;
        self.card.entries.push(parsed);
        Ok(self.card.entries.len() - 1)
    }

    pub fn remove_entry(&mut self, index: usize) -> Result<VCardEntry, ContactError> {
        let entry = self
            .card
            .entries
            .get(index)
            .ok_or(ContactError::NoSuchProperty(index))?;
        if entry.name == VCardProperty::Uid {
            return Err(ContactError::UidImmutable);
        }
        Ok(self.card.entries.remove(index))
    }
}

fn is_bookkeeping(name: &VCardProperty) -> bool {
    matches!(
        name,
        VCardProperty::Begin | VCardProperty::End | VCardProperty::Version
    )
}

fn first_text(entry: &VCardEntry) -> Option<&str> {
    entry.values.first().and_then(VCardValue::as_text)
}

fn unfold(serialized: &str) -> String {
    serialized
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .trim_end_matches("\r\n")
        .to_owned()
}

/// Byte offset just past the first colon outside double-quoted
/// parameter values — where the property value starts.
fn value_offset(line: &str) -> usize {
    let mut in_quotes = false;
    for (offset, character) in line.char_indices() {
        match character {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => return offset + 1,
            _ => {}
        }
    }
    line.len()
}

fn parse_entry_line(line: &str) -> Result<VCardEntry, ContactError> {
    if line.trim().is_empty() || line.contains('\n') {
        return Err(ContactError::InvalidProperty(line.to_owned()));
    }
    let wrapped = format!("BEGIN:VCARD\r\nVERSION:4.0\r\n{line}\r\nEND:VCARD\r\n");
    let entry = Parser::new(&wrapped).entry();
    let Entry::VCard(card) = entry else {
        return Err(ContactError::InvalidProperty(line.to_owned()));
    };
    let mut parsed = card
        .entries
        .into_iter()
        .filter(|entry| !is_bookkeeping(&entry.name));
    match (parsed.next(), parsed.next()) {
        (Some(entry), None) if entry.name != VCardProperty::Uid => Ok(entry),
        (Some(entry), None) if entry.name == VCardProperty::Uid => Err(ContactError::UidImmutable),
        _ => Err(ContactError::InvalidProperty(line.to_owned())),
    }
}

/// Escapes one component of a compound value (N/ADR prompts build
/// their value text from these).
pub fn escape_component(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const EXOTIC: &str = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:abc-123\r\nFN:Ada Lovelace\r\nX-CUSTOM;X-PARAM=zig:zag\r\nGENDER:F\r\nEMAIL;TYPE=work:ada@example.com\r\nEND:VCARD\r\n";

    #[test]
    fn new_contact_has_uid_and_display_name() {
        let contact = Contact::new("Norman Kerr");
        assert_eq!(contact.display_name(), "Norman Kerr");
        assert!(!contact.uid().is_empty());
    }

    #[test]
    fn parsing_injects_uid_when_missing() {
        let contact =
            Contact::from_vcf("BEGIN:VCARD\r\nVERSION:4.0\r\nFN:X\r\nEND:VCARD\r\n").unwrap();
        assert!(!contact.uid().is_empty());
    }

    #[test]
    fn non_vcard_input_is_rejected() {
        assert!(Contact::from_vcf("hello world").is_err());
    }

    #[test]
    fn unmodeled_properties_round_trip_through_an_edit() {
        let mut contact = Contact::from_vcf(EXOTIC).unwrap();
        let email_index = *contact
            .entry_indices()
            .iter()
            .find(|&&index| contact.entry_at(index).unwrap().name == VCardProperty::Email)
            .unwrap();
        contact
            .edit_entry(email_index, "countess@example.com")
            .unwrap();
        let written = contact.to_vcf();
        assert!(written.contains("X-CUSTOM;X-PARAM=zig:zag"), "{written}");
        assert!(written.contains("GENDER:F"), "{written}");
        assert!(
            written.contains("EMAIL;TYPE=WORK:countess@example.com"),
            "type parameter must survive a value edit: {written}"
        );
    }

    #[test]
    fn entry_value_text_splits_after_parameters() {
        let contact = Contact::from_vcf(EXOTIC).unwrap();
        let email_index = contact.entry_indices()[4];
        assert_eq!(
            contact.entry_line(email_index).unwrap(),
            "EMAIL;TYPE=WORK:ada@example.com"
        );
        assert_eq!(
            contact.entry_value_text(email_index).unwrap(),
            "ada@example.com"
        );
    }

    #[test]
    fn raw_replace_validates_and_rejects_garbage() {
        let mut contact = Contact::from_vcf(EXOTIC).unwrap();
        let email_index = contact.entry_indices()[4];
        contact
            .replace_entry_line(email_index, "EMAIL;TYPE=home:ada@home.example")
            .unwrap();
        assert_eq!(contact.primary_email(), Some("ada@home.example"));
        assert!(contact.replace_entry_line(email_index, "").is_err());
        assert!(
            contact
                .replace_entry_line(email_index, "two\r\nlines:x")
                .is_err()
        );
    }

    #[test]
    fn uid_cannot_be_edited_or_removed() {
        let mut contact = Contact::from_vcf(EXOTIC).unwrap();
        let uid_index = *contact
            .entry_indices()
            .iter()
            .find(|&&index| contact.entry_at(index).unwrap().name == VCardProperty::Uid)
            .unwrap();
        assert!(matches!(
            contact.replace_entry_line(uid_index, "UID:other"),
            Err(ContactError::UidImmutable)
        ));
        assert!(matches!(
            contact.remove_entry(uid_index),
            Err(ContactError::UidImmutable)
        ));
        let mut fresh = Contact::from_vcf(EXOTIC).unwrap();
        assert!(matches!(
            fresh.add_entry_line("UID:sneaky"),
            Err(ContactError::UidImmutable)
        ));
    }

    #[test]
    fn add_and_remove_properties() {
        let mut contact = Contact::new("N");
        let index = contact.add_entry_line("TEL;TYPE=cell:+1-555-0100").unwrap();
        assert_eq!(contact.primary_phone(), Some("+1-555-0100"));
        contact.remove_entry(index).unwrap();
        assert_eq!(contact.primary_phone(), None);
    }

    #[test]
    fn escape_component_escapes_separators() {
        assert_eq!(escape_component("a;b,c\\d"), "a\\;b\\,c\\\\d");
    }
}
