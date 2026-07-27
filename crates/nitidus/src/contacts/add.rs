//! Adding and removing whole records: `a` (property-kind picker → TYPE
//! picker → value), `n` (new contact), `D` (delete contact behind a
//! confirm).

use bevy::prelude::*;
use nitidus_contacts::{Contact, escape_component};

use super::ContactStore;
use super::edit::ADR_COMPONENTS;
use super::mutate::{
    ChainFinish, apply_mutation, contacts_dir, info, open_component_form, persist_and_upsert,
    selected_contact, warn,
};
use super::view::ContactsView;
use crate::addresses::format_entry;
use crate::overlay::form::{FieldSpec, FormSpec, open_form};
use crate::overlay::{PickerItem, PickerSpec, open_picker};

const VALUE_FIELD: &str = "value";
const RAW_FIELD: &str = "line";
const NAME_FIELD: &str = "name";
const EMAIL_FIELD: &str = "email";

fn non_empty(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("this cannot be empty".to_owned());
    }
    Ok(())
}

/// Runs the vCard parser over the line so the form can refuse it in
/// place, with the parser's own message.
pub(super) fn valid_entry_line(line: &str) -> Result<(), String> {
    if line.trim().is_empty() {
        return Err("this cannot be empty".to_owned());
    }
    Contact::new("probe")
        .add_entry_line(line)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

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
        return open_component_form(
            world,
            "Address",
            ADR_COMPONENTS,
            Vec::new(),
            ChainFinish::Add { prefix },
        );
    }
    let label = prefix.trim_end_matches(':').to_owned();
    open_form(
        world,
        FormSpec::new(
            label.clone(),
            "Add",
            vec![FieldSpec::text(VALUE_FIELD, label).validated(non_empty)],
            Box::new(move |world: &mut World, values| {
                let line = format!(
                    "{prefix}{}",
                    escape_component(values.get(VALUE_FIELD).trim())
                );
                apply_mutation(world, move |contact| {
                    contact.add_entry_line(&line).map(|_| ())
                });
            }),
        ),
    );
}

/// The escape hatch for anything the modelled editors do not cover: a
/// vCard line, typed verbatim. Validation happens in the form, so a
/// malformed line holds it open with the parser's complaint rather than
/// closing and reopening.
fn raw_add_prompt(world: &mut World, prefill: String) {
    open_form(
        world,
        FormSpec::new(
            "Add property",
            "Add",
            vec![
                FieldSpec::text(RAW_FIELD, "vCard line")
                    .with_initial(prefill)
                    .validated(valid_entry_line),
            ],
            Box::new(move |world: &mut World, values| {
                let line = values.get(RAW_FIELD).to_owned();
                let attempted = selected_contact(world)
                    .map(|mut contact| contact.add_entry_line(&line).map(|_| contact));
                match attempted {
                    Some(Ok(contact)) => persist_and_upsert(world, contact),
                    Some(Err(error)) => warn(world, error.to_string()),
                    None => {}
                }
            }),
        ),
    );
}

pub fn new_contact(world: &mut World) {
    open_form(
        world,
        FormSpec::new(
            "New contact",
            "Create",
            vec![
                FieldSpec::text(NAME_FIELD, "Name").validated(non_empty),
                FieldSpec::text(EMAIL_FIELD, "Email"),
            ],
            Box::new(|world: &mut World, values| {
                let mut contact = Contact::new(values.get(NAME_FIELD).trim());
                let email = values.get(EMAIL_FIELD).trim();
                if !email.is_empty()
                    && let Err(error) = contact.add_entry_line(&format!("EMAIL:{email}"))
                {
                    return warn(world, error.to_string());
                }
                persist_and_upsert(world, contact);
            }),
        ),
    );
}

/// `A`/`:add-contact`: the selected message's sender into the book —
/// the pager's open message wins over the index selection.
pub fn add_contact_from_sender(world: &mut World) {
    let Some((display, addr)) = selected_sender(world) else {
        return warn(world, "no message selected".to_owned());
    };
    let already_known = world.resource::<ContactStore>().0.iter().any(|contact| {
        contact
            .emails()
            .any(|email| email.eq_ignore_ascii_case(&addr))
    });
    if already_known {
        return info(world, format!("{addr} is already in the contact book"));
    }
    open_form(
        world,
        FormSpec::new(
            "Add sender",
            "Add",
            vec![
                FieldSpec::text(NAME_FIELD, "Name").with_initial(display),
                FieldSpec::text(EMAIL_FIELD, "Email").with_initial(addr.clone()),
            ],
            Box::new(move |world: &mut World, values| {
                let typed = values.get(NAME_FIELD).trim().to_owned();
                let name = if typed.is_empty() { addr } else { typed };
                let mut contact = Contact::new(&name);
                let email = values.get(EMAIL_FIELD).trim();
                if let Err(error) = contact.add_entry_line(&format!("EMAIL:{email}")) {
                    return warn(world, error.to_string());
                }
                persist_and_upsert(world, contact);
            }),
        ),
    );
}

fn selected_sender(world: &World) -> Option<(String, String)> {
    let store = world.resource::<crate::store::MailStore>();
    if let Some(open) = world.resource::<crate::pager::PagerState>().open_message() {
        return store
            .position_of(&open.account, &open.folder, &open.id)
            .map(|position| &store.envelopes(&open.account, &open.folder)[position])
            .map(|envelope| (envelope.from_display.clone(), envelope.from_addr.clone()))
            .filter(|(_, addr)| !addr.is_empty());
    }
    let view = world.resource::<crate::index::IndexView>();
    let account = view.account.clone()?;
    let selected = view.selected.clone()?;
    let position = store.position_of(&account, &view.folder, &selected)?;
    let envelope = &store.envelopes(&account, &view.folder)[position];
    (!envelope.from_addr.is_empty())
        .then(|| (envelope.from_display.clone(), envelope.from_addr.clone()))
}

/// `m`/`:compose-to`: from a contact into a composition, To prefilled.
pub fn compose_to_selected(world: &mut World) {
    let Some(contact) = selected_contact(world) else {
        return;
    };
    let Some(email) = contact.primary_email().map(str::to_owned) else {
        return warn(
            world,
            format!("{} has no email address", contact.display_name()),
        );
    };
    let to = format_entry(contact.display_name(), &email);
    crate::shell::activate_tab(world, crate::shell::MAIL_TAB);
    crate::compose::start_compose_to(world, to);
}

pub fn delete_selected_contact(world: &mut World) {
    let Some(contact) = selected_contact(world) else {
        return;
    };
    let detail = contact.display_name().to_owned();
    crate::overlay::open_confirm(
        world,
        crate::overlay::ConfirmSpec::new(
            "Delete contact",
            "Delete this contact?",
            "Delete",
            Box::new(move |world: &mut World| perform_contact_deletion(world, &contact)),
        )
        .with_detail(vec![detail]),
    );
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
