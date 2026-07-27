//! The message log panel: everything the app has said this session,
//! newest last, in the bottom-right corner of the content region.
//!
//! Toasts and the status row both expire; this is where a message that
//! scrolled past is still readable. It reports rather than asks, so it
//! sits in a corner instead of over the panes, and it is the one overlay
//! whose keys only scroll.

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;
use crokey::KeyCombination;
use nitidus_ui_kit::surface::{FrameChrome, draw_frame};
use nitidus_ui_kit::theme::Theme;
use nitidus_ui_kit::{layer, layout};
use plurimus::{Widget, WidgetLayout, WidgetOrder};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::action::Motion;
use crate::keymap::{CONTEXT_LOG, KeymapMatch, Keymaps};
use crate::status::{MessageLog, Severity};

const PANEL_WIDTH_PCT: u16 = 50;
const PANEL_HEIGHT_PCT: u16 = 45;
const TITLE: &str = "messages";
const HINT: &str = " Esc close ";
const FALLBACK_PAGE_ROWS: usize = 10;

/// Open state plus how far back the reader has scrolled. Zero is pinned
/// to the newest entry, which is what opening gives you.
#[derive(Resource, Default)]
pub struct LogPanel {
    open: bool,
    scrollback: usize,
}

impl LogPanel {
    pub fn is_open(&self) -> bool {
        self.open
    }
}

pub struct LogPlugin;

impl Plugin for LogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LogPanel>();
        app.init_resource::<MessageLog>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.add_systems(Startup, spawn_log_widget);
        app.add_systems(Update, refresh_log);
    }
}

pub fn toggle(world: &mut World) {
    if world.resource::<LogPanel>().open {
        return close(world);
    }
    {
        let mut panel = world.resource_mut::<LogPanel>();
        panel.open = true;
        panel.scrollback = 0;
    }
    super::surface::raise(world, super::surface::Surface::MessageLog);
}

pub fn close(world: &mut World) {
    world.resource_mut::<LogPanel>().open = false;
}

/// Exact single-key `log` bindings only, like every other modal.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let outcome = {
        let keymaps = world.resource::<Keymaps>();
        keymaps.lookup(CONTEXT_LOG, &[KeyCombination::from(key)])
    };
    if let KeymapMatch::Exact(action) = outcome {
        crate::action::apply_action(world, &action);
    }
    Ok(())
}

/// Scrollback counts backwards from the newest entry, so new arrivals
/// while the panel is open do not shift what is being read.
pub fn scroll(world: &mut World, motion: Motion) {
    let total = world.resource::<MessageLog>().entries().len();
    let page = viewport_rows(world);
    let limit = total.saturating_sub(1);
    let mut panel = world.resource_mut::<LogPanel>();
    panel.scrollback = match motion {
        Motion::Prev => panel.scrollback.saturating_add(1),
        Motion::Next => panel.scrollback.saturating_sub(1),
        Motion::PrevPage => panel.scrollback.saturating_add(page),
        Motion::NextPage => panel.scrollback.saturating_sub(page),
        Motion::First | Motion::Parent => limit,
        Motion::Last => 0,
    }
    .min(limit);
}

fn viewport_rows(world: &mut World) -> usize {
    let mut widgets = world.query_filtered::<&Widget, With<LogWidget>>();
    widgets
        .single(world)
        .ok()
        .and_then(|widget| widget.get_state::<LogWindow>().ok())
        .map(|window| window.viewport.max(1))
        .unwrap_or(FALLBACK_PAGE_ROWS)
}

#[derive(Component)]
struct LogWidget;

#[derive(Clone, Default)]
struct LogWindow {
    open: bool,
    /// Already windowed to what fits, oldest first.
    lines: Vec<Line<'static>>,
    viewport: usize,
    surface: Style,
    empty: Style,
}

fn spawn_log_widget(mut commands: Commands) {
    commands.spawn((
        LogWidget,
        Widget::from_render_fn_with_state(render_log, LogWindow::default()),
        WidgetLayout::from(layout::corner_panel_layout(
            PANEL_WIDTH_PCT,
            PANEL_HEIGHT_PCT,
        )),
        WidgetOrder(layer::OVERLAY),
    ));
}

fn refresh_log(
    theme: Res<Theme>,
    panel: Res<LogPanel>,
    log: Res<MessageLog>,
    mut widgets: Query<&mut Widget, With<LogWidget>>,
) -> Result {
    if !(theme.is_changed() || panel.is_changed() || log.is_changed()) {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let viewport = widget.get_state::<LogWindow>()?.viewport;
    let lines = if panel.open {
        window_lines(&log, panel.scrollback, viewport.max(1), &theme)
    } else {
        Vec::new()
    };
    widget.set_state(LogWindow {
        open: panel.open,
        lines,
        viewport,
        surface: theme.paper.default.normal.style(),
        empty: theme.paper.default.disabled.style(),
    })?;
    Ok(())
}

/// The `viewport` entries ending `scrollback` back from the newest.
fn window_lines(
    log: &MessageLog,
    scrollback: usize,
    viewport: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let total = log.entries().len();
    let end = total.saturating_sub(scrollback);
    let start = end.saturating_sub(viewport);
    log.entries()
        .skip(start)
        .take(end - start)
        .map(|entry| {
            let style = match entry.severity {
                Severity::Info => theme.paper.info.normal.style(),
                Severity::Warning => theme.paper.warning.normal.style(),
                Severity::Error => theme.paper.error.normal.style(),
            };
            Line::from(Span::styled(entry.text.clone(), style))
        })
        .collect()
}

fn render_log(frame: &mut ratatui::Frame, area: Rect, state: &mut LogWindow) -> Result {
    if !state.open {
        return Ok(());
    }
    let inner = draw_frame(
        frame.buffer_mut(),
        area,
        FrameChrome {
            title: TITLE,
            hint: Some(HINT),
            style: state.surface,
        },
    );
    state.viewport = usize::from(inner.height);
    if state.lines.is_empty() {
        frame.render_widget(
            Paragraph::new("nothing logged yet").style(state.empty),
            inner,
        );
        return Ok(());
    }
    frame.render_widget(Paragraph::new(state.lines.clone()), inner);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::config::RawKeymaps;

    fn log_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<LogPanel>();
        app.init_resource::<MessageLog>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
        app
    }

    /// The whole point of the log: a warning that toasted and expired is
    /// still readable afterwards.
    #[test]
    fn the_panel_opens_over_what_was_already_logged() {
        let mut app = log_app();
        let world = app.world_mut();
        world
            .resource_mut::<MessageLog>()
            .warn("fetch failed".to_owned(), 1.0);
        world.resource_mut::<MessageLog>().take_pending();

        toggle(world);

        assert!(world.resource::<LogPanel>().is_open());
        let lines = window_lines(
            world.resource::<MessageLog>(),
            0,
            5,
            &nitidus_ui_kit::theme::tailwind_dark(),
        );
        assert_eq!(texts(&lines), vec!["fetch failed"]);
    }

    #[test]
    fn toggling_twice_closes_it() {
        let mut app = log_app();
        toggle(app.world_mut());
        toggle(app.world_mut());

        assert!(!app.world().resource::<LogPanel>().is_open());
    }

    #[test]
    fn esc_closes_the_panel_and_q_does_not_quit_through_it() {
        let mut app = log_app();
        toggle(app.world_mut());

        handle_key(app.world_mut(), KeyEvent::from(KeyCode::Char('q'))).unwrap();
        assert!(
            !app.world().resource::<LogPanel>().is_open(),
            "q is bound to :cancel inside the log, not the global quit"
        );
        assert!(
            app.world()
                .resource::<Messages<bevy::app::AppExit>>()
                .is_empty(),
            "a modal must not fall through to global bindings"
        );
    }

    #[test]
    fn scrollback_is_clamped_to_the_history() {
        let mut app = log_app();
        for index in 0..4 {
            app.world_mut()
                .resource_mut::<MessageLog>()
                .info(index.to_string(), index as f64);
        }
        toggle(app.world_mut());

        for _ in 0..20 {
            scroll(app.world_mut(), Motion::Prev);
        }

        assert_eq!(
            app.world().resource::<LogPanel>().scrollback,
            3,
            "scrolling past the oldest entry must stop there"
        );
    }

    fn log_with(count: usize) -> MessageLog {
        let mut log = MessageLog::default();
        for index in 0..count {
            log.info(index.to_string(), index as f64);
        }
        log
    }

    fn texts(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.clone()).collect())
            .collect()
    }

    #[test]
    fn opening_shows_the_newest_entries_last() {
        let log = log_with(10);
        let lines = window_lines(&log, 0, 3, &nitidus_ui_kit::theme::tailwind_dark());

        assert_eq!(texts(&lines), vec!["7", "8", "9"]);
    }

    #[test]
    fn scrollback_walks_backwards_through_the_history() {
        let log = log_with(10);
        let theme = nitidus_ui_kit::theme::tailwind_dark();

        assert_eq!(
            texts(&window_lines(&log, 2, 3, &theme)),
            vec!["5", "6", "7"]
        );
    }

    #[test]
    fn a_log_shorter_than_the_viewport_shows_all_of_it() {
        let log = log_with(2);
        let lines = window_lines(&log, 0, 10, &nitidus_ui_kit::theme::tailwind_dark());

        assert_eq!(texts(&lines), vec!["0", "1"]);
    }

    #[test]
    fn an_empty_log_yields_no_lines() {
        let lines = window_lines(
            &MessageLog::default(),
            0,
            5,
            &nitidus_ui_kit::theme::tailwind_dark(),
        );

        assert!(lines.is_empty());
    }

    #[test]
    fn severity_picks_the_row_style() {
        let mut log = MessageLog::default();
        log.info("quiet".to_owned(), 1.0);
        log.error("loud".to_owned(), 2.0);
        let theme = nitidus_ui_kit::theme::tailwind_dark();

        let lines = window_lines(&log, 0, 5, &theme);

        assert_eq!(lines[0].spans[0].style, theme.paper.info.normal.style());
        assert_eq!(lines[1].spans[0].style, theme.paper.error.normal.style());
    }
}
