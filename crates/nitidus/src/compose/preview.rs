//! The attachment preview overlay: what the token on the cursor line
//! actually points at.
//!
//! Terminal graphics are negotiated, not guaranteed, and the path may not
//! be an image at all — so the overlay always has something to say. It
//! falls back to the name, size, and path rather than refusing to open.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{Widget, WidgetLayout, WidgetOrder};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui_image::StatefulImage;
use ratatui_image::protocol::StatefulProtocol;

use crate::contacts::PhotoPicker;
use crate::status::StatusMessage;

const PANEL_WIDTH_PCT: u16 = 60;
const PANEL_MAX_ROWS: u16 = 20;
const OVERLAY_ORDER: i32 = 110;
const HINT: &str = " any key closes ";

#[derive(Resource, Default)]
pub struct AttachPreview(Option<Preview>);

#[derive(Clone)]
struct Preview {
    title: String,
    detail: Vec<String>,
    /// `None` when the terminal cannot draw images, or the file is not
    /// one — the detail lines carry the whole story then.
    image: Option<Arc<Mutex<StatefulProtocol>>>,
}

impl AttachPreview {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AttachPreview>();
        app.add_systems(Startup, spawn_preview);
        app.add_systems(Update, refresh_preview);
    }
}

/// Opens the preview for the token on the cursor line.
pub(super) fn open(world: &mut World) {
    let Some(token) = super::inline::token_at_cursor(world) else {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .info("no attachment on this line".to_owned(), now);
        return;
    };
    let preview = build(world, &token.path);
    world.resource_mut::<AttachPreview>().0 = Some(preview);
}

pub fn close(world: &mut World) {
    world.resource_mut::<AttachPreview>().0 = None;
}

/// Any key dismisses; the overlay is a look, not a mode.
pub fn handle_key(world: &mut World, _key: bevy_ratatui::crossterm::event::KeyEvent) -> Result {
    close(world);
    Ok(())
}

fn build(world: &World, path: &std::path::Path) -> Preview {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("attachment")
        .to_owned();
    let mut detail = vec![path.display().to_string()];
    match std::fs::metadata(path) {
        Ok(meta) => detail.push(format!("{} bytes", meta.len())),
        Err(error) => detail.push(format!("unreadable: {error}")),
    }
    let image = decode(world, path);
    if image.is_none() {
        detail.push("no preview available".to_owned());
    }
    Preview {
        title: name,
        detail,
        image,
    }
}

/// Needs both a terminal that negotiated a graphics protocol and a file
/// that actually decodes; either missing degrades to the detail lines.
fn decode(world: &World, path: &std::path::Path) -> Option<Arc<Mutex<StatefulProtocol>>> {
    let picker = world.get_resource::<PhotoPicker>()?.0.as_ref()?;
    let image = image::open(path).ok()?;
    Some(Arc::new(Mutex::new(picker.new_resize_protocol(image))))
}

#[derive(Component)]
struct PreviewWidget;

#[derive(Clone, Default)]
struct PreviewWindow {
    preview: Option<Preview>,
    normal: Style,
}

fn spawn_preview(mut commands: Commands) {
    commands.spawn((
        PreviewWidget,
        Widget::from_render_fn_with_state(render_preview, PreviewWindow::default()),
        WidgetLayout::from(layout::centered_panel_layout(
            PANEL_WIDTH_PCT,
            PANEL_MAX_ROWS,
        )),
        WidgetOrder(OVERLAY_ORDER),
    ));
}

fn refresh_preview(
    preview: Res<AttachPreview>,
    theme: Res<Theme>,
    mut widgets: Query<&mut Widget, With<PreviewWidget>>,
) -> Result {
    if !preview.is_changed() && !theme.is_changed() {
        return Ok(());
    }
    for mut widget in &mut widgets {
        widget.set_state(PreviewWindow {
            preview: preview.0.clone(),
            normal: theme.paper.default.normal.style(),
        })?;
    }
    Ok(())
}

fn render_preview(frame: &mut ratatui::Frame, area: Rect, state: &mut PreviewWindow) -> Result {
    let Some(preview) = &state.preview else {
        return Ok(());
    };
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(format!(" {} ", preview.title))
        .title_bottom(Line::from(HINT).right_aligned())
        .style(state.normal);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let detail_rows = u16::try_from(preview.detail.len()).unwrap_or(u16::MAX);
    let [image_area, detail_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(detail_rows)]).areas(inner);
    if let Some(cell) = &preview.image
        && let Ok(mut protocol) = cell.lock()
    {
        frame.render_stateful_widget(
            StatefulImage::<StatefulProtocol>::new(),
            image_area,
            &mut protocol,
        );
    }
    let lines: Vec<Line<'static>> = preview
        .detail
        .iter()
        .map(|line| Line::from(line.clone()))
        .collect();
    frame.render_widget(Paragraph::new(lines).style(state.normal), detail_area);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_missing_file_still_builds_a_preview() {
        let world = World::new();
        let preview = build(&world, std::path::Path::new("/nowhere/ghost.png"));

        assert_eq!(preview.title, "ghost.png");
        assert!(preview.image.is_none());
        assert!(
            preview
                .detail
                .iter()
                .any(|line| line.contains("unreadable")),
            "an unreadable file must say so: {:?}",
            preview.detail
        );
    }

    #[test]
    fn a_real_file_reports_its_size_and_degrades_without_a_picker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "12345").unwrap();
        let world = World::new();

        let preview = build(&world, &path);

        assert_eq!(preview.title, "notes.txt");
        assert!(
            preview.detail.iter().any(|line| line.contains("5 bytes")),
            "{:?}",
            preview.detail
        );
        assert!(
            preview
                .detail
                .iter()
                .any(|line| line.contains("no preview available")),
            "without terminal graphics the overlay must say so: {:?}",
            preview.detail
        );
    }
}
