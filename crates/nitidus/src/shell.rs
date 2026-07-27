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
use ratatui_comfy_tabs::{TabNav, TabNavState};

use crate::contacts::ContactsStatus;
use crate::engine::EngineStatus;
use crate::index::IndexStatus;
use crate::pager::PagerStatus;
use crate::router::PendingKeys;
use crate::status::MessageLog;

pub const MAIL_TAB: &str = "mail";
pub const CONTACTS_TAB: &str = "contacts";

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tabs>();
        app.init_resource::<MessageLog>();
        app.init_resource::<IndexStatus>();
        app.init_resource::<ContactsStatus>();
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
            labels: vec![MAIL_TAB.to_owned(), CONTACTS_TAB.to_owned()],
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

    pub fn position_of(&self, label: &str) -> Option<usize> {
        self.labels.iter().position(|candidate| candidate == label)
    }

    pub fn is_contacts(&self) -> bool {
        self.active_label() == CONTACTS_TAB
    }
}

/// Which tab owns the content region. Everything that used to ask
/// `Screen` asks this or `ComposeState::is_active` instead.
pub fn on_contacts(world: &World) -> bool {
    world.get_resource::<Tabs>().is_some_and(Tabs::is_contacts)
}

pub fn switch_tab(world: &mut World, delta: isize) {
    world.resource_mut::<Tabs>().rotate(delta);
    apply_active_tab(world);
}

pub fn activate_tab(world: &mut World, label: &str) {
    let Some(position) = world.resource::<Tabs>().position_of(label) else {
        return;
    };
    world.resource_mut::<Tabs>().active = position;
    apply_active_tab(world);
}

/// `1`/`2`/`:tab <n>` — positional jump, 1-based.
pub fn jump_tab(world: &mut World, position: usize) {
    let count = world.resource::<Tabs>().labels.len();
    if position == 0 || position > count {
        return;
    }
    world.resource_mut::<Tabs>().active = position - 1;
    apply_active_tab(world);
}

/// Leaving the mail tab parks its focus on the message list, so coming
/// back never lands on a pane that is no longer on screen.
fn apply_active_tab(world: &mut World) {
    if world.resource::<Tabs>().is_contacts() {
        crate::focus::focus(world, crate::focus::Pane::Messages);
    }
}

#[derive(Component)]
pub struct TabBar;

#[derive(Clone, Default)]
struct TabBarState {
    labels: Vec<String>,
    active: usize,
    normal: Style,
    highlight: Style,
    border: Style,
    nav: TabNavState,
}

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
        Widget::from_render_fn_with_state(render_tab_bar, TabBarState::default()),
        WidgetLayout::from(layout::tab_bar_layout()),
        plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
            handle_tab_mouse,
        )]),
    ));
    commands.spawn((
        Statusline,
        Widget::from_render_fn_with_state(render_statusline, StatuslineState::default()),
        WidgetLayout::from(layout::statusline_layout()),
    ));
}

/// Tab-bar mouse: a click on a tab box switches to it. The nav is
/// rebuilt with the rendered labels so `tab_index_at` sees the same
/// geometry the strip drew with.
fn handle_tab_mouse(world: &mut World, entity: Entity, event: plurimus::UiEvent) -> Result {
    let Some(local) = crate::mouse::local_event(world, entity, event) else {
        return Ok(());
    };
    if !local.is_left_click() || crate::mouse::is_modal_open(world) {
        return Ok(());
    }
    let Some(rect) = world.get::<plurimus::WidgetRect>(entity).map(|rect| rect.0) else {
        return Ok(());
    };
    let clicked = {
        let Some(state) = world
            .get::<Widget>(entity)
            .and_then(|widget| widget.get_state::<TabBarState>().ok())
        else {
            return Ok(());
        };
        let labels: Vec<&str> = state.labels.iter().map(String::as_str).collect();
        let nav = TabNav::new(&labels, state.active).selection_flash(false);
        nav.tab_index_at(
            rect,
            state.nav.scroll_offset,
            local.raw.column,
            local.raw.row,
        )
    };
    if let Some(index) = clicked {
        jump_tab(world, index + 1);
    }
    Ok(())
}

fn refresh_tab_bar(
    theme: Res<Theme>,
    tabs: Res<Tabs>,
    mut widgets: Query<&mut Widget, With<TabBar>>,
) -> Result {
    if !theme.is_changed() && !tabs.is_changed() {
        return Ok(());
    }
    let states = &theme.base.default;
    for mut widget in &mut widgets {
        let nav = widget.get_state::<TabBarState>()?.nav;
        widget.set_state(TabBarState {
            labels: tabs.labels.clone(),
            active: tabs.active,
            normal: states.normal.style(),
            highlight: states.selected.style(),
            border: states.disabled.style(),
            nav,
        })?;
    }
    Ok(())
}

fn render_tab_bar(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut TabBarState,
) -> Result {
    let labels: Vec<&str> = state.labels.iter().map(String::as_str).collect();
    state.nav.selected = state.active;
    let nav = TabNav::new(&labels, state.active)
        .style(state.normal)
        .highlight_style(state.highlight)
        .border_style(state.border)
        // Flash animation needs repaints plurimus only issues on
        // refresh; a stuck half-flash reads as a broken highlight.
        .selection_flash(false);
    frame.render_stateful_widget(nav, area, &mut state.nav);
    Ok(())
}

#[derive(bevy::ecs::system::SystemParam)]
struct StatuslineInputs<'w> {
    theme: Res<'w, Theme>,
    tabs: Res<'w, Tabs>,
    pending: Res<'w, PendingKeys>,
    status: Res<'w, MessageLog>,
    engine_status: Res<'w, EngineStatus>,
    index_status: Res<'w, IndexStatus>,
    contacts_status: Res<'w, ContactsStatus>,
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
            || self.contacts_status.is_changed()
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
            left: left_segment(&inputs),
            center: center.clone(),
            center_style,
            right: format!("nitidus v{}", env!("CARGO_PKG_VERSION")),
            style: theme.paper.default.normal.style(),
        })?;
    }
    Ok(())
}

fn left_segment(inputs: &StatuslineInputs<'_>) -> String {
    let tabs = &inputs.tabs;
    let (engine_status, index_status) = (&inputs.engine_status, &inputs.index_status);
    let mut segment = tabs.active_label().to_owned();
    if tabs.active_label() == CONTACTS_TAB {
        let contacts = &inputs.contacts_status;
        if contacts.total > 0 {
            segment = format!("{segment} ⋅ {}/{}", contacts.selected, contacts.total);
        }
        return segment;
    }
    if tabs.active_label() != MAIL_TAB {
        return segment;
    }
    if !index_status.folder.is_empty() {
        segment = format!("{segment} ⋅ {}", index_status.folder);
    }
    if !index_status.limits.is_empty() {
        segment = format!("{segment} ⋅ limit: {}", index_status.limits);
    }
    if index_status.marked > 0 {
        segment = format!("{segment} ⋅ {} marked", index_status.marked);
    }
    if let Some(summary) = engine_status.summary() {
        segment = format!("{segment} ⋅ {summary}");
    }
    if index_status.total > 0 {
        segment = format!(
            "{segment} ⋅ {}/{}",
            index_status.selected, index_status.total
        );
        if !index_status.limits.is_empty() {
            segment = format!("{segment} ({})", index_status.folder_total);
        }
    }
    segment
}

fn center_segment(
    pending: &PendingKeys,
    status: &MessageLog,
    pager_status: &PagerStatus,
    theme: &Theme,
) -> (String, Style) {
    if let Some(text) = status.current() {
        return (text.to_owned(), theme.paper.info.normal.style());
    }
    if let Some(hint) = pending.hint() {
        return (hint, theme.paper.default.focused.style());
    }
    if let Some(part) = &pager_status.part {
        return (part.clone(), theme.paper.default.normal.style());
    }
    (String::new(), theme.paper.default.normal.style())
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

    fn tab_switch_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Tabs>();
        app.init_resource::<MessageLog>();
        app.init_resource::<crate::sidebar::SidebarState>();
        app.init_resource::<crate::focus::PaneFocus>();
        app.update();
        app
    }

    #[test]
    fn switching_tabs_parks_the_mail_focus_on_the_message_list() {
        let mut app = tab_switch_app();
        let world = app.world_mut();
        crate::focus::focus(world, crate::focus::Pane::Folders);

        switch_tab(world, 1);
        assert!(world.resource::<Tabs>().is_contacts());
        assert!(
            !crate::focus::is_focused(world, crate::focus::Pane::Folders),
            "the mail sidebar must lose focus when leaving the mail tab"
        );

        switch_tab(world, 1);
        assert!(!world.resource::<Tabs>().is_contacts());
    }

    #[test]
    fn named_activation_jumps_to_the_contacts_tab() {
        let mut app = tab_switch_app();
        let world = app.world_mut();
        activate_tab(world, CONTACTS_TAB);
        assert!(world.resource::<Tabs>().is_contacts());
        assert_eq!(world.resource::<Tabs>().active_label(), CONTACTS_TAB);
        activate_tab(world, "no-such-tab");
        assert_eq!(world.resource::<Tabs>().active_label(), CONTACTS_TAB);
    }

    /// Tabbing away used to be refused outright. Drafts survive the
    /// switch through postpone and recall, so the guard cost more than
    /// it protected.
    #[test]
    fn tabbing_away_mid_composition_is_allowed() {
        let mut app = tab_switch_app();
        let world = app.world_mut();
        world.init_resource::<crate::compose::ComposeState>();

        switch_tab(world, 1);

        assert!(world.resource::<Tabs>().is_contacts());
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
