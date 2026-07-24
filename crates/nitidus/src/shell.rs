//! Persistent application chrome: tab bar, content pane, statusline, and
//! the temporary quit bindings (replaced by the action router in 1a.4).

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyModifiers};
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{
    KeyBinding, TachyonRegistry, UiActions, UiEvent, UiInputBinding, Widget, WidgetLayout, add_fx,
    enable_fx,
};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

const STARTUP_FX_MILLIS: u32 = 800;

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tabs>();
        app.add_systems(Startup, (spawn_shell, apply_startup_fx).chain());
        app.add_systems(
            Update,
            (refresh_tab_bar, refresh_content, refresh_statusline),
        );
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct Tabs {
    pub labels: Vec<String>,
    pub active: usize,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            labels: vec!["mail".to_owned()],
            active: 0,
        }
    }
}

impl Tabs {
    pub fn active_label(&self) -> &str {
        self.labels.get(self.active).map_or("", String::as_str)
    }
}

#[derive(Component)]
pub struct TabBar;

#[derive(Component)]
pub struct ContentPane;

#[derive(Component)]
pub struct Statusline;

#[derive(Clone, Default)]
struct StatuslineState {
    left: String,
    right: String,
    style: Style,
}

fn spawn_shell(mut commands: Commands) {
    commands.spawn((
        TabBar,
        Widget::from_widget(Paragraph::new("")),
        WidgetLayout::from(layout::tab_bar_layout()),
        quit_actions(),
    ));
    commands.spawn((
        ContentPane,
        Widget::from_widget(Block::new()),
        WidgetLayout::from(layout::content_layout()),
    ));
    commands.spawn((
        Statusline,
        Widget::from_render_fn_with_state(render_statusline, StatuslineState::default()),
        WidgetLayout::from(layout::statusline_layout()),
    ));
}

fn apply_startup_fx(
    mut commands: Commands,
    mut registry: NonSendMut<TachyonRegistry>,
    panes: Query<Entity, With<ContentPane>>,
) {
    for entity in &panes {
        enable_fx(&mut commands, &mut registry, entity);
        add_fx(
            &mut registry,
            entity,
            tachyonfx::fx::coalesce(STARTUP_FX_MILLIS),
        );
    }
}

fn quit_actions() -> UiActions {
    UiActions::new(vec![
        UiInputBinding::key_binding(KeyBinding::press(KeyCode::Char('q')), handle_quit).global(),
        UiInputBinding::key_binding(
            KeyBinding::press(KeyCode::Char('c')).with_modifiers(KeyModifiers::CONTROL),
            handle_quit,
        )
        .global(),
    ])
}

fn handle_quit(world: &mut World, _entity: Entity, _event: UiEvent) -> Result {
    world.write_message(AppExit::Success);
    Ok(())
}

fn refresh_tab_bar(
    theme: Res<Theme>,
    tabs: Res<Tabs>,
    mut widgets: Query<&mut Widget, With<TabBar>>,
) {
    if !theme.is_changed() && !tabs.is_changed() {
        return;
    }
    for mut widget in &mut widgets {
        widget.set_widget(tab_bar_paragraph(&tabs, &theme));
    }
}

fn refresh_content(theme: Res<Theme>, mut widgets: Query<&mut Widget, With<ContentPane>>) {
    if !theme.is_changed() {
        return;
    }
    for mut widget in &mut widgets {
        widget.set_widget(Block::new().style(theme.base.default.normal.style()));
    }
}

fn refresh_statusline(
    theme: Res<Theme>,
    tabs: Res<Tabs>,
    mut widgets: Query<&mut Widget, With<Statusline>>,
) -> Result {
    if !theme.is_changed() && !tabs.is_changed() {
        return Ok(());
    }
    for mut widget in &mut widgets {
        widget.set_state(StatuslineState {
            left: tabs.active_label().to_owned(),
            right: format!("nitidus v{}", env!("CARGO_PKG_VERSION")),
            style: theme.paper.default.normal.style(),
        })?;
    }
    Ok(())
}

fn tab_bar_paragraph(tabs: &Tabs, theme: &Theme) -> Paragraph<'static> {
    let states = &theme.base.default;
    let spans: Vec<Span<'static>> = tabs
        .labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let style = if index == tabs.active {
                states.selected.style()
            } else {
                states.normal.style()
            };
            Span::styled(format!(" {label} "), style)
        })
        .collect();
    Paragraph::new(Line::from(spans)).style(states.normal.style())
}

fn render_statusline(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut StatuslineState,
) -> Result {
    let text = statusline_text(&state.left, &state.right, area.width);
    frame.render_widget(Paragraph::new(text).style(state.style), area);
    Ok(())
}

fn statusline_text(left: &str, right: &str, width: u16) -> String {
    let width = usize::from(width);
    let used = left.chars().count() + right.chars().count();
    if used >= width {
        return format!("{left} {right}");
    }
    format!("{left}{}{right}", " ".repeat(width - used))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use nitidus_ui_kit::theme::tailwind_dark;

    fn shell_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_non_send_resource(TachyonRegistry::default());
        app.insert_resource(tailwind_dark());
        app.add_plugins(ShellPlugin);
        app
    }

    #[test]
    fn spawns_the_three_chrome_widgets() {
        let mut app = shell_app();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&TabBar>().iter(world).count(), 1);
        assert_eq!(world.query::<&ContentPane>().iter(world).count(), 1);
        assert_eq!(world.query::<&Statusline>().iter(world).count(), 1);
    }

    #[test]
    fn statusline_state_reflects_active_tab_and_version() -> Result {
        let mut app = shell_app();
        app.update();
        let world = app.world_mut();
        let mut query = world.query_filtered::<&Widget, With<Statusline>>();
        let widget = query.single(world)?;
        let state = widget.get_state::<StatuslineState>()?;
        assert_eq!(state.left, "mail");
        assert_eq!(
            state.right,
            format!("nitidus v{}", env!("CARGO_PKG_VERSION"))
        );
        Ok(())
    }

    #[test]
    fn statusline_text_pads_between_segments() {
        assert_eq!(statusline_text("mail", "v1", 10), "mail    v1");
        assert_eq!(statusline_text("mail", "v1", 4), "mail v1");
    }

    #[test]
    fn default_tabs_has_one_active_mail_tab() {
        let tabs = Tabs::default();
        assert_eq!(tabs.active_label(), "mail");
        assert_eq!(tabs.labels.len(), 1);
    }
}
