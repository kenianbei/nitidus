//! Shared mutation plumbing: save-before-upsert persistence, selection
//! lookups, and the component-walking prompt chain used by both the
//! editors and the add flow.

use bevy::prelude::*;
use nitidus_contacts::calcard::vcard::{VCardEntry, VCardValue};
use nitidus_contacts::{Contact, ContactError, escape_component};

use super::view::ContactsView;
use super::{ContactStore, ContactsDir, detail};
use crate::prompt::{PromptRequest, open_prompt};
use crate::status::StatusMessage;

pub(super) enum ChainFinish {
    Edit { entry_index: usize },
    Add { prefix: String },
}

/// One prompt per component; Enter keeps the prefilled value. The last
/// component finishes the chain into an edit or an add.
pub(super) fn component_chain_step(
    world: &mut World,
    labels: &'static [&'static str],
    current: Vec<String>,
    collected: Vec<String>,
    finish: ChainFinish,
) {
    let step = collected.len();
    let Some(label) = labels.get(step) else {
        return finish_chain(world, collected, finish);
    };
    let prefill = current.get(step).cloned().unwrap_or_default();
    let request = PromptRequest::new(
        format!("{label}: "),
        Box::new(move |world: &mut World, input: String| {
            let mut collected = collected;
            collected.push(input);
            component_chain_step(world, labels, current, collected, finish);
        }),
    )
    .with_initial(prefill);
    open_prompt(world, request);
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
    world.resource_mut::<StatusMessage>().warn(text, now);
}

pub(super) fn info(world: &mut World, text: String) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<StatusMessage>().info(text, now);
}
