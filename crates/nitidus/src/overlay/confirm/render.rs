//! Geometry and drawing for a confirmation. Both the entity layout
//! closures and the renderer call `confirm_geometry`, so click targets
//! can never drift from what was drawn.

use nitidus_ui_kit::layout;
use nitidus_ui_kit::surface::{FrameChrome, draw_frame};
use nitidus_ui_kit::theme::{Theme, ThemeColorStates};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::overlay::interaction::{Interaction, Visual};

const PANEL_WIDTH_PCT: u16 = 50;
/// Two borders, the question, a blank row, and the button row.
const CHROME_ROWS: u16 = 5;
const BUTTON_PAD: u16 = 2;
const BUTTON_GAP: u16 = 1;

pub(super) struct ConfirmGeometry {
    pub(super) frame: Rect,
    pub(super) question: Rect,
    pub(super) detail: Vec<Rect>,
    pub(super) buttons: [Rect; 2],
}

pub(super) fn button_width(labels: &[String; 2]) -> u16 {
    labels
        .iter()
        .map(|label| label.chars().count() as u16)
        .max()
        .map_or(0, |widest| widest + BUTTON_PAD * 2)
}

pub(super) fn confirm_geometry(
    area: Rect,
    detail_rows: usize,
    button_width: u16,
) -> ConfirmGeometry {
    let height = CHROME_ROWS + detail_rows as u16;
    let frame = layout::centered_panel(area, PANEL_WIDTH_PCT, height);
    let inner = inner_area(frame);
    let mut rows = inner.rows();
    let question = rows.next().unwrap_or(Rect::ZERO);
    let detail = (0..detail_rows)
        .map(|_| rows.next().unwrap_or(Rect::ZERO))
        .collect();
    // The blank row separating the context from the buttons.
    rows.next();
    let buttons = rows
        .next()
        .map_or([Rect::ZERO; 2], |row| button_rects(row, button_width));
    ConfirmGeometry {
        frame,
        question,
        detail,
        buttons,
    }
}

/// The frame minus its border. A frame too narrow to hold one would
/// otherwise produce an inner rect outside itself.
fn inner_area(frame: Rect) -> Rect {
    Rect {
        x: frame.x.saturating_add(1).min(frame.right()),
        y: frame.y.saturating_add(1).min(frame.bottom()),
        width: frame.width.saturating_sub(2),
        height: frame.height.saturating_sub(2),
    }
}

/// Right-aligned and uniform, Cancel then the affirmative — so the
/// destructive button is furthest from where the eye starts.
fn button_rects(row: Rect, width: u16) -> [Rect; 2] {
    let total = width * 2 + BUTTON_GAP;
    let start = row.right().saturating_sub(total).max(row.x);
    [0, 1].map(|index| Rect {
        x: start + index * (width + BUTTON_GAP),
        y: row.y,
        width,
        height: 1,
    })
}

#[derive(Clone)]
pub(super) struct FrameView {
    pub(super) title: String,
    pub(super) question: String,
    pub(super) detail: Vec<String>,
    pub(super) detail_rows: usize,
    pub(super) button_width: u16,
    pub(super) surface: ratatui::style::Style,
    pub(super) dim: ratatui::style::Style,
}

impl FrameView {
    pub(super) fn new(
        title: String,
        question: String,
        detail: Vec<String>,
        button_width: u16,
        theme: &Theme,
    ) -> Self {
        Self {
            title,
            detail_rows: detail.len(),
            question,
            detail,
            button_width,
            surface: theme.paper.default.normal.style(),
            dim: theme.paper.default.disabled.style(),
        }
    }
}

pub(super) fn render_frame(
    frame: &mut ratatui::Frame,
    area: Rect,
    view: &mut FrameView,
) -> bevy::prelude::Result {
    draw_frame(
        frame.buffer_mut(),
        area,
        FrameChrome {
            title: &view.title,
            hint: None,
            style: view.surface,
        },
    );
    let geometry = confirm_geometry(area, view.detail_rows, view.button_width);
    frame.render_widget(
        Paragraph::new(view.question.as_str()).style(view.surface),
        geometry.question,
    );
    for (line, rect) in view.detail.iter().zip(&geometry.detail) {
        frame.render_widget(Paragraph::new(line.as_str()).style(view.dim), *rect);
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct ButtonView {
    pub(super) label: String,
    pub(super) focused: bool,
    pub(super) interaction: Interaction,
    states: ThemeColorStates,
}

impl ButtonView {
    pub(super) fn new(label: String, theme: &Theme) -> Self {
        Self {
            label,
            focused: false,
            interaction: Interaction::default(),
            states: theme.paper.default,
        }
    }
}

pub(super) fn render_button(
    frame: &mut ratatui::Frame,
    area: Rect,
    view: &mut ButtonView,
) -> bevy::prelude::Result {
    let visual = Visual::resolve(view.focused, view.interaction);
    let mut style = visual.colors(&view.states).style();
    if view.focused {
        style = style.add_modifier(Modifier::BOLD);
    }
    let width = usize::from(area.width);
    let label: String = view.label.chars().take(width).collect();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!("{label:^width$}"), style))),
        area,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn the_affirmative_button_sits_right_of_cancel() {
        let geometry = confirm_geometry(Rect::new(0, 0, 100, 40), 0, 10);
        let [cancel, affirm] = geometry.buttons;

        assert_eq!(cancel.width, affirm.width, "uniform width");
        assert!(cancel.x < affirm.x, "Cancel comes first");
        assert!(
            affirm.right() <= geometry.frame.right(),
            "buttons stay inside the border"
        );
    }

    #[test]
    fn detail_rows_stack_between_the_question_and_the_buttons() {
        let geometry = confirm_geometry(Rect::new(0, 0, 100, 40), 2, 10);

        assert_eq!(geometry.detail.len(), 2);
        assert_eq!(geometry.detail[0].y, geometry.question.y + 1);
        assert_eq!(geometry.detail[1].y, geometry.detail[0].y + 1);
        assert!(geometry.buttons[0].y > geometry.detail[1].y);
    }

    #[test]
    fn a_taller_question_grows_the_frame_rather_than_overflowing_it() {
        let short = confirm_geometry(Rect::new(0, 0, 100, 40), 0, 10);
        let tall = confirm_geometry(Rect::new(0, 0, 100, 40), 3, 10);

        assert_eq!(tall.frame.height, short.frame.height + 3);
        assert!(tall.buttons[0].bottom() <= tall.frame.bottom());
    }

    #[test]
    fn nothing_escapes_a_terminal_too_small_to_hold_the_frame() {
        let area = Rect::new(0, 0, 8, 4);
        let geometry = confirm_geometry(area, 2, 10);

        assert!(geometry.frame.right() <= area.right());
        assert!(geometry.frame.bottom() <= area.bottom());
        for button in &geometry.buttons {
            assert!(button.right() <= area.right(), "{button:?}");
        }
    }
}
