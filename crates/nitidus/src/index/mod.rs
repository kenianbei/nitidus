//! The virtualized message index: a windowed table over `MailStore`.
//! Only the visible rows are ever built; the render fn feeds the actual
//! viewport height back through its widget state.

mod filter;
pub mod marks;
mod mouse;
mod ops;
mod remove;
mod render;
pub mod search;
pub mod staged;
mod thread_view;
mod view;

pub use filter::{clear_filters, push_limit};
pub use marks::{batch_ids, mark_thread, toggle_mark, toggle_visual, unmark_all};
pub(crate) use ops::mark_seen;
pub use ops::{flag_selected, fold, move_cursor, reverse_sort, set_sort, toggle_threads};
pub use remove::{archive_selected, delete_permanent_selected, delete_selected, move_selected};
pub use staged::{OpDelay, StagedOps};
pub use view::{IndexView, SortKey, SortMode, apply_motion, scrolled_top};

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, EnvelopeSummary};
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
        app.init_resource::<search::SearchState>();
        app.init_resource::<staged::OpDelay>();
        app.init_resource::<staged::StagedOps>();
        app.init_resource::<IndexOrder>();
        app.init_resource::<IndexStatus>();
        app.init_resource::<ThreadSet>();
        app.init_resource::<crate::shell::Tabs>();
        app.init_resource::<crate::pager::PagerState>();
        app.add_systems(
            Startup,
            (
                configure_view,
                first_view_sync,
                spawn_index,
                search::spawn_search_line,
            )
                .chain(),
        );
        app.add_systems(Last, staged::flush_on_exit);
        app.add_systems(
            Update,
            (
                thread_view::refresh_threads,
                thread_view::refresh_order,
                marks::clear_marks_on_folder_change,
                staged::tick_staged,
                mouse::clear_departed_hover,
                refresh_index,
                search::refresh_search_line,
            )
                .chain(),
        );
    }
}

/// Selected position / folder total for the statusline (1-based; zero
/// total means hide), plus the viewed folder's display name.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStatus {
    pub selected: usize,
    /// Visible rows — the filtered count while a limit is active.
    pub total: usize,
    pub folder: String,
    /// The whole folder's count; differs from `total` while limited.
    pub folder_total: usize,
    /// Joined `:limit` stack, empty when unlimited.
    pub limits: String,
    /// Marked rows (sticky + visual), for the statusline.
    pub marked: usize,
}

/// Cached display entry list; rebuilt when the store, thread rows, sort
/// mode, or fold state change — never on cursor movement.
#[derive(Resource, Default)]
struct IndexOrder {
    entries: Vec<OrderEntry>,
    for_key: Option<(SortMode, bool, u64, u64)>,
}

#[derive(Component)]
pub struct IndexWidget;

#[derive(Clone, Default)]
struct IndexWindowState {
    /// Cleared while another screen owns the content region — plurimus
    /// repaints refreshed widgets individually, so an inactive screen
    /// must draw nothing rather than rely on draw order.
    active: bool,
    rows: Vec<IndexRow>,
    empty_message: Option<String>,
    context: render::RowContext,
    /// Retained search query — lights up matches in the rows.
    search: Option<String>,
    last_height: u16,
    /// Absolute row of `rows[0]`, anchoring mouse row arithmetic.
    window_top: usize,
    /// Mouse-hovered absolute row; survives refresh, cleared on leave.
    hovered_row: Option<usize>,
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
            WidgetLayout::from(crate::panes::mail_layout(
                crate::panes::MailPane::Messages,
                true,
            )),
            plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
                mouse::handle,
            )]),
            plurimus::UiHoverable,
        ))
        .id();
    enable_fx(&mut commands, &mut registry, entity);
    add_fx(
        &mut registry,
        entity,
        tachyonfx::fx::coalesce(STARTUP_FX_MILLIS),
    );
}

fn current_envelopes<'a>(store: &'a MailStore, index_view: &IndexView) -> &'a [EnvelopeSummary] {
    match &index_view.account {
        Some(account) => store.envelopes(account, &index_view.folder),
        None => &[],
    }
}

fn refresh_index(
    (theme, config): (Res<Theme>, Res<Config>),
    (store, order, tabs): (Res<MailStore>, Res<IndexOrder>, Res<crate::shell::Tabs>),
    pager: Res<crate::pager::PagerState>,
    mut index_view: ResMut<IndexView>,
    mut status: ResMut<IndexStatus>,
    mut widgets: Query<&mut Widget, With<IndexWidget>>,
) -> Result {
    let changed = theme.is_changed()
        || config.is_changed()
        || store.is_changed()
        || index_view.is_changed()
        || order.is_changed()
        || tabs.is_changed()
        || pager.is_changed();
    if !changed {
        return Ok(());
    }
    let envelopes = current_envelopes(&store, &index_view);
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let (last_height, hovered_row) = {
        let previous = widget.get_state::<IndexWindowState>()?;
        (previous.last_height, previous.hovered_row)
    };
    let viewport = usize::from(last_height).max(1);
    let selected_row = view::resolve_selection(&index_view, envelopes, &order.entries);
    // Cache writes bypass change detection: they are derived state, and
    // a tracked write here would re-trigger this system every frame.
    let cached = index_view.bypass_change_detection();
    anchor_selection(cached, envelopes, &order.entries, selected_row, viewport);
    let mut window = build_window_state(
        &theme,
        &config.ui.index,
        WindowSource {
            envelopes,
            entries: &order.entries,
            index_view: cached,
            viewport,
            reading: pager.open_id().cloned(),
        },
    );
    window.last_height = last_height;
    window.hovered_row = hovered_row;
    window.active = !tabs.is_contacts();
    widget.set_state(window)?;
    let limited = !cached.limits.is_empty();
    let position = IndexStatus {
        selected: selected_row.map_or(0, |row| row + 1),
        total: if limited {
            order.entries.len()
        } else {
            envelopes.len()
        },
        folder: folder_display_name(&store, cached),
        folder_total: envelopes.len(),
        limits: cached.limits.join("+"),
        marked: marked_row_count(&order.entries, envelopes, cached),
    };
    if *status != position {
        *status = position;
    }
    Ok(())
}

fn folder_display_name(store: &MailStore, index_view: &IndexView) -> String {
    let Some(account) = &index_view.account else {
        return String::new();
    };
    store
        .folders(account)
        .iter()
        .find(|meta| meta.id == index_view.folder)
        .map(|meta| meta.name.clone())
        .unwrap_or_else(|| index_view.folder.to_string())
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
            // `entries` can be a frame staler than `envelopes` when a
            // row was just removed — resolve leniently, never index.
            index_view.selected = entries
                .get(row)
                .and_then(|entry| envelopes.get(entry.index as usize))
                .map(|envelope| envelope.id.clone());
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

struct WindowSource<'a> {
    envelopes: &'a [EnvelopeSummary],
    entries: &'a [OrderEntry],
    index_view: &'a IndexView,
    viewport: usize,
    /// Which message the reading pane holds, which is not always the
    /// one under the cursor.
    reading: Option<EnvelopeId>,
}

fn build_window_state(
    theme: &Theme,
    index_config: &crate::config::IndexUiConfig,
    source: WindowSource<'_>,
) -> IndexWindowState {
    let empty_message = if source.index_view.account.is_none() {
        Some("no accounts configured".to_owned())
    } else if source.envelopes.is_empty() {
        Some("empty folder".to_owned())
    } else {
        None
    };
    let (rows, window_top) = build_window_rows(&source, index_config.date);
    IndexWindowState {
        active: false,
        rows,
        empty_message,
        context: render::RowContext {
            styles: RowStyles::from_theme(theme),
            columns: index_config.columns.clone(),
        },
        search: source.index_view.search.clone(),
        last_height: 0,
        window_top,
        hovered_row: None,
    }
}

fn build_window_rows(
    source: &WindowSource<'_>,
    date: crate::config::DateFormat,
) -> (Vec<IndexRow>, usize) {
    let index_view = source.index_view;
    let now = jiff::Zoned::now();
    // `entries` can be a frame staler than `envelopes` after a row
    // removal — clamp the window and resolve rows leniently.
    let window_top = index_view.top.min(source.entries.len());
    let window_end = (window_top + source.viewport.max(MIN_WINDOW_ROWS)).min(source.entries.len());
    let rows = source.entries[window_top..window_end]
        .iter()
        .enumerate()
        .filter_map(|(offset, entry)| {
            let row = window_top + offset;
            let visual = marks::visual_rows(index_view);
            source.envelopes.get(entry.index as usize).map(|envelope| {
                let context = render::RowBuildContext {
                    now: &now,
                    date,
                    selected: row == index_view.selected_row,
                    marked: index_view.marked.contains(&envelope.id)
                        || visual.is_some_and(|range| range.contains(&row)),
                    reading: source.reading.as_ref() == Some(&envelope.id),
                };
                render::build_row(envelope, entry, &context)
            })
        })
        .collect();
    (rows, window_top)
}

fn marked_row_count(
    entries: &[self::thread_view::OrderEntry],
    envelopes: &[EnvelopeSummary],
    index_view: &IndexView,
) -> usize {
    let visual = marks::visual_rows(index_view);
    entries
        .iter()
        .enumerate()
        .filter(|(row, entry)| {
            let in_visual = visual.as_ref().is_some_and(|range| range.contains(row));
            in_visual
                || envelopes
                    .get(entry.index as usize)
                    .is_some_and(|envelope| index_view.marked.contains(&envelope.id))
        })
        .count()
}

fn render_index(frame: &mut ratatui::Frame, area: Rect, state: &mut IndexWindowState) -> Result {
    state.last_height = area.height;
    if !state.active {
        return Ok(());
    }
    if let Some(message) = &state.empty_message {
        let paragraph = Paragraph::new(message.as_str())
            .style(state.context.styles.normal)
            .centered();
        frame.render_widget(paragraph, area);
        return Ok(());
    }
    let lines: Vec<Line<'static>> = state
        .rows
        .iter()
        .take(usize::from(area.height))
        .enumerate()
        .map(|(offset, row)| {
            let query = state.search.as_deref();
            if state.hovered_row == Some(state.window_top + offset) && !row.selected {
                let mut hovered = row.clone();
                hovered.hovered = true;
                return render::row_line(&hovered, area.width, &state.context, query);
            }
            render::row_line(row, area.width, &state.context, query)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(state.context.styles.normal),
        area,
    );
    Ok(())
}
