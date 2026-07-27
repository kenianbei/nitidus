//! Import and export: how whole address books get in and out.
//! Importing never overwrites — cards whose UID already exists are
//! skipped, so a re-import cannot clobber local edits.

use std::path::PathBuf;

use bevy::prelude::*;
use nitidus_contacts::{parse_all, save_contact, write_export};

use super::ContactStore;
use super::mutate::{contacts_dir, info, warn};
use super::view::ContactsView;

const DEFAULT_EXPORT_PATH: &str = "~/nitidus-contacts.vcf";

pub fn import_contacts(world: &mut World, path_argument: Option<&str>) {
    match path_argument {
        Some(path) => run_import(world, expand_tilde(path)),
        None => import_path_prompt(world),
    }
}

fn import_path_prompt(world: &mut World) {
    crate::explorer::open_explorer(
        world,
        crate::explorer::ExplorerRequest {
            title: "import contacts".to_owned(),
            extensions: &["vcf"],
            start_dir: None,
            on_pick: Box::new(run_import),
        },
    );
}

pub(super) fn run_import(world: &mut World, path: PathBuf) {
    let input = match std::fs::read_to_string(&path) {
        Ok(input) => input,
        Err(error) => return warn(world, format!("import failed: {}: {error}", path.display())),
    };
    let (cards, issues) = parse_all(&input);
    if cards.is_empty() && issues.is_empty() {
        return warn(world, format!("no vCards in {}", path.display()));
    }
    let Some(dir) = contacts_dir(world) else {
        return;
    };
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut failed = issues.len();
    for issue in &issues {
        tracing::warn!("import {}: {issue}", path.display());
    }
    for card in cards {
        if world
            .resource::<ContactStore>()
            .0
            .position_of(card.uid())
            .is_some()
        {
            skipped += 1;
            continue;
        }
        match save_contact(&dir, &card) {
            Ok(_) => {
                world.resource_mut::<ContactStore>().0.upsert(card);
                imported += 1;
            }
            Err(error) => {
                tracing::warn!("import {}: {} failed: {error}", path.display(), card.uid());
                failed += 1;
            }
        }
    }
    world.resource_mut::<ContactsView>().detail_selected = 0;
    info(
        world,
        format!("imported {imported}, skipped {skipped} existing, {failed} failed"),
    );
}

pub fn export_contacts(world: &mut World, path_argument: Option<&str>) {
    match path_argument {
        Some(path) => run_export(world, expand_tilde(path)),
        None => export_path_prompt(world),
    }
}

const EXPORT_FIELD: &str = "path";

fn export_path_prompt(world: &mut World) {
    crate::overlay::form::open_form(
        world,
        crate::overlay::form::FormSpec::new(
            "Export contacts",
            "Export",
            vec![
                crate::overlay::form::FieldSpec::text(EXPORT_FIELD, "Write to")
                    .with_initial(DEFAULT_EXPORT_PATH)
                    .validated(|value| {
                        if value.trim().is_empty() {
                            return Err("a path is required".to_owned());
                        }
                        Ok(())
                    }),
            ],
            Box::new(|world: &mut World, values| {
                run_export(world, expand_tilde(values.get(EXPORT_FIELD).trim()));
            }),
        ),
    );
}

fn run_export(world: &mut World, path: PathBuf) {
    let store = world.resource::<ContactStore>();
    let total = store.0.len();
    if total == 0 {
        return warn(world, "the contact book is empty".to_owned());
    }
    match write_export(&path, store.0.iter()) {
        Ok(()) => info(
            world,
            format!("exported {total} contacts to {}", path.display()),
        ),
        Err(error) => warn(world, format!("export failed: {error}")),
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if (path == "~" || path.starts_with("~/"))
        && let Ok(home) = etcetera::home_dir()
    {
        return home.join(path.trim_start_matches("~/"));
    }
    PathBuf::from(path)
}
