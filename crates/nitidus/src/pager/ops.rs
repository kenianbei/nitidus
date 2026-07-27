//! World-mutating pager operations: open/close, adjacent-message
//! navigation, scrolling, part switching, save/open, and link picking.

use std::path::Path;

use bevy::prelude::*;
use nitidus_mail::MailCommand;
use nitidus_mail::message::PartKind;
use plurimus::Widget;

use super::render::PagerWindow;
use super::{PagerState, PagerWidget, body, html, save};
use crate::action::{Motion, PagerOp};
use crate::engine::EngineResource;
use crate::index::{self, IndexView};
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::status::MessageLog;

const FALLBACK_PAGE_ROWS: usize = 20;
/// Anchors merge across wrapped lines, so any sane width works here.
const ANCHOR_RENDER_WIDTH: usize = 200;

pub fn open_selected(world: &mut World) {
    let index_view = world.resource::<IndexView>();
    let (Some(account), Some(id)) = (index_view.account.clone(), index_view.selected.clone())
    else {
        return;
    };
    let folder = index_view.folder.clone();
    // Re-opening what the pane already holds is a network round trip for
    // a message that is right there. A fetch in flight is deliberately
    // not a reason to skip: it may be for a different message.
    if world.resource::<PagerState>().open_id() == Some(&id) {
        crate::focus::focus(world, crate::focus::Pane::Reading);
        return;
    }
    let Some(engine) = world.get_resource::<EngineResource>() else {
        return;
    };
    let job = engine.0.next_job();
    let command = MailCommand::FetchMessage { folder, id, job };
    if let Err(error) = engine.0.send(&account, command) {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
            .warn(format!("fetch failed: {error}"), now);
        return;
    }
    {
        let mut pager = world.resource_mut::<PagerState>();
        pager.open = None;
        pager.loading = Some(job);
    }
    crate::focus::focus(world, crate::focus::Pane::Reading);
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
        PagerOp::SavePart => with_attachment(world, save::save_attachment),
        PagerOp::OpenPart => with_attachment(world, save::open_attachment),
        PagerOp::Links => links(world),
        PagerOp::Zoom => super::toggle_zoom(world),
    }
}

pub(crate) fn close(world: &mut World) {
    super::unzoom(world);
    {
        let mut pager = world.resource_mut::<PagerState>();
        pager.open = None;
        pager.loading = None;
    }
    crate::focus::focus(world, crate::focus::Pane::Messages);
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
    let Some(open) = pager.open.as_mut() else {
        return;
    };
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

/// Pager mouse: the wheel scrolls the body.
pub(super) fn handle_mouse(world: &mut World, entity: Entity, event: plurimus::UiEvent) -> Result {
    if crate::shell::on_contacts(world) || crate::mouse::is_modal_open(world) {
        return Ok(());
    }
    let Some(local) = crate::mouse::local_event(world, entity, event) else {
        return Ok(());
    };
    if let Some(motion) = local.wheel_motion() {
        scroll(world, motion);
    }
    Ok(())
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
    let page = if height > 1 {
        height - 1
    } else {
        FALLBACK_PAGE_ROWS
    };
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
                label: part
                    .filename
                    .clone()
                    .unwrap_or_else(|| "(unnamed)".to_owned()),
                detail: Some(format!("{} · {} bytes", part.mime, part.size)),
            }
        })
        .collect()
}

fn links(world: &mut World) {
    let anchors = current_anchors(world);
    if anchors.is_empty() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
            .info("no links in this part".to_owned(), now);
        return;
    }
    let items = anchors
        .iter()
        .map(|anchor| PickerItem {
            label: anchor.label.clone(),
            detail: (anchor.label != anchor.href).then(|| anchor.href.clone()),
        })
        .collect();
    open_picker(
        world,
        PickerSpec {
            title: "links".to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                let href = &anchors[picked].href;
                let now = world.resource::<Time>().elapsed_secs_f64();
                let mut status = world.resource_mut::<MessageLog>();
                match save::spawn_opener(Path::new(href)) {
                    Ok(()) => status.info(format!("opening {href}"), now),
                    Err(error) => status.warn(format!("open failed: {error}"), now),
                }
            }),
        },
    );
}

/// HTML parts list real anchors; plain text keeps the unwrapped URL
/// scan (wrapping can never split a URL at `usize::MAX`).
fn current_anchors(world: &World) -> Vec<html::Anchor> {
    let pager = world.resource::<PagerState>();
    let Some(open) = &pager.open else {
        return Vec::new();
    };
    let Some(part) = open.view.parts.get(open.part) else {
        return Vec::new();
    };
    if part.kind == PartKind::Html {
        let sanitized = html::sanitize(part.text.as_deref().unwrap_or_default());
        return html::render_html(&sanitized.html, ANCHOR_RENDER_WIDTH).anchors;
    }
    body::extract_links(&body::build_body_lines(part, usize::MAX))
        .into_iter()
        .map(|url| html::Anchor {
            href: url.clone(),
            label: url,
        })
        .collect()
}
