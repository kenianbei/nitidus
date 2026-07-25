//! Persistent application chrome: tab bar and a three-segment
//! statusline (tab | chord hint or status message | version). The
//! content region belongs to the active screen's own widget; nothing
//! here may draw into it. All input flows through the action router.

use bevy::prelude::*;
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{Widget, WidgetLayout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::engine::EngineStatus;
use crate::index::IndexStatus;
use crate::pager::PagerStatus;
use crate::router::PendingKeys;
use crate::status::{Severity, StatusMessage};

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tabs>();
        app.init_resource::<StatusMessage>();
        app.init_resource::<IndexStatus>();
        app.init_resource::<PagerStatus>();
        app.add_systems(Startup, spawn_shell);
        app.add_systems(Update, (refresh_tab_bar, refresh_statusline));
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

    pub fn rotate(&mut self, delta: isize) {
        let len = self.labels.len() as isize;
        if len > 0 {
            self.active = (self.active as isize + delta).rem_euclid(len) as usize;
        }
    }
}

#[derive(Component)]
pub struct TabBar;

#[derive(Component)]
pub struct Statusline;

#[derive(Clone, Default)]
struct StatuslineState {
    left: String,
    center: String,
    center_style: Style,
    right: String,
    style: Style,
}

fn spawn_shell(mut commands: Commands) {
    commands.spawn((
        TabBar,
        Widget::from_widget(Paragraph::new("")),
        WidgetLayout::from(layout::tab_bar_layout()),
    ));
    commands.spawn((
        Statusline,
        Widget::from_render_fn_with_state(render_statusline, StatuslineState::default()),
        WidgetLayout::from(layout::statusline_layout()),
    ));
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

#[derive(bevy::ecs::system::SystemParam)]
struct StatuslineInputs<'w> {
    theme: Res<'w, Theme>,
    tabs: Res<'w, Tabs>,
    pending: Res<'w, PendingKeys>,
    status: Res<'w, StatusMessage>,
    engine_status: Res<'w, EngineStatus>,
    index_status: Res<'w, IndexStatus>,
    pager_status: Res<'w, PagerStatus>,
}

impl StatuslineInputs<'_> {
    fn any_changed(&self) -> bool {
        self.theme.is_changed()
            || self.tabs.is_changed()
            || self.pending.is_changed()
            || self.status.is_changed()
            || self.engine_status.is_changed()
            || self.index_status.is_changed()
            || self.pager_status.is_changed()
    }
}

fn refresh_statusline(
    inputs: StatuslineInputs,
    mut widgets: Query<&mut Widget, With<Statusline>>,
) -> Result {
    if !inputs.any_changed() {
        return Ok(());
    }
    let theme = &inputs.theme;
    let (center, center_style) =
        center_segment(&inputs.pending, &inputs.status, &inputs.pager_status, theme);
    for mut widget in &mut widgets {
        widget.set_state(StatuslineState {
            left: left_segment(&inputs.tabs, &inputs.engine_status, &inputs.index_status),
            center: center.clone(),
            center_style,
            right: format!("nitidus v{}", env!("CARGO_PKG_VERSION")),
            style: theme.paper.default.normal.style(),
        })?;
    }
    Ok(())
}

fn left_segment(tabs: &Tabs, engine_status: &EngineStatus, index_status: &IndexStatus) -> String {
    let mut segment = tabs.active_label().to_owned();
    if !index_status.folder.is_empty() {
        segment = format!("{segment} ⋅ {}", index_status.folder);
    }
    if let Some(summary) = engine_status.summary() {
        segment = format!("{segment} ⋅ {summary}");
    }
    if index_status.total > 0 {
        segment = format!(
            "{segment} ⋅ {}/{}",
            index_status.selected, index_status.total
        );
    }
    segment
}

fn center_segment(
    pending: &PendingKeys,
    status: &StatusMessage,
    pager_status: &PagerStatus,
    theme: &Theme,
) -> (String, Style) {
    if let Some((text, severity)) = status.current() {
        let palette = &theme.paper;
        let style = match severity {
            Severity::Info => palette.info.normal.style(),
            Severity::Warning => palette.warning.normal.style(),
            Severity::Error => palette.error.normal.style(),
        };
        return (text.to_owned(), style);
    }
    if let Some(hint) = pending.hint() {
        return (hint, theme.paper.default.focused.style());
    }
    if let Some(part) = &pager_status.part {
        return (part.clone(), theme.paper.default.normal.style());
    }
    (String::new(), theme.paper.default.normal.style())
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
    frame.render_widget(Paragraph::new(statusline_line(state, area.width)), area);
    Ok(())
}

fn statusline_line(state: &StatuslineState, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let used = [&state.left, &state.center, &state.right]
        .iter()
        .map(|s| s.chars().count())
        .sum::<usize>();
    let remaining = width.saturating_sub(used).max(2);
    let pad_left = remaining / 2;
    let pad_right = remaining - pad_left;
    Line::from(vec![
        Span::styled(state.left.clone(), state.style),
        Span::styled(" ".repeat(pad_left), state.style),
        Span::styled(state.center.clone(), state.center_style),
        Span::styled(" ".repeat(pad_right), state.style),
        Span::styled(state.right.clone(), state.style),
    ])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use nitidus_ui_kit::theme::tailwind_dark;

    fn shell_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(tailwind_dark());
        app.init_resource::<PendingKeys>();
        app.init_resource::<EngineStatus>();
        app.add_plugins(ShellPlugin);
        app
    }

    #[test]
    fn spawns_only_tab_bar_and_statusline() {
        let mut app = shell_app();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&TabBar>().iter(world).count(), 1);
        assert_eq!(world.query::<&Statusline>().iter(world).count(), 1);
        assert_eq!(
            world.query::<&Widget>().iter(world).count(),
            2,
            "the shell must not own a content-region widget that could draw over the active screen"
        );
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
    fn statusline_line_pads_between_segments() {
        let state = StatuslineState {
            left: "mail".to_owned(),
            center: "gg".to_owned(),
            right: "v1".to_owned(),
            ..StatuslineState::default()
        };
        let line = statusline_line(&state, 20);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text.chars().count(), 20);
        assert!(text.starts_with("mail"));
        assert!(text.ends_with("v1"));
        assert!(text.contains("gg"));
    }

    #[test]
    fn tabs_rotate_wraps_both_directions() {
        let mut tabs = Tabs {
            labels: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            active: 0,
        };
        tabs.rotate(1);
        assert_eq!(tabs.active, 1);
        tabs.rotate(-2);
        assert_eq!(tabs.active, 2);
        tabs.rotate(1);
        assert_eq!(tabs.active, 0);
    }
}
