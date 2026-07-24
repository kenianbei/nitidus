//! World-mutating pager operations: open/close, adjacent-message
//! navigation, scrolling, part switching, save/open, and link picking.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use nitidus_mail::message::part_bytes;
use nitidus_mail::{Flags, MailCommand};
use plurimus::Widget;

use super::body;
use super::render::PagerWindow;
use super::{PagerState, PagerWidget, SaveDir};
use crate::action::{FlagOp, Motion, PagerOp};
use crate::engine::EngineResource;
use crate::index::{self, IndexView};
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::screen::Screen;
use crate::status::StatusMessage;

const FALLBACK_PAGE_ROWS: usize = 20;

pub fn open_selected(world: &mut World) {
    let index_view = world.resource::<IndexView>();
    let (Some(account), Some(id)) = (index_view.account.clone(), index_view.selected.clone())
    else {
        return;
    };
    let folder = index_view.folder.clone();
    index::flag_selected(world, Flags::SEEN, FlagOp::Set);
    let Some(engine) = world.get_resource::<EngineResource>() else {
        return;
    };
    let job = engine.0.next_job();
    let command = MailCommand::FetchMessage { folder, id, job };
    if let Err(error) = engine.0.send(&account, command) {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("fetch failed: {error}"), now);
        return;
    }
    {
        let mut pager = world.resource_mut::<PagerState>();
        pager.open = None;
        pager.loading = Some(job);
    }
    *world.resource_mut::<Screen>() = Screen::Pager;
}

pub fn dispatch(world: &mut World, op: PagerOp) {
    match op {
        PagerOp::Close => close(world),
        PagerOp::NextMessage => adjacent(world, Motion::Next),
        PagerOp::PrevMessage => adjacent(world, Motion::Prev),
        PagerOp::ToggleHeaders => toggle_headers(world),
        PagerOp::SkipQuoted => skip_quoted(world),
        PagerOp::NextPart => switch_part(world, 1),
        PagerOp::PrevPart => switch_part(world, -1),
        PagerOp::SavePart => with_attachment(world, save_attachment),
        PagerOp::OpenPart => with_attachment(world, open_attachment),
        PagerOp::Links => links(world),
    }
}

fn close(world: &mut World) {
    {
        let mut pager = world.resource_mut::<PagerState>();
        pager.open = None;
        pager.loading = None;
    }
    *world.resource_mut::<Screen>() = Screen::Index;
}

fn adjacent(world: &mut World, motion: Motion) {
    index::move_cursor(world, motion);
    open_selected(world);
}

fn toggle_headers(world: &mut World) {
    let mut pager = world.resource_mut::<PagerState>();
    if let Some(open) = pager.open.as_mut() {
        open.show_all_headers = !open.show_all_headers;
    }
}

fn switch_part(world: &mut World, delta: isize) {
    let mut pager = world.resource_mut::<PagerState>();
    let Some(open) = pager.open.as_mut() else { return };
    let bodies = open.view.body_part_indices();
    if bodies.len() < 2 {
        return;
    }
    let current = bodies
        .iter()
        .position(|&index| index == open.part)
        .unwrap_or(0);
    let next = (current as isize + delta).rem_euclid(bodies.len() as isize) as usize;
    open.part = bodies[next];
}

pub fn scroll(world: &mut World, motion: Motion) {
    let mut widgets = world.query_filtered::<&mut Widget, With<PagerWidget>>();
    let Ok(mut widget) = widgets.single_mut(world) else {
        return;
    };
    let Ok(window) = widget.get_state_mut::<PagerWindow>() else {
        return;
    };
    let height = usize::from(window.last_height);
    let page = if height > 1 { height - 1 } else { FALLBACK_PAGE_ROWS };
    let max_scroll = window.lines.len().saturating_sub(height.max(1));
    window.scroll = match motion {
        Motion::Next => (window.scroll + 1).min(max_scroll),
        Motion::Prev => window.scroll.saturating_sub(1),
        Motion::NextPage => (window.scroll + page).min(max_scroll),
        Motion::PrevPage => window.scroll.saturating_sub(page),
        Motion::First => 0,
        Motion::Last => max_scroll,
        Motion::Parent => window.scroll,
    };
}

fn skip_quoted(world: &mut World) {
    let mut widgets = world.query_filtered::<&mut Widget, With<PagerWidget>>();
    let Ok(mut widget) = widgets.single_mut(world) else {
        return;
    };
    let Ok(window) = widget.get_state_mut::<PagerWindow>() else {
        return;
    };
    if let Some(target) = body::skip_quoted_target(&window.kinds.clone(), window.scroll) {
        let height = usize::from(window.last_height).max(1);
        window.scroll = target.min(window.lines.len().saturating_sub(height));
    }
}

/// Direct with one attachment, picker with several, current body part
/// with none.
fn with_attachment(world: &mut World, act: fn(&mut World, usize)) {
    let (attachments, current_part) = {
        let pager = world.resource::<PagerState>();
        let Some(open) = &pager.open else { return };
        (open.view.attachment_indices(), open.part)
    };
    match attachments.len() {
        0 => act(world, current_part),
        1 => act(world, attachments[0]),
        _ => {
            let items = attachment_items(world, &attachments);
            open_picker(
                world,
                PickerSpec {
                    title: "attachments".to_owned(),
                    items,
                    on_select: Box::new(move |world, picked| act(world, attachments[picked])),
                },
            );
        }
    }
}

fn attachment_items(world: &World, attachments: &[usize]) -> Vec<PickerItem> {
    let pager = world.resource::<PagerState>();
    let Some(open) = &pager.open else {
        return Vec::new();
    };
    attachments
        .iter()
        .map(|&index| {
            let part = &open.view.parts[index];
            PickerItem {
                label: part.filename.clone().unwrap_or_else(|| "(unnamed)".to_owned()),
                detail: Some(format!("{} · {} bytes", part.mime, part.size)),
            }
        })
        .collect()
}

fn save_attachment(world: &mut World, part_index: usize) {
    let Some((bytes, name)) = decoded_part(world, part_index) else {
        return;
    };
    let directory = world.resource::<SaveDir>().0.clone();
    let result = std::fs::create_dir_all(&directory)
        .map_err(|error| error.to_string())
        .and_then(|()| {
            let path = uniquify(&directory, &name);
            std::fs::write(&path, &bytes)
                .map(|()| path)
                .map_err(|error| error.to_string())
        });
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<StatusMessage>();
    match result {
        Ok(path) => status.info(format!("saved {}", path.display()), now),
        Err(error) => status.warn(format!("save failed: {error}"), now),
    }
}

fn open_attachment(world: &mut World, part_index: usize) {
    let Some((bytes, name)) = decoded_part(world, part_index) else {
        return;
    };
    let directory = std::env::temp_dir().join("nitidus");
    let result = std::fs::create_dir_all(&directory)
        .map_err(|error| error.to_string())
        .and_then(|()| {
            let path = uniquify(&directory, &name);
            std::fs::write(&path, &bytes)
                .map(|()| path)
                .map_err(|error| error.to_string())
        })
        .and_then(|path| spawn_opener(&path).map(|()| path));
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<StatusMessage>();
    match result {
        Ok(path) => status.info(format!("opened {}", path.display()), now),
        Err(error) => status.warn(format!("open failed: {error}"), now),
    }
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

fn links(world: &mut World) {
    let links = {
        let pager = world.resource::<PagerState>();
        let Some(open) = &pager.open else { return };
        let Some(part) = open.view.parts.get(open.part) else {
            return;
        };
        // Unwrapped build so wrapping can never split a URL.
        body::extract_links(&body::build_body_lines(part, usize::MAX))
    };
    if links.is_empty() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .info("no links in this part".to_owned(), now);
        return;
    }
    let items = links
        .iter()
        .map(|url| PickerItem {
            label: url.clone(),
            detail: None,
        })
        .collect();
    open_picker(
        world,
        PickerSpec {
            title: "links".to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                let now = world.resource::<Time>().elapsed_secs_f64();
                let mut status = world.resource_mut::<StatusMessage>();
                match spawn_opener(Path::new(&links[picked])) {
                    Ok(()) => status.info(format!("opening {}", links[picked]), now),
                    Err(error) => status.warn(format!("open failed: {error}"), now),
                }
            }),
        },
    );
}

/// Detached: the frame loop must never wait on a viewer.
fn spawn_opener(target: &Path) -> Result<(), String> {
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
        .map(|c| if matches!(c, '/' | '\\' | '\0') { '_' } else { c })
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
