//! Adding and removing whole records: `a` (property-kind picker → TYPE
//! picker → value), `n` (new contact), `D` (delete contact behind a
//! confirm).

use bevy::prelude::*;
use nitidus_contacts::{Contact, escape_component};

use super::ContactStore;
use super::edit::ADR_COMPONENTS;
use super::mutate::{
    ChainFinish, apply_mutation, component_chain_step, contacts_dir, info, persist_and_upsert,
    selected_contact, warn,
};
use super::view::ContactsView;
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::prompt::{PromptRequest, open_prompt};

/// Add-picker rows: label, property name (empty = raw line), offers a
/// TYPE picker, compound (component-walking) value.
const ADD_KINDS: &[(&str, &str, bool, bool)] = &[
    ("email", "EMAIL", true, false),
    ("phone", "TEL", true, false),
    ("address", "ADR", true, true),
    ("url", "URL", false, false),
    ("nickname", "NICKNAME", false, false),
    ("organization", "ORG", false, false),
    ("title", "TITLE", false, false),
    ("role", "ROLE", false, false),
    ("birthday", "BDAY", false, false),
    ("note", "NOTE", false, false),
    ("custom", "", false, false),
];
const TYPE_CHOICES: &[&str] = &["none", "home", "work", "cell"];

pub fn add_property(world: &mut World) {
    if selected_contact(world).is_none() {
        return;
    }
    let items = ADD_KINDS
        .iter()
        .map(|(label, _, _, _)| PickerItem {
            label: (*label).to_owned(),
            detail: None,
        })
        .collect();
    open_picker(
        world,
        PickerSpec {
            title: "add property".to_owned(),
            items,
            on_select: Box::new(dispatch_add),
        },
    );
}

fn dispatch_add(world: &mut World, kind_index: usize) {
    let Some(&(_, property, has_type, compound)) = ADD_KINDS.get(kind_index) else {
        return;
    };
    if property.is_empty() {
        return raw_add_prompt(world, String::new());
    }
    if has_type {
        type_picker(world, property, compound);
    } else {
        add_value_prompt(world, format!("{property}:"), compound);
    }
}

fn type_picker(world: &mut World, property: &'static str, compound: bool) {
    let items = TYPE_CHOICES
        .iter()
        .map(|choice| PickerItem {
            label: (*choice).to_owned(),
            detail: None,
        })
        .collect();
    open_picker(
        world,
        PickerSpec {
            title: format!("{property} type"),
            items,
            on_select: Box::new(move |world, choice| {
                let prefix = match TYPE_CHOICES.get(choice) {
                    Some(&"none") | None => format!("{property}:"),
                    Some(kind) => format!("{property};TYPE={kind}:"),
                };
                add_value_prompt(world, prefix, compound);
            }),
        },
    );
}

fn add_value_prompt(world: &mut World, prefix: String, compound: bool) {
    if compound {
        return component_chain_step(
            world,
            ADR_COMPONENTS,
            Vec::new(),
            Vec::new(),
            ChainFinish::Add { prefix },
        );
    }
    let label = format!("{}: ", prefix.trim_end_matches(':'));
    let request = PromptRequest::new(
        label,
        Box::new(move |world: &mut World, input: String| {
            if input.trim().is_empty() {
                return;
            }
            let line = format!("{prefix}{}", escape_component(&input));
            apply_mutation(world, move |contact| {
                contact.add_entry_line(&line).map(|_| ())
            });
        }),
    );
    open_prompt(world, request);
}

fn raw_add_prompt(world: &mut World, prefill: String) {
    let request = PromptRequest::new(
        "property: ",
        Box::new(move |world: &mut World, input: String| {
            if input.trim().is_empty() {
                return;
            }
            let attempted = selected_contact(world).map(|contact| {
                let mut contact = contact;
                contact.add_entry_line(&input).map(|_| contact)
            });
            match attempted {
                Some(Ok(contact)) => persist_and_upsert(world, contact),
                Some(Err(error)) => {
                    warn(world, error.to_string());
                    raw_add_prompt(world, input);
                }
                None => {}
            }
        }),
    )
    .with_initial(prefill);
    open_prompt(world, request);
}

pub fn new_contact(world: &mut World) {
    let request = PromptRequest::new(
        "Name: ",
        Box::new(|world: &mut World, name: String| {
            if name.trim().is_empty() {
                return warn(world, "a contact needs a name".to_owned());
            }
            new_contact_email_prompt(world, name);
        }),
    );
    open_prompt(world, request);
}

fn new_contact_email_prompt(world: &mut World, name: String) {
    let request = PromptRequest::new(
        "Email (Enter skips): ",
        Box::new(move |world: &mut World, email: String| {
            let mut contact = Contact::new(name.trim());
            if !email.trim().is_empty()
                && let Err(error) = contact.add_entry_line(&format!("EMAIL:{}", email.trim()))
            {
                return warn(world, error.to_string());
            }
            persist_and_upsert(world, contact);
        }),
    );
    open_prompt(world, request);
}

pub fn delete_selected_contact(world: &mut World) {
    let Some(contact) = selected_contact(world) else {
        return;
    };
    let request = PromptRequest::new(
        format!("Delete {}? (y/n): ", contact.display_name()),
        Box::new(move |world: &mut World, answer: String| {
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                perform_contact_deletion(world, &contact);
            }
        }),
    );
    open_prompt(world, request);
}

fn perform_contact_deletion(world: &mut World, contact: &Contact) {
    let Some(dir) = contacts_dir(world) else {
        return;
    };
    if let Err(error) = nitidus_contacts::delete_contact(&dir, contact) {
        return warn(world, format!("delete failed: {error}"));
    }
    let name = contact.display_name().to_owned();
    let uid = contact.uid().to_owned();
    let mut store = world.resource_mut::<ContactStore>();
    if let Some(position) = store.0.position_of(&uid) {
        store.0.remove(position);
    }
    world.resource_mut::<ContactsView>().detail_selected = 0;
    info(world, format!("deleted {name}"));
}
