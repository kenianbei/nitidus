//! Modal overlay surfaces and the rule that orders them.
//!
//! Every modal here takes the keyboard from the screen beneath it. Which
//! one holds it is no longer a hand-written gate order in the router but
//! the top of `surface::OverlayStack` — see that module for the rule. A
//! surface that opens above another must also draw above it, so the
//! stack and the `layer` ladder have to agree.
//!
//! Keyboard stays on the router for every surface (rebindable, no double
//! delivery); plurimus handles the mouse path. Chrome — the cleared
//! region, the frame, its title and hint — comes from
//! `nitidus_ui_kit::surface`, so every modal reads as the same kind of
//! thing.

pub mod confirm;
pub mod form;
pub(crate) mod interaction;
pub mod log;
pub mod picker;
pub mod surface;

pub use confirm::{ConfirmSpec, open_confirm};
pub use picker::{ActiveOverlay, PickerItem, PickerSpec, move_selection, open_picker};

use bevy::prelude::*;

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<surface::OverlayStack>();
        app.add_plugins((
            picker::PickerPlugin,
            form::FormPlugin,
            confirm::ConfirmPlugin,
            log::LogPlugin,
        ));
    }
}
