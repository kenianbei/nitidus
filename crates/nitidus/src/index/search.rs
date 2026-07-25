//! Incremental `/` search: keystrokes jump the selection live across
//! the visible (possibly limited) rows, Enter accepts and keeps the
//! query for `n`/`N` repeats, Esc restores where you started. The
//! query also drives the match highlight via `IndexView::search`.

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus_mail::EnvelopeId;
use nitidus_ui_kit::layout;
use plurimus::{Widget, WidgetLayout};
use ratatui::widgets::Paragraph;

use super::{IndexOrder, IndexView, filter};
use crate::keymap::{InputMode, Mode};
use crate::status::StatusMessage;
use crate::store::MailStore;

#[derive(Resource, Default)]
pub struct SearchState {
    buffer: String,
    origin: Option<EnvelopeId>,
    origin_row: usize,
    prior_query: Option<String>,
}

impl SearchState {
    pub fn buffer(&self) -> &str {
        &self.buffer
    }
}

/// `/` — record where we started and take over input.
pub fn start_search(world: &mut World) {
    let (origin, origin_row, prior_query) = {
        let view = world.resource::<IndexView>();
        (
            view.selected.clone(),
            view.selected_row,
            view.search.clone(),
        )
    };
    *world.resource_mut::<SearchState>() = SearchState {
        buffer: String::new(),
        origin,
        origin_row,
        prior_query,
    };
    world.resource_mut::<Mode>().0 = InputMode::Search;
}

pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    match key.code {
        KeyCode::Esc => cancel(world),
        KeyCode::Enter => accept(world),
        KeyCode::Backspace => {
            world.resource_mut::<SearchState>().buffer.pop();
            update_live(world);
        }
        KeyCode::Char(character) => {
            world.resource_mut::<SearchState>().buffer.push(character);
            update_live(world);
        }
        _ => {}
    }
    Ok(())
}

fn cancel(world: &mut World) {
    let (origin, origin_row, prior_query) = {
        let state = world.resource::<SearchState>();
        (
            state.origin.clone(),
            state.origin_row,
            state.prior_query.clone(),
        )
    };
    let mut view = world.resource_mut::<IndexView>();
    view.selected = origin;
    view.selected_row = origin_row;
    view.search = prior_query;
    world.resource_mut::<Mode>().0 = InputMode::Normal;
}

fn accept(world: &mut World) {
    let query = world.resource::<SearchState>().buffer.trim().to_lowercase();
    world.resource_mut::<IndexView>().search = (!query.is_empty()).then_some(query);
    world.resource_mut::<Mode>().0 = InputMode::Normal;
}

/// Every keystroke: refresh the highlight query and jump to the first
/// match at-or-after the origin; an unmatched query stays at the origin.
fn update_live(world: &mut World) {
    let query = world.resource::<SearchState>().buffer.trim().to_lowercase();
    world.resource_mut::<IndexView>().search = (!query.is_empty()).then(|| query.clone());
    if query.is_empty() {
        return restore_origin_selection(world);
    }
    let origin_row = world.resource::<SearchState>().origin_row;
    match find_match(world, &query, origin_row, 0) {
        Some(row) => select_row(world, row),
        None => restore_origin_selection(world),
    }
}

fn restore_origin_selection(world: &mut World) {
    let (origin, origin_row) = {
        let state = world.resource::<SearchState>();
        (state.origin.clone(), state.origin_row)
    };
    let mut view = world.resource_mut::<IndexView>();
    view.selected = origin;
    view.selected_row = origin_row;
}

/// `n` — the next match after the selection, wrapping.
pub fn search_next(world: &mut World) {
    repeat_search(world, 1);
}

/// `N` — the previous match before the selection, wrapping.
pub fn search_prev(world: &mut World) {
    repeat_search(world, -1);
}

fn repeat_search(world: &mut World, direction: isize) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    let Some(query) = world.resource::<IndexView>().search.clone() else {
        return world
            .resource_mut::<StatusMessage>()
            .info("no search (press /)".to_owned(), now);
    };
    let from_row = world.resource::<IndexView>().selected_row;
    let total = world.resource::<IndexOrder>().entries.len();
    if total == 0 {
        return;
    }
    let start = wrapped_step(from_row, direction, total);
    match find_match(world, &query, start, direction) {
        Some(row) => select_row(world, row),
        None => world
            .resource_mut::<StatusMessage>()
            .info(format!("no match for {query}"), now),
    }
}

fn wrapped_step(row: usize, direction: isize, total: usize) -> usize {
    if direction >= 0 {
        (row + 1) % total
    } else {
        (row + total - 1) % total
    }
}

/// Scans the visible entries starting at `start_row`, wrapping once
/// around; `direction < 0` scans backwards.
fn find_match(
    world: &World,
    query_lower: &str,
    start_row: usize,
    direction: isize,
) -> Option<usize> {
    let view = world.resource::<IndexView>();
    let store = world.resource::<MailStore>();
    let order = world.resource::<IndexOrder>();
    let account = view.account.as_ref()?;
    let envelopes = store.envelopes(account, &view.folder);
    let total = order.entries.len();
    if total == 0 {
        return None;
    }
    (0..total)
        .map(|offset| {
            if direction >= 0 {
                (start_row + offset) % total
            } else {
                (start_row + total - offset) % total
            }
        })
        .find(|&row| {
            order.entries.get(row).is_some_and(|entry| {
                envelopes
                    .get(entry.index as usize)
                    .is_some_and(|envelope| filter::matches(envelope, query_lower))
            })
        })
}

fn select_row(world: &mut World, row: usize) {
    let selected = {
        let view = world.resource::<IndexView>();
        let store = world.resource::<MailStore>();
        let order = world.resource::<IndexOrder>();
        view.account.as_ref().and_then(|account| {
            order.entries.get(row).and_then(|entry| {
                store
                    .envelopes(account, &view.folder)
                    .get(entry.index as usize)
                    .map(|envelope| envelope.id.clone())
            })
        })
    };
    let mut view = world.resource_mut::<IndexView>();
    view.selected = selected;
    view.selected_row = row;
}

#[derive(Component)]
pub(super) struct SearchLine;

#[derive(Clone, Default)]
struct SearchRender(String);

pub(super) fn spawn_search_line(mut commands: Commands) {
    let mut widget = Widget::from_render_fn_with_state(render_search, SearchRender::default());
    widget.set_enabled(false);
    commands.spawn((
        SearchLine,
        widget,
        WidgetLayout::from(layout::statusline_layout()),
    ));
}

pub(super) fn refresh_search_line(
    mode: Option<Res<Mode>>,
    state: Res<SearchState>,
    mut widgets: Query<&mut Widget, With<SearchLine>>,
) -> Result {
    let Some(mode) = mode else {
        return Ok(());
    };
    if !mode.is_changed() && !state.is_changed() {
        return Ok(());
    }
    for mut widget in &mut widgets {
        widget.set_enabled(mode.0 == InputMode::Search);
        widget.set_state(SearchRender(format!("/{}", state.buffer)))?;
    }
    Ok(())
}

fn render_search(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut SearchRender,
) -> Result {
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(Paragraph::new(state.0.as_str()), area);
    Ok(())
}
