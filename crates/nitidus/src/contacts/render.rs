//! Rendering for the contact book: table pane (name / email / phone /
//! org) beside a detail pane, one widget owning the whole content
//! region. Viewport heights feed back through the widget state.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use nitidus_ui_kit::theme::Theme;
use plurimus::Widget;
use ratatui::style::{Modifier, Style};

use super::photo::{self, PhotoCell, PhotoPicker};
use super::view::ContactsView;
use super::{ContactStore, ContactsStatus, ContactsWidget, detail};
use crate::focus::{Pane, PaneFocus};
use crate::index::scrolled_top;
use crate::shell::Tabs;

pub(super) const TABLE_PANE_PERCENT: u16 = 45;
pub(super) const DETAIL_LABEL_WIDTH: usize = 18;
/// Column shares of the table pane width: name, email, phone, org.
pub(super) const COLUMN_SHARES: [u16; 4] = [30, 35, 15, 20];

#[derive(Clone, Default)]
pub(super) struct ContactsWindow {
    pub(super) active: bool,
    pub(super) table_rows: Vec<TableRow>,
    pub(super) detail_rows: Vec<DetailLine>,
    pub(super) empty_message: Option<String>,
    pub(super) detail_focused: bool,
    pub(super) table_top: usize,
    pub(super) detail_top: usize,
    pub(super) styles: PaneStyles,
    pub(super) photo: Option<PhotoCell>,
    pub(super) table_height: u16,
    pub(super) detail_height: u16,
}

#[derive(Clone)]
pub(super) struct TableRow {
    pub(super) cells: [String; 4],
    pub(super) is_selected: bool,
}

#[derive(Clone)]
pub(super) struct DetailLine {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) is_selected: bool,
    pub(super) is_modeled: bool,
}

#[derive(Clone, Default)]
pub(super) struct PaneStyles {
    pub(super) normal: Style,
    pub(super) selected: Style,
    pub(super) unfocused_selected: Style,
    pub(super) label: Style,
    pub(super) dim: Style,
}

/// The presentation context every contact-book frame reads: how to
/// paint, whether the tab is on screen, and which pane holds focus.
#[derive(SystemParam)]
pub(super) struct Chrome<'w> {
    theme: Res<'w, Theme>,
    tabs: Res<'w, Tabs>,
    focus: Res<'w, PaneFocus>,
}

impl Chrome<'_> {
    fn is_changed(&self) -> bool {
        self.theme.is_changed() || self.tabs.is_changed() || self.focus.is_changed()
    }
}

pub(super) fn refresh_contacts(
    chrome: Chrome,
    store: Res<ContactStore>,
    picker: Option<Res<PhotoPicker>>,
    mut view: ResMut<ContactsView>,
    mut status: ResMut<ContactsStatus>,
    mut widgets: Query<&mut Widget, With<ContactsWidget>>,
) -> Result {
    if !(chrome.is_changed() || store.is_changed() || view.is_changed()) {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let previous = widget.get_state::<ContactsWindow>()?;
    let (table_height, detail_height) = (previous.table_height, previous.detail_height);
    let previous_photo = previous.photo.clone();
    // Derived scroll/viewport bookkeeping must not re-trigger this
    // system every frame.
    let cached = view.bypass_change_detection();
    clamp_selection(cached, &store);
    cached.table_viewport = usize::from(table_height).max(1);
    cached.detail_viewport = usize::from(detail_height).max(1);
    cached.table_top = scrolled_top(cached.table_top, cached.selected, cached.table_viewport);
    cached.detail_top = scrolled_top(
        cached.detail_top,
        cached.detail_selected,
        cached.detail_viewport,
    );
    let detail_focused = chrome.focus.is(Pane::ContactDetail);
    let mut window = build_window(&chrome.theme, &store, cached, detail_focused);
    window.photo = current_photo(&picker, &store, cached, previous_photo.as_ref());
    window.active = chrome.tabs.is_contacts();
    window.table_height = table_height;
    window.detail_height = detail_height;
    widget.set_state(window)?;
    let position = ContactsStatus {
        selected: if store.0.is_empty() {
            0
        } else {
            cached.selected + 1
        },
        total: store.0.len(),
    };
    if *status != position {
        *status = position;
    }
    Ok(())
}

fn clamp_selection(view: &mut ContactsView, store: &ContactStore) {
    let last = store.0.len().saturating_sub(1);
    view.selected = view.selected.min(last);
    let detail_last = store
        .0
        .get(view.selected)
        .map_or(0, |contact| detail::build_rows(contact).len())
        .saturating_sub(1);
    view.detail_selected = view.detail_selected.min(detail_last);
}

fn build_window(
    theme: &Theme,
    store: &ContactStore,
    view: &ContactsView,
    detail_focused: bool,
) -> ContactsWindow {
    let states = &theme.base.default;
    let styles = PaneStyles {
        normal: states.normal.style(),
        selected: states.selected.style(),
        unfocused_selected: states.selected.style().add_modifier(Modifier::DIM),
        label: theme.base.info.normal.style(),
        dim: states.disabled.style(),
    };
    let empty_message = store
        .0
        .is_empty()
        .then(|| "contact book is empty".to_owned());
    let table_rows = store
        .0
        .iter()
        .enumerate()
        .map(|(row, contact)| TableRow {
            cells: [
                contact.display_name().to_owned(),
                contact.primary_email().unwrap_or_default().to_owned(),
                contact.primary_phone().unwrap_or_default().to_owned(),
                contact.organization().unwrap_or_default().to_owned(),
            ],
            is_selected: row == view.selected,
        })
        .collect();
    let detail_rows = store.0.get(view.selected).map_or_else(Vec::new, |contact| {
        detail::build_rows(contact)
            .iter()
            .enumerate()
            .map(|(row, detail_row)| DetailLine {
                label: detail_row.label.clone(),
                value: detail_row.value.clone(),
                is_selected: row == view.detail_selected,
                is_modeled: detail_row.modeled,
            })
            .collect()
    });
    ContactsWindow {
        active: false,
        table_rows,
        detail_rows,
        empty_message,
        detail_focused,
        table_top: view.table_top,
        detail_top: view.detail_top,
        styles,
        photo: None,
        table_height: 0,
        detail_height: 0,
    }
}

fn current_photo(
    picker: &Option<Res<PhotoPicker>>,
    store: &ContactStore,
    view: &ContactsView,
    previous: Option<&PhotoCell>,
) -> Option<PhotoCell> {
    let picker = picker.as_ref().and_then(|resource| resource.0.as_ref())?;
    let contact = store.0.get(view.selected)?;
    photo::photo_cell(picker, contact, previous)
}
