//! The virtualized message index: a windowed table over `MailStore`.
//! Only the visible rows are ever built; the render fn feeds the actual
//! viewport height back through its widget state.

mod ops;
mod render;
mod thread_view;
mod view;

pub use ops::{flag_selected, fold, move_cursor, set_sort, toggle_threads};
pub use view::{IndexView, SortKey, SortMode, apply_motion};

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeSummary};
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{TachyonRegistry, Widget, WidgetLayout, add_fx, enable_fx};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use self::render::{IndexRow, RowStyles};
use self::thread_view::OrderEntry;
use crate::bootstrap::request_sync;
use crate::config::Config;
use crate::engine::EngineResource;
use crate::store::{MailStore, SyncTracker, ThreadSet};

const STARTUP_FX_MILLIS: u32 = 800;
/// Rows built beyond the last known viewport, so a taller resize has
/// spare rows before the next refresh catches up.
const MIN_WINDOW_ROWS: usize = 100;

pub struct IndexPlugin;

impl Plugin for IndexPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IndexView>();
        app.init_resource::<IndexOrder>();
        app.init_resource::<IndexStatus>();
        app.init_resource::<ThreadSet>();
        app.init_resource::<crate::screen::Screen>();
        app.add_systems(Startup, (configure_view, first_view_sync, spawn_index).chain());
        app.add_systems(
            Update,
            (
                thread_view::refresh_threads,
                thread_view::refresh_order,
                refresh_index,
            )
                .chain(),
        );
    }
}

/// Selected position / folder total for the statusline (1-based; zero
/// total means hide).
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStatus {
    pub selected: usize,
    pub total: usize,
}

/// Cached display entry list; rebuilt when the store, thread rows, sort
/// mode, or fold state change — never on cursor movement.
#[derive(Resource, Default)]
struct IndexOrder {
    entries: Vec<OrderEntry>,
    for_key: Option<(SortMode, bool, u64)>,
}

#[derive(Component)]
struct IndexWidget;

#[derive(Clone, Default)]
struct IndexWindowState {
    /// Cleared while another screen owns the content region — plurimus
    /// repaints refreshed widgets individually, so an inactive screen
    /// must draw nothing rather than rely on draw order.
    active: bool,
    rows: Vec<IndexRow>,
    empty_message: Option<String>,
    styles: RowStyles,
    last_height: u16,
}

fn configure_view(mut index_view: ResMut<IndexView>, config: Res<Config>) {
    index_view.account = config
        .accounts
        .first()
        .map(|account| AccountId::new(&account.name));
}

/// No-op while INBOX is eagerly synced at registration; folder switching
/// inherits the lazy contract through this same call.
fn first_view_sync(
    index_view: Res<IndexView>,
    engine: Option<Res<EngineResource>>,
    mut tracker: ResMut<SyncTracker>,
) {
    let (Some(account), Some(engine)) = (&index_view.account, engine) else {
        return;
    };
    if tracker.is_tracked(account, &index_view.folder) {
        return;
    }
    if let Err(error) = request_sync(&engine.0, &mut tracker, account, &index_view.folder) {
        tracing::warn!("first-view sync of {} failed: {error}", index_view.folder);
    }
}

fn spawn_index(mut commands: Commands, mut registry: NonSendMut<TachyonRegistry>) {
    let entity = commands
        .spawn((
            IndexWidget,
            Widget::from_render_fn_with_state(render_index, IndexWindowState::default()),
            WidgetLayout::from(layout::content_layout()),
        ))
        .id();
    enable_fx(&mut commands, &mut registry, entity);
    add_fx(
        &mut registry,
        entity,
        tachyonfx::fx::coalesce(STARTUP_FX_MILLIS),
    );
}

fn current_envelopes<'a>(
    store: &'a MailStore,
    index_view: &IndexView,
) -> &'a [EnvelopeSummary] {
    match &index_view.account {
        Some(account) => store.envelopes(account, &index_view.folder),
        None => &[],
    }
}

fn refresh_index(
    theme: Res<Theme>,
    store: Res<MailStore>,
    order: Res<IndexOrder>,
    screen: Res<crate::screen::Screen>,
    mut index_view: ResMut<IndexView>,
    mut status: ResMut<IndexStatus>,
    mut widgets: Query<&mut Widget, With<IndexWidget>>,
) -> Result {
    let changed = theme.is_changed()
        || store.is_changed()
        || index_view.is_changed()
        || order.is_changed()
        || screen.is_changed();
    if !changed {
        return Ok(());
    }
    let envelopes = current_envelopes(&store, &index_view);
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let last_height = widget.get_state::<IndexWindowState>()?.last_height;
    let viewport = usize::from(last_height).max(1);
    let selected_row = view::resolve_selection(&index_view, envelopes, &order.entries);
    // Cache writes bypass change detection: they are derived state, and
    // a tracked write here would re-trigger this system every frame.
    let cached = index_view.bypass_change_detection();
    anchor_selection(cached, envelopes, &order.entries, selected_row, viewport);
    let mut window = build_window_state(&theme, envelopes, &order.entries, cached, viewport);
    window.last_height = last_height;
    window.active = *screen == crate::screen::Screen::Index;
    widget.set_state(window)?;
    let position = IndexStatus {
        selected: selected_row.map_or(0, |row| row + 1),
        total: envelopes.len(),
    };
    if *status != position {
        *status = position;
    }
    Ok(())
}

fn anchor_selection(
    index_view: &mut IndexView,
    envelopes: &[EnvelopeSummary],
    entries: &[OrderEntry],
    selected_row: Option<usize>,
    viewport: usize,
) {
    match selected_row {
        Some(row) => {
            index_view.selected = entries
                .get(row)
                .map(|entry| envelopes[entry.index as usize].id.clone());
            index_view.selected_row = row;
            index_view.top = view::scrolled_top(index_view.top, row, viewport);
        }
        None => {
            index_view.selected = None;
            index_view.selected_row = 0;
            index_view.top = 0;
        }
    }
}

fn build_window_state(
    theme: &Theme,
    envelopes: &[EnvelopeSummary],
    entries: &[OrderEntry],
    index_view: &IndexView,
    viewport: usize,
) -> IndexWindowState {
    let empty_message = if index_view.account.is_none() {
        Some("no accounts configured".to_owned())
    } else if envelopes.is_empty() {
        Some("empty folder".to_owned())
    } else {
        None
    };
    let now = jiff::Zoned::now();
    let window_end = (index_view.top + viewport.max(MIN_WINDOW_ROWS)).min(entries.len());
    let rows = entries[index_view.top..window_end]
        .iter()
        .enumerate()
        .map(|(offset, entry)| {
            let selected = index_view.top + offset == index_view.selected_row;
            render::build_row(&envelopes[entry.index as usize], entry, selected, &now)
        })
        .collect();
    IndexWindowState {
        active: false,
        rows,
        empty_message,
        styles: RowStyles::from_theme(theme),
        last_height: 0,
    }
}

fn render_index(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut IndexWindowState,
) -> Result {
    state.last_height = area.height;
    if !state.active {
        return Ok(());
    }
    if let Some(message) = &state.empty_message {
        let paragraph = Paragraph::new(message.as_str())
            .style(state.styles.normal)
            .centered();
        frame.render_widget(paragraph, area);
        return Ok(());
    }
    let lines: Vec<Line<'static>> = state
        .rows
        .iter()
        .take(usize::from(area.height))
        .map(|row| render::row_line(row, area.width, &state.styles))
        .collect();
    frame.render_widget(Paragraph::new(lines).style(state.styles.normal), area);
    Ok(())
}

