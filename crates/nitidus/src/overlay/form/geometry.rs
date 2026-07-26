//! Where every part of a form lands. Pure functions, called by both the
//! entities' layout fns and the renderers, so a click can never land
//! somewhere the drawing did not.

use nitidus_ui_kit::layout;
use ratatui::layout::Rect;

const PANEL_WIDTH_PCT: u16 = 60;
/// Border rows, the blank above the message, the message, the buttons.
const CHROME_ROWS: u16 = 5;
/// Padding either side of a step's title in the strip.
const STEP_PAD: u16 = 1;
const STEP_GAP: u16 = 1;
pub(super) const LABEL_WIDTH: u16 = 18;
/// Padding either side of a button's label, inside its brackets.
const BUTTON_PAD: u16 = 2;
const BUTTON_GAP: u16 = 1;

/// The shape a form's layout depends on. Cheap and `Copy` so each
/// entity's layout closure can capture it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FormMetrics {
    pub(super) field_count: usize,
    pub(super) button_count: usize,
    pub(super) button_width: u16,
    /// Multi-page forms reserve a row for the step strip; single-page
    /// ones keep the tighter frame they had before pages existed.
    pub(super) has_strip: bool,
}

pub(super) fn button_width(labels: &[String]) -> u16 {
    labels
        .iter()
        .map(|label| label.chars().count() as u16)
        .max()
        .map_or(0, |widest| widest + BUTTON_PAD * 2)
}

pub(super) struct FormGeometry {
    pub(super) frame: Rect,
    pub(super) strip: Rect,
    pub(super) fields: Vec<Rect>,
    pub(super) message: Rect,
    pub(super) buttons: Vec<Rect>,
}

pub(super) fn form_geometry(area: Rect, metrics: FormMetrics) -> FormGeometry {
    let strip_rows = u16::from(metrics.has_strip);
    let height = metrics.field_count as u16 + CHROME_ROWS + strip_rows;
    let frame = layout::centered_panel(area, PANEL_WIDTH_PCT, height);
    let inner = inner_area(frame);
    let mut rows = inner.rows();
    let strip = if metrics.has_strip {
        rows.next().unwrap_or(Rect::ZERO)
    } else {
        Rect::ZERO
    };
    let fields = (0..metrics.field_count)
        .map(|_| rows.next().unwrap_or(Rect::ZERO))
        .collect();
    // The blank row separating the fields from the message.
    rows.next();
    let message = rows.next().unwrap_or(Rect::ZERO);
    let buttons = rows
        .next()
        .map_or_else(Vec::new, |row| button_rects(row, metrics));
    FormGeometry {
        frame,
        strip,
        fields,
        message,
        buttons,
    }
}

/// A step's box is its title plus a space either side; the strip runs
/// left to right and drops whatever overflows rather than wrapping.
pub(super) fn step_rects(strip: &Rect, widths: &[u16]) -> Vec<Rect> {
    let mut x = strip.x;
    let mut rects = Vec::with_capacity(widths.len());
    for width in widths {
        let width = width + STEP_PAD * 2;
        if x + width > strip.right() {
            rects.push(Rect::ZERO);
            continue;
        }
        rects.push(Rect {
            x,
            y: strip.y,
            width,
            height: 1,
        });
        x += width + STEP_GAP;
    }
    rects
}

pub(super) fn step_widths(titles: &[String]) -> Vec<u16> {
    titles
        .iter()
        .map(|title| title.chars().count() as u16)
        .collect()
}

/// The frame minus its border. A frame too small to hold one collapses
/// to nothing at its own origin rather than stepping outside itself.
fn inner_area(frame: Rect) -> Rect {
    if frame.width < 2 || frame.height < 2 {
        return Rect {
            x: frame.x,
            y: frame.y,
            width: 0,
            height: 0,
        };
    }
    Rect {
        x: frame.x + 1,
        y: frame.y + 1,
        width: frame.width - 2,
        height: frame.height - 2,
    }
}

/// Buttons sit right-aligned on their row, uniformly wide so the row
/// reads as a group rather than a ragged edge.
fn button_rects(row: Rect, metrics: FormMetrics) -> Vec<Rect> {
    let count = metrics.button_count as u16;
    if count == 0 {
        return Vec::new();
    }
    let span = count * metrics.button_width + count.saturating_sub(1) * BUTTON_GAP;
    if span > row.width {
        return Vec::new();
    }
    let start = row.x + row.width - span;
    (0..count)
        .map(|index| Rect {
            x: start + index * (metrics.button_width + BUTTON_GAP),
            y: row.y,
            width: metrics.button_width,
            height: 1,
        })
        .collect()
}

/// The value box of a field row: everything right of the label column.
pub(super) fn value_area(row: Rect) -> Rect {
    let label = LABEL_WIDTH.min(row.width);
    Rect {
        x: row.x.saturating_add(label),
        y: row.y,
        width: row.width.saturating_sub(label),
        height: row.height,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn metrics(field_count: usize) -> FormMetrics {
        FormMetrics {
            field_count,
            button_count: 2,
            button_width: 10,
            has_strip: false,
        }
    }

    #[test]
    fn rows_stack_without_overlapping_and_stay_inside_the_frame() {
        let geometry = form_geometry(Rect::new(0, 0, 100, 40), metrics(3));
        assert_eq!(geometry.fields.len(), 3);
        for pair in geometry.fields.windows(2) {
            assert_eq!(pair[1].y, pair[0].y + 1, "field rows must stack");
        }
        assert!(
            geometry.message.y > geometry.fields[2].y,
            "the message sits below the fields"
        );
        assert!(geometry.buttons[0].y > geometry.message.y);
        assert!(
            geometry.buttons[1].bottom() < geometry.frame.bottom(),
            "buttons stay inside the border"
        );
        for field in &geometry.fields {
            assert!(field.x > geometry.frame.x, "fields clear the left border");
            assert!(field.right() < geometry.frame.right());
        }
    }

    #[test]
    fn buttons_are_right_aligned_and_uniform() {
        let geometry = form_geometry(Rect::new(0, 0, 100, 40), metrics(2));
        let [cancel, primary] = geometry.buttons[..] else {
            panic!("expected two buttons, got {:?}", geometry.buttons);
        };
        assert_eq!(cancel.width, primary.width, "uniform width");
        assert_eq!(primary.x, cancel.right() + BUTTON_GAP);
        let inner_right = geometry.frame.right() - 1;
        assert_eq!(primary.right(), inner_right, "flush with the inner edge");
    }

    #[test]
    fn a_narrow_frame_drops_buttons_rather_than_overflowing() {
        let wide = FormMetrics {
            field_count: 1,
            button_count: 2,
            button_width: 60,
            has_strip: false,
        };
        let geometry = form_geometry(Rect::new(0, 0, 60, 30), wide);
        assert!(
            geometry.buttons.is_empty(),
            "buttons that cannot fit must not be placed off-frame"
        );
    }

    #[test]
    fn a_terminal_too_small_for_the_form_still_stays_in_bounds() {
        for height in 0..8 {
            for width in 0..12 {
                let area = Rect::new(0, 0, width, height);
                let geometry = form_geometry(area, metrics(4));
                let placed = geometry
                    .fields
                    .iter()
                    .chain(geometry.buttons.iter())
                    .chain(std::iter::once(&geometry.message));
                for rect in placed {
                    assert!(
                        rect.right() <= area.width && rect.bottom() <= area.height,
                        "{rect:?} escaped a {width}x{height} terminal"
                    );
                }
            }
        }
    }

    #[test]
    fn value_area_starts_after_the_label_column() {
        let row = Rect::new(4, 9, 60, 1);
        let value = value_area(row);
        assert_eq!(value.x, row.x + LABEL_WIDTH);
        assert_eq!(value.width, row.width - LABEL_WIDTH);
        let cramped = value_area(Rect::new(0, 0, 4, 1));
        assert_eq!(cramped.width, 0, "a row narrower than the label column");
    }

    #[test]
    fn a_strip_costs_one_row_and_pushes_the_fields_down() {
        let area = Rect::new(0, 0, 100, 40);
        let plain = form_geometry(area, metrics(2));
        let stripped = form_geometry(
            area,
            FormMetrics {
                has_strip: true,
                ..metrics(2)
            },
        );
        assert_eq!(plain.strip, Rect::ZERO, "no strip without pages");
        assert_eq!(stripped.frame.height, plain.frame.height + 1);
        assert_eq!(stripped.strip.y, stripped.frame.y + 1, "inside the border");
        assert_eq!(stripped.fields[0].y, stripped.strip.y + 1);
    }

    #[test]
    fn steps_lay_out_left_to_right_without_overlapping() {
        let strip = Rect::new(2, 5, 40, 1);
        let rects = step_rects(
            &strip,
            &step_widths(&["Account".to_owned(), "Servers".to_owned()]),
        );
        assert_eq!(rects[0].x, strip.x);
        assert_eq!(rects[0].width, 7 + STEP_PAD * 2);
        assert_eq!(rects[1].x, rects[0].right() + STEP_GAP);
        assert!(rects[1].right() <= strip.right());
    }

    #[test]
    fn a_step_that_would_overflow_the_strip_gets_no_box() {
        let strip = Rect::new(0, 0, 12, 1);
        let rects = step_rects(
            &strip,
            &step_widths(&["Account".to_owned(), "Servers".to_owned()]),
        );
        assert_eq!(rects[0].width, 9);
        assert_eq!(rects[1], Rect::ZERO, "unclickable rather than off-frame");
    }

    #[test]
    fn button_width_follows_the_widest_label() {
        let labels = ["Cancel".to_owned(), "Create".to_owned()];
        assert_eq!(button_width(&labels), 6 + BUTTON_PAD * 2);
        assert_eq!(button_width(&[]), 0);
    }
}
