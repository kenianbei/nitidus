//! The property editors: `e` (typed edit, structured N/ADR walk their
//! components), `E` (raw vCard line, any property), `x` (remove).

use bevy::prelude::*;
use nitidus_contacts::calcard::vcard::VCardProperty;

use super::detail;
use super::mutate::{
    ChainFinish, apply_mutation, components_of, open_component_form, persist_and_upsert,
    selected_contact, selected_entry, warn,
};
use crate::overlay::form::{FieldSpec, FormSpec, open_form};

const VALUE_FIELD: &str = "value";
const RAW_FIELD: &str = "line";

const N_COMPONENTS: &[&str] = &[
    "Family name",
    "Given name",
    "Middle names",
    "Prefix",
    "Suffix",
];
pub(super) const ADR_COMPONENTS: &[&str] = &[
    "PO box",
    "Extended address",
    "Street",
    "City",
    "Region",
    "Postal code",
    "Country",
];

pub fn edit_selected(world: &mut World) {
    let Some((entry_index, entry)) = selected_entry(world) else {
        return;
    };
    match entry.name {
        VCardProperty::N => open_component_form(
            world,
            "Name",
            N_COMPONENTS,
            components_of(&entry),
            ChainFinish::Edit { entry_index },
        ),
        VCardProperty::Adr => open_component_form(
            world,
            "Address",
            ADR_COMPONENTS,
            components_of(&entry),
            ChainFinish::Edit { entry_index },
        ),
        ref name if !detail::is_modeled(name) => warn(
            world,
            format!("no editor for {} — E edits the raw line", name.as_str()),
        ),
        ref name => value_prompt(world, entry_index, name.as_str().to_owned()),
    }
}

fn value_prompt(world: &mut World, entry_index: usize, name: String) {
    let prefill = selected_contact(world)
        .and_then(|contact| contact.entry_value_text(entry_index))
        .unwrap_or_default();
    open_form(
        world,
        FormSpec::new(
            name.clone(),
            "Save",
            vec![FieldSpec::text(VALUE_FIELD, name).with_initial(prefill)],
            Box::new(move |world: &mut World, values| {
                let value = values.get(VALUE_FIELD).to_owned();
                apply_mutation(world, move |contact| {
                    contact.edit_entry(entry_index, &value)
                });
            }),
        ),
    );
}

/// The `E` editor: the whole entry as one raw vCard line, any property
/// including unmodeled ones. A rejected line holds the form open with
/// the reason rather than closing and reopening.
pub fn edit_selected_raw(world: &mut World) {
    let Some((entry_index, _)) = selected_entry(world) else {
        return;
    };
    let Some(prefill) = selected_contact(world).and_then(|contact| contact.entry_line(entry_index))
    else {
        return;
    };
    raw_line_prompt(world, entry_index, prefill);
}

fn raw_line_prompt(world: &mut World, entry_index: usize, prefill: String) {
    open_form(
        world,
        FormSpec::new(
            "Edit property",
            "Save",
            vec![
                FieldSpec::text(RAW_FIELD, "vCard line")
                    .with_initial(prefill)
                    .validated(super::add::valid_entry_line),
            ],
            Box::new(move |world: &mut World, values| {
                let line = values.get(RAW_FIELD).to_owned();
                let attempted = selected_contact(world).map(|mut contact| {
                    contact
                        .replace_entry_line(entry_index, &line)
                        .map(|()| contact)
                });
                match attempted {
                    Some(Ok(contact)) => persist_and_upsert(world, contact),
                    Some(Err(error)) => warn(world, error.to_string()),
                    None => {}
                }
            }),
        ),
    );
}

pub fn remove_selected_property(world: &mut World) {
    let Some((entry_index, _)) = selected_entry(world) else {
        return;
    };
    apply_mutation(world, move |contact| {
        contact.remove_entry(entry_index).map(|_| ())
    });
}
