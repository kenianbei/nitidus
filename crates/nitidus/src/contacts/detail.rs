//! Detail-pane rows for one contact: identity first, communication and
//! organization next, everything unmodeled last (read-only until the
//! raw editor). Row order within a group follows the card.

use nitidus_contacts::Contact;
use nitidus_contacts::calcard::common::IanaType;
use nitidus_contacts::calcard::vcard::{VCardEntry, VCardParameterName, VCardProperty, VCardValue};

/// One selectable detail row, anchored to its raw entry index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DetailRow {
    pub entry_index: usize,
    pub label: String,
    pub value: String,
    /// Has a first-class editor (phase 4); unmodeled rows only take the
    /// raw property editor.
    pub modeled: bool,
}

pub(super) fn build_rows(contact: &Contact) -> Vec<DetailRow> {
    let mut indices = contact.entry_indices();
    indices.sort_by_key(|&index| contact.entry_at(index).map_or(u8::MAX, |e| rank(&e.name)));
    indices
        .iter()
        .filter_map(|&index| {
            let entry = contact.entry_at(index)?;
            Some(DetailRow {
                entry_index: index,
                label: label(entry),
                value: display_value(entry),
                modeled: is_modeled(&entry.name),
            })
        })
        .collect()
}

/// Display groups: identity, communication, organization, dates, notes,
/// then everything else with UID last.
fn rank(name: &VCardProperty) -> u8 {
    match name {
        VCardProperty::Fn | VCardProperty::N | VCardProperty::Nickname => 0,
        VCardProperty::Email | VCardProperty::Tel => 1,
        VCardProperty::Adr | VCardProperty::Url | VCardProperty::Impp => 2,
        VCardProperty::Org | VCardProperty::Title | VCardProperty::Role => 3,
        VCardProperty::Bday | VCardProperty::Anniversary => 4,
        VCardProperty::Note | VCardProperty::Categories => 5,
        VCardProperty::Photo => 6,
        VCardProperty::Uid => 8,
        _ => 7,
    }
}

pub(super) fn is_modeled(name: &VCardProperty) -> bool {
    matches!(
        name,
        VCardProperty::Fn
            | VCardProperty::N
            | VCardProperty::Email
            | VCardProperty::Tel
            | VCardProperty::Adr
            | VCardProperty::Org
            | VCardProperty::Title
            | VCardProperty::Bday
            | VCardProperty::Url
            | VCardProperty::Note
    )
}

/// `EMAIL (work)`, `TEL (home, cell)`, `X-CUSTOM`.
fn label(entry: &VCardEntry) -> String {
    let name = match &entry.name {
        VCardProperty::Other(other) => other.as_str(),
        known => known.as_str(),
    };
    let types: Vec<String> = entry
        .params
        .iter()
        .filter(|param| param.name == VCardParameterName::Type)
        .filter_map(|param| param.value.as_type())
        .map(|kind| match kind {
            IanaType::Iana(known) => {
                use nitidus_contacts::calcard::common::IanaString;
                known.as_str().to_lowercase()
            }
            IanaType::Other(other) => other.to_lowercase(),
        })
        .collect();
    if types.is_empty() {
        name.to_owned()
    } else {
        format!("{name} ({})", types.join(", "))
    }
}

fn display_value(entry: &VCardEntry) -> String {
    let parts: Vec<String> = entry.values.iter().filter_map(format_value).collect();
    parts.join(", ")
}

fn format_value(value: &VCardValue) -> Option<String> {
    match value {
        VCardValue::Text(text) => Some(text.clone()),
        VCardValue::Component(components) => {
            let joined: Vec<&str> = components
                .iter()
                .map(String::as_str)
                .filter(|component| !component.is_empty())
                .collect();
            (!joined.is_empty()).then(|| joined.join(", "))
        }
        VCardValue::Integer(number) => Some(number.to_string()),
        VCardValue::Float(number) => Some(number.to_string()),
        VCardValue::Boolean(flag) => Some(flag.to_string()),
        VCardValue::PartialDateTime(date) => Some(
            date.to_rfc3339()
                .unwrap_or_else(|| format_partial_date(date)),
        ),
        VCardValue::Binary(data) => Some(format!(
            "[{} bytes{}]",
            data.data.len(),
            data.content_type
                .as_deref()
                .map(|kind| format!(", {kind}"))
                .unwrap_or_default()
        )),
        VCardValue::Sex(sex) => Some(format!("{sex:?}")),
        VCardValue::GramGender(gender) => Some(format!("{gender:?}")),
        VCardValue::Kind(kind) => Some(format!("{kind:?}")),
    }
}

/// Dates like `BDAY:--0401` have no full timestamp; show what exists.
fn format_partial_date(date: &nitidus_contacts::calcard::common::PartialDateTime) -> String {
    let year = date.year.map_or("????".to_owned(), |y| y.to_string());
    let month = date.month.map_or("??".to_owned(), |m| format!("{m:02}"));
    let day = date.day.map_or("??".to_owned(), |d| format!("{d:02}"));
    format!("{year}-{month}-{day}")
}
