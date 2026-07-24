//! Modal overlay infrastructure. The picker entity spawns/despawns with
//! the `ActiveOverlay` resource (the vcard_tui pattern) and draws above
//! every screen via `WidgetOrder`; plurimus focus components track it,
//! but keyboard input stays on the router — resolved against the
//! rebindable `picker` context, with unbound printables typing into the
//! filter (so global bindings never leak through a modal).

mod render;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crokey::KeyCombination;
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};
use plurimus::{UiFocusMessage, UiFocusable, Widget, WidgetLayout, WidgetOrder};

use self::render::{PickerRow, PickerWindow};
use crate::action::{Motion, apply_action};
use crate::keymap::{CONTEXT_PICKER, KeymapMatch, Keymaps};

const OVERLAY_ORDER: i32 = 100;
const PANEL_WIDTH_PCT: u16 = 50;
const PANEL_MAX_HEIGHT: u16 = 16;
const PICKER_PAGE_ROWS: usize = 8;

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveOverlay>();
        // Idempotent with PlurimusUiPlugin's registration; keeps this
        // plugin usable in headless test apps.
        app.add_message::<UiFocusMessage>();
        app.add_systems(Update, (sync_picker_entity, refresh_picker).chain());
    }
}

type SelectFn = Box<dyn Fn(&mut World, usize) + Send + Sync>;

pub struct PickerSpec {
    pub title: String,
    pub items: Vec<PickerItem>,
    /// Receives the *original* item index of the confirmed entry, after
    /// the overlay has closed (so it may open another).
    pub on_select: SelectFn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerItem {
    pub label: String,
    pub detail: Option<String>,
}

pub struct PickerState {
    title: String,
    items: Vec<PickerItem>,
    filter: String,
    matches: Vec<u32>,
    selected: usize,
    on_select: SelectFn,
}

impl PickerState {
    fn new(spec: PickerSpec) -> Self {
        let mut state = Self {
            title: spec.title,
            items: spec.items,
            filter: String::new(),
            matches: Vec::new(),
            selected: 0,
            on_select: spec.on_select,
        };
        state.rematch();
        state
    }

    /// Empty filter keeps the caller's order; otherwise nucleo ranks.
    fn rematch(&mut self) {
        if self.filter.is_empty() {
            self.matches = (0..self.items.len() as u32).collect();
        } else {
            let mut matcher = Matcher::new(MatcherConfig::DEFAULT);
            let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
            let mut buffer = Vec::new();
            let mut scored: Vec<(u32, u32)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let haystack = Utf32Str::new(&item.label, &mut buffer);
                    pattern
                        .score(haystack, &mut matcher)
                        .map(|score| (score, index as u32))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.matches = scored.into_iter().map(|(_, index)| index).collect();
        }
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
    }

    fn selected_item(&self) -> Option<usize> {
        self.matches.get(self.selected).map(|&index| index as usize)
    }

    fn edit_filter(&mut self, edit: impl FnOnce(&mut String)) {
        edit(&mut self.filter);
        self.rematch();
    }
}

#[derive(Resource, Default)]
pub struct ActiveOverlay(Option<PickerState>);

impl ActiveOverlay {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

pub fn open_picker(world: &mut World, spec: PickerSpec) {
    world.resource_mut::<ActiveOverlay>().0 = Some(PickerState::new(spec));
}

pub fn close(world: &mut World) {
    world.resource_mut::<ActiveOverlay>().0 = None;
}

pub fn confirm(world: &mut World) {
    let Some(picker) = world.resource_mut::<ActiveOverlay>().0.take() else {
        return;
    };
    if let Some(index) = picker.selected_item() {
        (picker.on_select)(world, index);
    }
}

pub fn move_selection(world: &mut World, motion: Motion) {
    let mut overlay = world.resource_mut::<ActiveOverlay>();
    let Some(picker) = overlay.0.as_mut() else {
        return;
    };
    picker.selected = crate::index::apply_motion(
        picker.selected,
        picker.matches.len(),
        PICKER_PAGE_ROWS,
        motion,
    );
}

/// Exact single-key `picker` bindings win; everything else printable is
/// filter text. No chord waits and no global fallback, by design.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let outcome = {
        let keymaps = world.resource::<Keymaps>();
        keymaps.lookup(CONTEXT_PICKER, &[KeyCombination::from(key)])
    };
    if let KeymapMatch::Exact(action) = outcome {
        apply_action(world, &action);
        return Ok(());
    }
    let mut overlay = world.resource_mut::<ActiveOverlay>();
    let Some(picker) = overlay.0.as_mut() else {
        return Ok(());
    };
    match key.code {
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            picker.edit_filter(|filter| filter.push(character));
        }
        KeyCode::Backspace => picker.edit_filter(|filter| {
            filter.pop();
        }),
        _ => {}
    }
    Ok(())
}

#[derive(Component)]
struct PickerWidget;

fn sync_picker_entity(
    mut commands: Commands,
    overlay: Res<ActiveOverlay>,
    existing: Query<Entity, With<PickerWidget>>,
    mut focus: MessageWriter<UiFocusMessage>,
) {
    if !overlay.is_changed() {
        return;
    }
    match (overlay.is_open(), existing.single()) {
        (true, Err(_)) => {
            let entity = commands
                .spawn((
                    PickerWidget,
                    Widget::from_render_fn_with_state(render::render_picker, PickerWindow::default()),
                    WidgetLayout::from(layout::centered_panel_layout(
                        PANEL_WIDTH_PCT,
                        PANEL_MAX_HEIGHT,
                    )),
                    WidgetOrder(OVERLAY_ORDER),
                    UiFocusable::new(0),
                ))
                .id();
            focus.write(UiFocusMessage::set(entity));
        }
        (false, Ok(entity)) => {
            commands.entity(entity).despawn();
            focus.write(UiFocusMessage::clear());
        }
        _ => {}
    }
}

fn refresh_picker(
    overlay: Res<ActiveOverlay>,
    theme: Res<Theme>,
    mut widgets: Query<&mut Widget, With<PickerWidget>>,
) -> Result {
    if !overlay.is_changed() && !theme.is_changed() {
        return Ok(());
    }
    let (Some(picker), Ok(mut widget)) = (&overlay.0, widgets.single_mut()) else {
        return Ok(());
    };
    let rows = picker
        .matches
        .iter()
        .enumerate()
        .map(|(row, &index)| {
            let item = &picker.items[index as usize];
            PickerRow {
                label: item.label.clone(),
                detail: item.detail.clone(),
                selected: row == picker.selected,
            }
        })
        .collect();
    widget.set_state(PickerWindow::new(picker, rows, &theme))?;
    Ok(())
}
