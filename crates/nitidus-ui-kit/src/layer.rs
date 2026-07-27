//! The elevation ladder, defined once. Every `WidgetOrder` in the app
//! comes from here, so "what draws above what" is a stated rule rather
//! than a set of magic numbers scattered across modules.
//!
//! Gaps between rungs leave room for a surface that must sit between two
//! named layers without renumbering the ladder.

/// Screen content: the index, pager, sidebar, compose review.
pub const BASE: i32 = 0;

/// A base pane drawn over its neighbours: the reading overlay. Still
/// screen content, so every panel and overlay covers it in turn — a
/// picker opened from the reading pane has to stay visible.
pub const ZOOM: i32 = 10;

/// Attached to a base surface and dismissed with it: completion panels
/// above the command line and the prompt.
pub const PANEL: i32 = 90;

/// Takes the keyboard from the screen beneath it: pickers, forms, the
/// file explorer.
pub const OVERLAY: i32 = 100;

/// Opens above an overlay: previews and confirmations.
pub const MODAL: i32 = 110;

/// Notifications, which must never be occluded.
pub const TOAST: i32 = 120;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn ladder_ascends() {
        let rungs = [BASE, ZOOM, PANEL, OVERLAY, MODAL, TOAST];
        for pair in rungs.windows(2) {
            assert!(
                pair[0] < pair[1],
                "elevation must ascend: {} is not below {}",
                pair[0],
                pair[1]
            );
        }
    }
}
