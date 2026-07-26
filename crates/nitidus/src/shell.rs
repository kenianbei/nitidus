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
use crate::screen::{MailScreenMemory, Screen};
use crate::status::{Severity, StatusMessage};

pub const MAIL_TAB: &str = "mail";
pub const CONTACTS_TAB: &str = "contacts";

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tabs>();
        app.init_resource::<MailScreenMemory>();
        app.init_resource::<StatusMessage>();
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
}

/// Rotates tabs and drives `Screen` to the new tab's owner.
pub fn switch_tab(world: &mut World, delta: isize) {
    if refuse_while_composing(world) {
        return;
    }
    world.resource_mut::<Tabs>().rotate(delta);
    apply_active_tab(world);
}

/// Jumps to a named tab (`:contacts`) from anywhere but the composer.
pub fn activate_tab(world: &mut World, label: &str) {
    if refuse_while_composing(world) {
        return;
    }
    let Some(position) = world.resource::<Tabs>().position_of(label) else {
        return;
    };
    world.resource_mut::<Tabs>().active = position;
    apply_active_tab(world);
}

/// `1`/`2`/`:tab <n>` — positional jump, 1-based.
pub fn jump_tab(world: &mut World, position: usize) {
    if refuse_while_composing(world) {
        return;
    }
    let count = world.resource::<Tabs>().labels.len();
    if position == 0 || position > count {
        return;
    }
    world.resource_mut::<Tabs>().active = position - 1;
    apply_active_tab(world);
}

/// The composer stays modal until sent, postponed, or discarded —
/// tabbing away would orphan an open editing session.
fn refuse_while_composing(world: &mut World) -> bool {
    if *world.resource::<Screen>() != Screen::Compose {
        return false;
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<StatusMessage>().warn(
        "finish or discard the composition before switching tabs".to_owned(),
        now,
    );
    true
}

fn apply_active_tab(world: &mut World) {
    let label = world.resource::<Tabs>().active_label().to_owned();
    let current = *world.resource::<Screen>();
    if label == CONTACTS_TAB && current != Screen::Contacts {
        world.resource_mut::<MailScreenMemory>().0 = current;
        world.resource_mut::<crate::sidebar::SidebarState>().focused = false;
        *world.resource_mut::<Screen>() = Screen::Contacts;
    } else if label == MAIL_TAB && current == Screen::Contacts {
        *world.resource_mut::<Screen>() = world.resource::<MailScreenMemory>().0;
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
    status: Res<'w, StatusMessage>,
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
        app.init_resource::<Screen>();
        app.init_resource::<MailScreenMemory>();
        app.init_resource::<StatusMessage>();
        app.init_resource::<crate::sidebar::SidebarState>();
        app.update();
        app
    }

    #[test]
    fn tab_switch_drives_screen_and_restores_the_mail_screen() {
        let mut app = tab_switch_app();
        let world = app.world_mut();
        *world.resource_mut::<Screen>() = Screen::Pager;
        world.resource_mut::<crate::sidebar::SidebarState>().focused = true;

        switch_tab(world, 1);
        assert_eq!(*world.resource::<Screen>(), Screen::Contacts);
        assert!(
            !world.resource::<crate::sidebar::SidebarState>().focused,
            "the mail sidebar must lose focus when leaving the mail tab"
        );

        switch_tab(world, 1);
        assert_eq!(
            *world.resource::<Screen>(),
            Screen::Pager,
            "returning to the mail tab must restore the screen it left"
        );
    }

    #[test]
    fn named_activation_jumps_to_the_contacts_tab() {
        let mut app = tab_switch_app();
        let world = app.world_mut();
        activate_tab(world, CONTACTS_TAB);
        assert_eq!(*world.resource::<Screen>(), Screen::Contacts);
        assert_eq!(world.resource::<Tabs>().active_label(), CONTACTS_TAB);
        activate_tab(world, "no-such-tab");
        assert_eq!(world.resource::<Tabs>().active_label(), CONTACTS_TAB);
    }

    #[test]
    fn composing_refuses_tab_switches_with_a_notice() {
        let mut app = tab_switch_app();
        let world = app.world_mut();
        *world.resource_mut::<Screen>() = Screen::Compose;
        switch_tab(world, 1);
        assert_eq!(*world.resource::<Screen>(), Screen::Compose);
        assert_eq!(world.resource::<Tabs>().active, 0);
        assert!(
            world.resource::<StatusMessage>().current().is_some(),
            "refusal must be explained in the statusline"
        );
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
