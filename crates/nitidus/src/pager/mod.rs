//! The message pager screen: fetches the selected message, decodes it
//! through `MessageView`, and renders headers + body over the content
//! region.

pub(crate) mod body;
mod html;
pub(crate) mod ops;
mod render;
mod save;

pub use ops::{dispatch, open_selected, scroll};

use bevy::prelude::*;
use nitidus_mail::message::MessageView;
use nitidus_mail::{AccountId, EnvelopeId, FolderId, JobId};
use nitidus_ui_kit::theme::Theme;
use nitidus_ui_kit::{layer, layout};
use plurimus::{Widget, WidgetLayout, WidgetOrder};

use self::render::PagerWindow;
use crate::config::Config;

pub struct PagerPlugin;

impl Plugin for PagerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PagerState>();
        app.init_resource::<ReadingZoom>();
        app.init_resource::<Config>();
        // The reading column's rect depends on whether the sidebar is
        // taking one.
        app.init_resource::<crate::sidebar::SidebarState>();
        app.init_resource::<PagerStatus>();
        app.init_resource::<SaveDir>();
        app.add_systems(Startup, spawn_pager);
        app.add_systems(Update, (apply_mark_read, apply_zoom, refresh_pager));
    }
}

/// Where `:save-part` writes. A resource so tests (and later, config)
/// can redirect it.
#[derive(Resource)]
pub struct SaveDir(pub std::path::PathBuf);

impl Default for SaveDir {
    fn default() -> Self {
        let downloads = etcetera::home_dir()
            .map(|home| home.join("Downloads"))
            .unwrap_or_else(|_no_home| std::path::PathBuf::from("."));
        Self(downloads)
    }
}

/// Part label for the statusline center while multiple body parts
/// exist (`text/plain 1/2`).
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct PagerStatus {
    pub part: Option<String>,
}

pub struct OpenMessage {
    pub account: AccountId,
    pub folder: FolderId,
    pub id: EnvelopeId,
    pub raw: Vec<u8>,
    pub view: MessageView,
    pub part: usize,
    pub show_all_headers: bool,
}

#[derive(Resource, Default)]
pub struct PagerState {
    open: Option<OpenMessage>,
    loading: Option<JobId>,
    /// A message that just landed and has not been through the
    /// `mark_read` policy yet. Set here because the event router has no
    /// `&mut World` to write flags with.
    arrived: Option<(AccountId, FolderId, EnvelopeId)>,
}

impl PagerState {
    pub fn open_message(&self) -> Option<&OpenMessage> {
        self.open.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn open_id(&self) -> Option<&EnvelopeId> {
        self.open.as_ref().map(|open| &open.id)
    }

    /// Routes a fetched message in; stale fetches are dropped. Returns
    /// whether this one landed.
    pub fn receive(
        &mut self,
        account: AccountId,
        folder: FolderId,
        id: EnvelopeId,
        job: JobId,
        raw: Vec<u8>,
    ) -> bool {
        if self.loading != Some(job) {
            return false;
        }
        self.loading = None;
        let view = MessageView::parse(&raw);
        let part = view.default_part().unwrap_or(0);
        self.arrived = Some((account.clone(), folder.clone(), id.clone()));
        self.open = Some(OpenMessage {
            account,
            folder,
            id,
            raw,
            view,
            part,
            show_all_headers: false,
        });
        true
    }

    /// True when the failed job was the pending fetch — the caller
    /// returns to the index.
    pub fn fail_fetch(&mut self, job: JobId) -> bool {
        if self.loading == Some(job) {
            self.loading = None;
            true
        } else {
            false
        }
    }
}

/// `mark_read = "open"` fires when the fetch lands, not when it is
/// asked for: what you never received, you never read.
fn apply_mark_read(world: &mut World) {
    let Some((account, folder, id)) = world.resource_mut::<PagerState>().arrived.take() else {
        return;
    };
    if world.resource::<Config>().ui.pager.mark_read != crate::config::MarkRead::Open {
        return;
    }
    crate::index::mark_seen(world, &account, &folder, &id);
}

/// Whether the reading pane is drawn zoomed over its neighbours. It is
/// still the same pane holding the same message — only where it draws
/// changes, which is why nothing else in the pager knows about it.
#[derive(Resource, Default)]
pub struct ReadingZoom(bool);

impl ReadingZoom {
    pub fn is_zoomed(&self) -> bool {
        self.0
    }
}

/// From the message list, zooming means "read this one full screen", so
/// it opens the selected message first — the pane is otherwise empty and
/// there is nothing to enlarge. From the reading pane it is a plain
/// toggle over what is already there.
pub fn toggle_zoom(world: &mut World) {
    if world.resource::<ReadingZoom>().0 {
        return unzoom(world);
    }
    if !crate::focus::is_focused(world, crate::focus::Pane::Reading) {
        ops::open_selected(world);
    }
    let has_message = {
        let pager = world.resource::<PagerState>();
        pager.is_open() || pager.is_loading()
    };
    if !has_message {
        return;
    }
    world.resource_mut::<ReadingZoom>().0 = true;
    crate::focus::focus(world, crate::focus::Pane::Reading);
}

pub(crate) fn unzoom(world: &mut World) {
    world.resource_mut::<ReadingZoom>().0 = false;
}

/// Zooming swaps the pane's rect and its rung; a picker opened from the
/// reading pane still draws above it, which is why `layer::ZOOM` sits
/// below every panel rather than at `MODAL`.
fn apply_zoom(
    zoom: Res<ReadingZoom>,
    config: Res<Config>,
    sidebar: Res<crate::sidebar::SidebarState>,
    mut commands: Commands,
    widgets: Query<Entity, With<PagerWidget>>,
) {
    if !(zoom.is_changed() || config.is_changed() || sidebar.is_changed()) {
        return;
    }
    let (layout, order) = if zoom.0 {
        (
            layout::centered_capped_layout(config.ui.pager.max_width, 1),
            layer::ZOOM,
        )
    } else {
        (
            crate::panes::mail_layout(
                crate::panes::MailPane::Reading,
                crate::panes::PaneBudget::new(sidebar.visible, config.ui.index.list_width()),
            ),
            layer::BASE,
        )
    };
    for entity in &widgets {
        commands
            .entity(entity)
            .insert((WidgetLayout::from(layout.clone()), WidgetOrder(order)));
    }
}

#[derive(Component)]
pub struct PagerWidget;

fn spawn_pager(config: Res<crate::config::Config>, mut commands: Commands) {
    commands.spawn((
        PagerWidget,
        Widget::from_render_fn_with_state(render::render_pager, PagerWindow::default()),
        WidgetLayout::from(crate::panes::mail_layout(
            crate::panes::MailPane::Reading,
            crate::panes::PaneBudget::new(true, config.ui.index.list_width()),
        )),
        plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
            ops::handle_mouse,
        )]),
    ));
}

fn refresh_pager(
    pager: Res<PagerState>,
    theme: Res<Theme>,
    tabs: Res<crate::shell::Tabs>,
    zoom: Res<ReadingZoom>,
    mut status: ResMut<PagerStatus>,
    mut widgets: Query<&mut Widget, With<PagerWidget>>,
) -> Result {
    if !pager.is_changed() && !theme.is_changed() && !tabs.is_changed() && !zoom.is_changed() {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let previous = widget.get_state::<PagerWindow>()?;
    let chrome = render::WindowChrome {
        active: !tabs.is_contacts(),
        zoomed: zoom.is_zoomed(),
    };
    let window = render::build_window(&pager, &theme, chrome, previous);
    let part_label = window.part_label.clone();
    widget.set_state(window)?;
    if status.part != part_label {
        status.part = part_label;
    }
    Ok(())
}
