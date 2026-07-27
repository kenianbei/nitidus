//! Which style each classified body line is drawn in. The pass that
//! applies them lives with the widget, in `overlay::form::body`.

use nitidus_ui_kit::theme::Theme;
use ratatui::style::Style;

use crate::pager::body::LineKind;

/// The style a body line is drawn in, or `None` to leave it alone.
pub(crate) fn line_style(kind: LineKind, theme: &Theme) -> Option<Style> {
    match kind {
        LineKind::Normal => None,
        LineKind::Quote(_) | LineKind::Signature => Some(theme.base.default.disabled.style()),
    }
}
