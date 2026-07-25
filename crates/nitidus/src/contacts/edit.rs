//! The property editors: `e` (typed edit, structured N/ADR walk their
//! components), `E` (raw vCard line, any property), `x` (remove).

use bevy::prelude::*;
use nitidus_contacts::calcard::vcard::VCardProperty;

use super::detail;
use super::mutate::{
    ChainFinish, apply_mutation, component_chain_step, components_of, persist_and_upsert,
    selected_contact, selected_entry, warn,
};
use crate::prompt::{PromptRequest, open_prompt};

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
        VCardProperty::N => component_chain_step(
            world,
            N_COMPONENTS,
            components_of(&entry),
            Vec::new(),
            ChainFinish::Edit { entry_index },
        ),
        VCardProperty::Adr => component_chain_step(
            world,
            ADR_COMPONENTS,
            components_of(&entry),
            Vec::new(),
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
    let request = PromptRequest::new(
        format!("{name}: "),
        Box::new(move |world: &mut World, input: String| {
            apply_mutation(world, move |contact| {
                contact.edit_entry(entry_index, &input)
            });
        }),
    )
    .with_initial(prefill);
    open_prompt(world, request);
}

/// The `E` editor: the whole entry as one raw vCard line, any property
/// including unmodeled ones. A rejected line re-prompts with the reason.
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
    let request = PromptRequest::new(
        "property: ",
        Box::new(move |world: &mut World, input: String| {
            let attempted = selected_contact(world).map(|contact| {
                let mut contact = contact;
                contact
                    .replace_entry_line(entry_index, &input)
                    .map(|()| contact)
            });
            match attempted {
                Some(Ok(contact)) => persist_and_upsert(world, contact),
                Some(Err(error)) => {
                    warn(world, error.to_string());
                    raw_line_prompt(world, entry_index, input);
                }
                None => {}
            }
        }),
    )
    .with_initial(prefill);
    open_prompt(world, request);
}

pub fn remove_selected_property(world: &mut World) {
    let Some((entry_index, _)) = selected_entry(world) else {
        return;
    };
    apply_mutation(world, move |contact| {
        contact.remove_entry(entry_index).map(|_| ())
    });
}
