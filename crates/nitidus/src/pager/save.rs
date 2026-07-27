//! Attachment persistence and external opening: decode a part's bytes,
//! write them under a collision-free name, and hand paths or URLs to
//! the system opener.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use nitidus_mail::message::part_bytes;

use super::{PagerState, SaveDir};
use crate::status::MessageLog;

pub(super) fn save_attachment(world: &mut World, part_index: usize) {
    let Some((bytes, name)) = decoded_part(world, part_index) else {
        return;
    };
    let directory = world.resource::<SaveDir>().0.clone();
    let result = write_unique(&directory, &name, &bytes);
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<MessageLog>();
    match result {
        Ok(path) => status.info(format!("saved {}", path.display()), now),
        Err(error) => status.warn(format!("save failed: {error}"), now),
    }
}

pub(super) fn open_attachment(world: &mut World, part_index: usize) {
    let Some((bytes, name)) = decoded_part(world, part_index) else {
        return;
    };
    let directory = std::env::temp_dir().join("nitidus");
    let result =
        write_unique(&directory, &name, &bytes).and_then(|path| spawn_opener(&path).map(|()| path));
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<MessageLog>();
    match result {
        Ok(path) => status.info(format!("opened {}", path.display()), now),
        Err(error) => status.warn(format!("open failed: {error}"), now),
    }
}

fn write_unique(directory: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = uniquify(directory, name);
    std::fs::write(&path, bytes)
        .map(|()| path)
        .map_err(|error| error.to_string())
}

fn decoded_part(world: &World, part_index: usize) -> Option<(Vec<u8>, String)> {
    let pager = world.resource::<PagerState>();
    let open = pager.open.as_ref()?;
    let part = open.view.parts.get(part_index)?;
    let bytes = part_bytes(&open.raw, part.source_index)?;
    let name = part
        .filename
        .clone()
        .unwrap_or_else(|| format!("part-{part_index}.txt"));
    Some((bytes, sanitize(&name)))
}

/// Detached: the frame loop must never wait on a viewer.
pub(super) fn spawn_opener(target: &Path) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_child| ())
        .map_err(|error| format!("xdg-open: {error}"))
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned.trim_start_matches('.').to_owned()
}

fn uniquify(directory: &Path, name: &str) -> PathBuf {
    let candidate = directory.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (name, String::new()),
    };
    (1..)
        .map(|counter| directory.join(format!("{stem}({counter}){extension}")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| directory.join(name))
}
