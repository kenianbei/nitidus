//! A modal file picker over ratatui-explorer: open with a title, an
//! extension filter, and an `on_pick` callback; Enter on a matching
//! file fires it, Esc cancels. Keys resolve against the rebindable
//! `explorer` context and drive the crate through its own `Input`
//! vocabulary; rendering is ours, so the panel matches the app's
//! overlay chrome.

mod mouse;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;
use crokey::KeyCombination;
use nitidus_ui_kit::surface::{FrameChrome, draw_frame};
use nitidus_ui_kit::theme::Theme;
use nitidus_ui_kit::{layer, layout};
use plurimus::{Widget, WidgetLayout, WidgetOrder};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui_explorer::{FileExplorer, Input};

use crate::action::Motion;
use crate::keymap::{KeymapMatch, Keymaps};
use crate::overlay::surface::Surface;
use crate::status::MessageLog;

const PANEL_WIDTH_PCT: u16 = 70;
const PANEL_MAX_ROWS: u16 = 24;
const HINT: &str = " Enter pick ⋅ h/l dirs ⋅ C-h hidden ⋅ Esc cancel ";

pub type PickFn = Box<dyn FnOnce(&mut World, PathBuf) + Send + Sync>;

pub struct ExplorerRequest {
    pub title: String,
    /// Lowercase extensions a pickable file must match; directories
    /// always show (they are how you get anywhere).
    pub extensions: &'static [&'static str],
    /// Starting directory; `None` opens in the working directory.
    pub start_dir: Option<PathBuf>,
    pub on_pick: PickFn,
}

#[derive(Resource, Default)]
pub struct ExplorerState(Option<ActiveExplorer>);

struct ActiveExplorer {
    explorer: FileExplorer,
    title: String,
    on_pick: PickFn,
}

impl ExplorerState {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

pub struct ExplorerPlugin;

impl Plugin for ExplorerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ExplorerState>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.add_systems(Startup, spawn_explorer_widget);
        app.add_systems(Update, refresh_explorer);
    }
}

pub fn open_explorer(world: &mut World, request: ExplorerRequest) {
    let built = FileExplorer::new().and_then(|mut explorer| {
        if let Some(start) = &request.start_dir {
            explorer.set_cwd(start)?;
        }
        let extensions = request.extensions;
        explorer.set_filter_map(move |file| {
            (file.is_dir || matches_extension(&file.path, extensions)).then_some(file)
        })?;
        Ok(explorer)
    });
    match built {
        Ok(explorer) => {
            world.resource_mut::<ExplorerState>().0 = Some(ActiveExplorer {
                explorer,
                title: request.title,
                on_pick: request.on_pick,
            });
            crate::overlay::surface::raise(world, crate::overlay::surface::Surface::Explorer);
        }
        Err(error) => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world
                .resource_mut::<MessageLog>()
                .warn(format!("file browser failed: {error}"), now);
        }
    }
}

fn matches_extension(path: &std::path::Path, extensions: &[&str]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let lowered = extension.to_lowercase();
            extensions.contains(&lowered.as_str())
        })
}

/// Exact single-key `explorer` bindings win; anything else is ignored.
/// No chord waits and no global fallback, matching the picker.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let outcome = {
        let layers = Surface::Explorer.key_layers(world);
        let keymaps = world.resource::<Keymaps>();
        keymaps.resolve_layered(&layers, &[KeyCombination::from(key)])
    };
    if let KeymapMatch::Exact(action) = outcome {
        crate::action::apply_action(world, &action);
    }
    Ok(())
}

pub fn close(world: &mut World) {
    world.resource_mut::<ExplorerState>().0 = None;
}

/// Picks the selected file; on a directory it descends instead.
pub fn confirm(world: &mut World) {
    let is_file = world
        .resource::<ExplorerState>()
        .0
        .as_ref()
        .is_some_and(|active| active.explorer.current().is_file());
    if !is_file {
        return descend(world);
    }
    let Some(active) = world.resource_mut::<ExplorerState>().0.take() else {
        return;
    };
    let path = active.explorer.current().path.clone();
    (active.on_pick)(world, path);
}

pub fn move_cursor(world: &mut World, motion: Motion) {
    send(
        world,
        match motion {
            Motion::Next => Input::Down,
            Motion::Prev => Input::Up,
            Motion::NextPage => Input::PageDown,
            Motion::PrevPage => Input::PageUp,
            Motion::First => Input::Home,
            Motion::Last => Input::End,
            Motion::Parent => Input::Left,
        },
    );
}

pub fn to_parent(world: &mut World) {
    send(world, Input::Left);
}

pub fn descend(world: &mut World) {
    send(world, Input::Right);
}

pub fn toggle_hidden(world: &mut World) {
    send(world, Input::ToggleShowHidden);
}

fn send(world: &mut World, input: Input) {
    let failure = {
        let mut state = world.resource_mut::<ExplorerState>();
        state
            .0
            .as_mut()
            .and_then(|active| active.explorer.handle(input).err())
    };
    if let Some(error) = failure {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
            .warn(format!("file browser: {error}"), now);
    }
}

#[derive(Component)]
struct ExplorerWidget;

#[derive(Clone, Default)]
struct ExplorerWindow {
    is_open: bool,
    title: String,
    rows: Vec<(String, bool, bool)>,
    selected: usize,
    normal: Style,
    selected_style: Style,
    dir_style: Style,
}

fn spawn_explorer_widget(mut commands: Commands) {
    commands.spawn((
        ExplorerWidget,
        Widget::from_render_fn_with_state(render_explorer, ExplorerWindow::default()),
        WidgetLayout::from(layout::centered_panel_layout(
            PANEL_WIDTH_PCT,
            PANEL_MAX_ROWS,
        )),
        WidgetOrder(layer::OVERLAY),
        plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
            mouse::handle,
        )]),
    ));
}

fn refresh_explorer(
    theme: Res<Theme>,
    state: Res<ExplorerState>,
    mut widgets: Query<&mut Widget, With<ExplorerWidget>>,
) -> Result {
    if !theme.is_changed() && !state.is_changed() {
        return Ok(());
    }
    let window = state
        .0
        .as_ref()
        .map_or_else(ExplorerWindow::default, |active| {
            let states = &theme.paper.default;
            ExplorerWindow {
                is_open: true,
                title: format!("{} — {}", active.title, active.explorer.cwd().display()),
                rows: active
                    .explorer
                    .files()
                    .iter()
                    .enumerate()
                    .map(|(index, file)| {
                        (
                            file.name.clone(),
                            file.is_dir,
                            index == active.explorer.selected_idx(),
                        )
                    })
                    .collect(),
                selected: active.explorer.selected_idx(),
                normal: states.normal.style(),
                selected_style: states.selected.style(),
                dir_style: theme.paper.info.normal.style(),
            }
        });
    for mut widget in &mut widgets {
        widget.set_state(window.clone())?;
    }
    Ok(())
}

fn render_explorer(frame: &mut ratatui::Frame, area: Rect, state: &mut ExplorerWindow) -> Result {
    if !state.is_open {
        return Ok(());
    }
    let inner = draw_frame(
        frame.buffer_mut(),
        area,
        FrameChrome {
            title: &state.title,
            hint: Some(HINT),
            style: state.normal,
        },
    );
    let viewport = usize::from(inner.height.max(1));
    let top = scrolled_window_top(state.selected, viewport, state.rows.len());
    let lines: Vec<Line<'static>> = state
        .rows
        .iter()
        .skip(top)
        .take(viewport)
        .map(|(name, is_dir, is_selected)| row_line(name, *is_dir, *is_selected, state))
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    Ok(())
}

fn row_line(name: &str, is_dir: bool, is_selected: bool, state: &ExplorerWindow) -> Line<'static> {
    let style = if is_selected {
        state.selected_style.add_modifier(Modifier::BOLD)
    } else if is_dir {
        state.dir_style
    } else {
        state.normal
    };
    Line::styled(name.to_owned(), style)
}

fn scrolled_window_top(selected: usize, viewport: usize, total: usize) -> usize {
    if total <= viewport {
        return 0;
    }
    selected.saturating_sub(viewport / 2).min(total - viewport)
}
