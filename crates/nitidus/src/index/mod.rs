//! The virtualized message index: a windowed table over `MailStore`.
//! Only the visible rows are ever built; the render fn feeds the actual
//! viewport height back through its widget state.

mod ops;
mod render;
mod view;

pub use ops::{flag_selected, move_cursor, set_sort};
pub use view::{IndexView, SortKey, SortMode};

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeSummary};
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{TachyonRegistry, Widget, WidgetLayout, add_fx, enable_fx};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use self::render::{IndexRow, RowStyles};
use crate::bootstrap::request_sync;
use crate::config::Config;
use crate::engine::EngineResource;
use crate::store::{MailStore, SyncTracker};

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
        app.add_systems(Startup, (configure_view, first_view_sync, spawn_index).chain());
        app.add_systems(Update, (refresh_order, refresh_index).chain());
    }
}

/// Selected position / folder total for the statusline (1-based; zero
/// total means hide).
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStatus {
    pub selected: usize,
    pub total: usize,
}

/// Cached display permutation; recomputed when the store or sort mode
/// changes, not on cursor movement.
#[derive(Resource, Default)]
struct IndexOrder {
    order: Vec<u32>,
    for_sort: Option<SortMode>,
}

#[derive(Component)]
struct IndexWidget;

#[derive(Clone, Default)]
struct IndexWindowState {
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

fn refresh_order(
    store: Res<MailStore>,
    index_view: Res<IndexView>,
    mut order: ResMut<IndexOrder>,
) {
    if !store.is_changed() && order.for_sort == Some(index_view.sort) {
        return;
    }
    order.order = view::compute_order(current_envelopes(&store, &index_view), index_view.sort);
    order.for_sort = Some(index_view.sort);
}

fn refresh_index(
    theme: Res<Theme>,
    store: Res<MailStore>,
    order: Res<IndexOrder>,
    mut index_view: ResMut<IndexView>,
    mut status: ResMut<IndexStatus>,
    mut widgets: Query<&mut Widget, With<IndexWidget>>,
) -> Result {
    let changed =
        theme.is_changed() || store.is_changed() || index_view.is_changed() || order.is_changed();
    if !changed {
        return Ok(());
    }
    let envelopes = current_envelopes(&store, &index_view);
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let last_height = widget.get_state::<IndexWindowState>()?.last_height;
    let viewport = usize::from(last_height).max(1);
    let selected_row = view::resolve_selection(&index_view, envelopes, &order.order);
    // Cache writes bypass change detection: they are derived state, and
    // a tracked write here would re-trigger this system every frame.
    let cached = index_view.bypass_change_detection();
    anchor_selection(cached, envelopes, &order.order, selected_row, viewport);
    let mut window = build_window_state(&theme, envelopes, &order.order, cached, viewport);
    window.last_height = last_height;
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
    order: &[u32],
    selected_row: Option<usize>,
    viewport: usize,
) {
    match selected_row {
        Some(row) => {
            index_view.selected = order
                .get(row)
                .map(|&index| envelopes[index as usize].id.clone());
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
    order: &[u32],
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
    let window_end = (index_view.top + viewport.max(MIN_WINDOW_ROWS)).min(order.len());
    let rows = order[index_view.top..window_end]
        .iter()
        .enumerate()
        .map(|(offset, &index)| {
            let selected = index_view.top + offset == index_view.selected_row;
            render::build_row(&envelopes[index as usize], selected, &now)
        })
        .collect();
    IndexWindowState {
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

