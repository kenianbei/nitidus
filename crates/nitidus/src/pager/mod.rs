//! The message pager screen: fetches the selected message, decodes it
//! through `MessageView`, and renders headers + body over the content
//! region while `Screen::Pager` is active.

mod body;
mod html;
mod ops;
mod render;
mod save;

pub use ops::{dispatch, open_selected, scroll};

use bevy::prelude::*;
use nitidus_mail::message::MessageView;
use nitidus_mail::{AccountId, EnvelopeId, FolderId, JobId};
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{Widget, WidgetLayout};

use self::render::PagerWindow;
use crate::screen::Screen;

pub struct PagerPlugin;

impl Plugin for PagerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PagerState>();
        app.init_resource::<PagerStatus>();
        app.init_resource::<SaveDir>();
        app.add_systems(Startup, spawn_pager);
        app.add_systems(Update, refresh_pager);
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
}

impl PagerState {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn open_id(&self) -> Option<&EnvelopeId> {
        self.open.as_ref().map(|open| &open.id)
    }

    /// Routes a fetched message in; stale fetches are dropped.
    pub fn receive(
        &mut self,
        account: AccountId,
        folder: FolderId,
        id: EnvelopeId,
        job: JobId,
        raw: Vec<u8>,
    ) {
        if self.loading != Some(job) {
            return;
        }
        self.loading = None;
        let view = MessageView::parse(&raw);
        let part = view.default_part().unwrap_or(0);
        self.open = Some(OpenMessage {
            account,
            folder,
            id,
            raw,
            view,
            part,
            show_all_headers: false,
        });
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

#[derive(Component)]
pub struct PagerWidget;

fn spawn_pager(mut commands: Commands) {
    commands.spawn((
        PagerWidget,
        Widget::from_render_fn_with_state(render::render_pager, PagerWindow::default()),
        WidgetLayout::from(layout::content_layout()),
    ));
}

fn refresh_pager(
    pager: Res<PagerState>,
    theme: Res<Theme>,
    screen: Res<Screen>,
    mut status: ResMut<PagerStatus>,
    mut widgets: Query<&mut Widget, With<PagerWidget>>,
) -> Result {
    if !pager.is_changed() && !theme.is_changed() && !screen.is_changed() {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let previous = widget.get_state::<PagerWindow>()?;
    let window = render::build_window(&pager, &theme, *screen == Screen::Pager, previous);
    let part_label = window.part_label.clone();
    widget.set_state(window)?;
    if status.part != part_label {
        status.part = part_label;
    }
    Ok(())
}
