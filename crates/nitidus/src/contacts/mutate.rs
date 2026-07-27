//! Shared mutation plumbing: save-before-upsert persistence, selection
//! lookups, and the component form used by both the editors and the add
//! flow.

use bevy::prelude::*;
use nitidus_contacts::calcard::vcard::{VCardEntry, VCardValue};
use nitidus_contacts::{Contact, ContactError, escape_component};

use super::view::ContactsView;
use super::{ContactStore, ContactsDir, detail};
use crate::overlay::form::{FieldSpec, FormSpec, open_form};
use crate::status::MessageLog;

pub(super) enum ChainFinish {
    Edit { entry_index: usize },
    Add { prefix: String },
}

/// One field per component of a structured property — a name's family
/// and given parts, an address's street and city — on one surface. They
/// used to be one prompt each, so you could not see what you had
/// already entered or go back to fix it.
pub(super) fn open_component_form(
    world: &mut World,
    title: &str,
    labels: &'static [&'static str],
    current: Vec<String>,
    finish: ChainFinish,
) {
    let fields = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            FieldSpec::text(label, *label)
                .with_initial(current.get(index).cloned().unwrap_or_default())
        })
        .collect();
    open_form(
        world,
        FormSpec::new(
            title.to_owned(),
            "Save",
            fields,
            Box::new(move |world: &mut World, values| {
                let collected = labels
                    .iter()
                    .map(|label| values.get(label).to_owned())
                    .collect();
                finish_chain(world, collected, finish);
            }),
        ),
    );
}

fn finish_chain(world: &mut World, collected: Vec<String>, finish: ChainFinish) {
    let value_text = collected
        .iter()
        .map(|component| escape_component(component))
        .collect::<Vec<_>>()
        .join(";");
    match finish {
        ChainFinish::Edit { entry_index } => {
            apply_mutation(world, move |contact| {
                contact.edit_entry(entry_index, &value_text)
            });
        }
        ChainFinish::Add { prefix } => {
            if collected
                .iter()
                .all(|component| component.trim().is_empty())
            {
                return;
            }
            let line = format!("{prefix}{value_text}");
            apply_mutation(world, move |contact| {
                contact.add_entry_line(&line).map(|_| ())
            });
        }
    }
}

/// Save first, then update the book — on a failed write the UI keeps
/// showing what the disk still holds.
pub(super) fn persist_and_upsert(world: &mut World, contact: Contact) {
    let Some(dir) = contacts_dir(world) else {
        return;
    };
    if let Err(error) = nitidus_contacts::save_contact(&dir, &contact) {
        return warn(world, format!("save failed: {error}"));
    }
    let name = contact.display_name().to_owned();
    let position = world.resource_mut::<ContactStore>().0.upsert(contact);
    world.resource_mut::<ContactsView>().selected = position;
    info(world, format!("saved {name}"));
}

pub(super) fn apply_mutation(
    world: &mut World,
    mutate: impl FnOnce(&mut Contact) -> Result<(), ContactError>,
) {
    let Some(mut contact) = selected_contact(world) else {
        return;
    };
    if let Err(error) = mutate(&mut contact) {
        return warn(world, error.to_string());
    }
    persist_and_upsert(world, contact);
}

pub(super) fn selected_contact(world: &World) -> Option<Contact> {
    let view = world.resource::<ContactsView>();
    world
        .resource::<ContactStore>()
        .0
        .get(view.selected)
        .cloned()
}

pub(super) fn selected_entry(world: &World) -> Option<(usize, VCardEntry)> {
    let view = world.resource::<ContactsView>();
    let contact = world.resource::<ContactStore>().0.get(view.selected)?;
    let rows = detail::build_rows(contact);
    let row = rows.get(view.detail_selected)?;
    contact
        .entry_at(row.entry_index)
        .map(|entry| (row.entry_index, entry.clone()))
}

/// calcard represents a compound value (N/ADR) as one `Text` value per
/// component; `Component` only appears for a comma-list inside one
/// component. Flatten both so every prompt prefills its real value.
pub(super) fn components_of(entry: &VCardEntry) -> Vec<String> {
    entry
        .values
        .iter()
        .map(|value| match value {
            VCardValue::Text(text) => text.clone(),
            VCardValue::Component(parts) => parts.join(","),
            other => other.as_text().unwrap_or_default().to_owned(),
        })
        .collect()
}

pub(super) fn contacts_dir(world: &mut World) -> Option<std::path::PathBuf> {
    let dir = world.get_resource::<ContactsDir>().map(|dir| dir.0.clone());
    if dir.is_none() {
        warn(world, "contacts directory unavailable".to_owned());
    }
    dir
}

pub(super) fn warn(world: &mut World, text: String) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<MessageLog>().warn(text, now);
}

pub(super) fn info(world: &mut World, text: String) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<MessageLog>().info(text, now);
}
