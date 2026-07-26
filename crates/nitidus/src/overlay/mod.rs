//! Modal overlay surfaces and the rule that orders them.
//!
//! Every modal here takes the keyboard from the screen beneath it, and
//! the router checks their gates outermost-first. A surface that can
//! open above another must also draw above it — the gate order and the
//! `layer` ladder are two views of one stacking rule, and they have to
//! agree. Pickers and forms both sit at `layer::OVERLAY`; they never
//! coexist, so their relative order is undefined by design.
//!
//! Keyboard stays on the router for every surface (rebindable, no double
//! delivery); plurimus handles the mouse path.

pub mod form;
pub mod picker;

pub use picker::{
    ActiveOverlay, PickerItem, PickerSpec, close, confirm, handle_key, move_selection, open_picker,
};

use bevy::prelude::*;

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((picker::PickerPlugin, form::FormPlugin));
    }
}
