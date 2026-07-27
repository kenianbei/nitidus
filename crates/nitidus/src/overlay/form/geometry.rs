//! Where every part of a form lands. Pure functions, called by both the
//! entities' layout fns and the renderers, so a click can never land
//! somewhere the drawing did not.

use nitidus_ui_kit::layout;
use ratatui::layout::Rect;

use super::spec::{FieldHeight, FormPlacement};

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

/// The shape a form's layout depends on, cloned into each entity's
/// layout closure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FormMetrics {
    pub(super) fields: Vec<FieldHeight>,
    pub(super) button_count: usize,
    pub(super) button_width: u16,
    /// Multi-page forms reserve a row for the step strip; single-page
    /// ones keep the tighter frame they had before pages existed.
    pub(super) has_strip: bool,
}

/// The rows a modal gives a filling field. A hosted form has a real
/// frame to divide up; a modal has only what it decides to ask for.
const MODAL_FILL_ROWS: u16 = 8;

impl FormMetrics {
    /// The rows the fields want, before the frame says what it has.
    fn requested_rows(&self) -> u16 {
        self.fields
            .iter()
            .map(|height| match height {
                FieldHeight::Row => 1,
                FieldHeight::Fill => MODAL_FILL_ROWS,
            })
            .sum()
    }

    fn fixed_rows(&self) -> u16 {
        self.fields
            .iter()
            .filter(|height| **height == FieldHeight::Row)
            .count() as u16
    }

    fn fill_count(&self) -> u16 {
        self.fields
            .iter()
            .filter(|height| **height == FieldHeight::Fill)
            .count() as u16
    }
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

/// The message row and the buttons are pinned to the bottom of the
/// frame and the fields stack from the top. A modal sizes its frame so
/// the two meet with one blank row between them; a hosted form is as
/// tall as its host and the slack falls in that gap.
pub(super) fn form_geometry(
    area: Rect,
    metrics: &FormMetrics,
    placement: &FormPlacement,
) -> FormGeometry {
    let frame = frame_rect(area, metrics, placement);
    let inner = inner_area(frame);
    if inner.height == 0 {
        return FormGeometry {
            frame,
            strip: Rect::ZERO,
            fields: Vec::new(),
            message: Rect::ZERO,
            buttons: Vec::new(),
        };
    }
    let strip = if metrics.has_strip {
        row_at(inner, inner.y)
    } else {
        Rect::ZERO
    };
    let button_row = row_at(inner, inner.bottom() - 1);
    let message = match inner.height {
        0 | 1 => Rect::ZERO,
        _ => row_at(inner, button_row.y - 1),
    };
    let fields = field_rects(inner, metrics, message);
    FormGeometry {
        frame,
        strip,
        fields,
        message,
        buttons: button_rects(button_row, metrics),
    }
}

/// A modal's frame is exactly as tall as its contents need; a hosted
/// one is whatever its host hands over.
fn frame_rect(area: Rect, metrics: &FormMetrics, placement: &FormPlacement) -> Rect {
    match placement {
        FormPlacement::Overlay => {
            let strip_rows = u16::from(metrics.has_strip);
            let height = metrics.requested_rows() + CHROME_ROWS + strip_rows;
            layout::centered_panel(area, PANEL_WIDTH_PCT, height)
        }
        FormPlacement::Host { layout, .. } => layout(&area),
    }
}

fn row_at(inner: Rect, y: u16) -> Rect {
    Rect {
        y,
        height: 1,
        ..inner
    }
}

/// Fields stack from the top of the frame down to the blank row above
/// the message, each taking its own height. A field with nowhere left to
/// go gets no box rather than one outside the frame.
fn field_rects(inner: Rect, metrics: &FormMetrics, message: Rect) -> Vec<Rect> {
    let top = inner.y.saturating_add(u16::from(metrics.has_strip));
    let limit = if message == Rect::ZERO {
        inner.bottom()
    } else {
        message.y.saturating_sub(1)
    };
    let fill_rows = fill_rows(limit.saturating_sub(top), metrics);
    let mut y = top;
    metrics
        .fields
        .iter()
        .map(|height| {
            let rows = match height {
                FieldHeight::Row => 1,
                FieldHeight::Fill => fill_rows,
            };
            let rect = band(inner, y, rows.min(limit.saturating_sub(y)));
            y = y.saturating_add(rows);
            rect
        })
        .collect()
}

/// What each filling field gets: the band minus the fixed rows, split
/// evenly. Nothing left over means nothing to give.
fn fill_rows(available: u16, metrics: &FormMetrics) -> u16 {
    match metrics.fill_count() {
        0 => 0,
        count => available.saturating_sub(metrics.fixed_rows()) / count,
    }
}

fn band(inner: Rect, y: u16, height: u16) -> Rect {
    if height == 0 {
        return Rect::ZERO;
    }
    Rect { y, height, ..inner }
}

#[derive(Clone, Copy)]
pub(super) enum Slot {
    Frame,
    Message,
    Field(usize),
    Button(usize),
}

/// Controls sit one rung above the frame so hit-testing never has to
/// break a tie between a control and the panel behind it.
const CONTROL_RUNG: i32 = 1;

/// What every control's layout closure needs to find its own box: the
/// shape of the form and where the form itself sits.
#[derive(Clone)]
pub(super) struct FormLayout {
    pub(super) metrics: FormMetrics,
    placement: FormPlacement,
}

impl FormLayout {
    pub(super) fn of(state: &super::state::FormState) -> Self {
        let labels = state.button_labels();
        Self {
            metrics: FormMetrics {
                fields: state.fields.iter().map(|field| field.spec.height).collect(),
                button_count: labels.len(),
                button_width: button_width(&labels),
                has_strip: state.has_strip(),
            },
            placement: state.placement().clone(),
        }
    }

    pub(super) fn geometry(&self, area: Rect) -> FormGeometry {
        form_geometry(area, &self.metrics, &self.placement)
    }

    pub(super) fn slot(&self, area: Rect, slot: Slot) -> Rect {
        let geometry = self.geometry(area);
        let picked = match slot {
            Slot::Frame => Some(geometry.frame),
            Slot::Message => Some(geometry.message),
            Slot::Field(index) => geometry.fields.get(index).copied(),
            Slot::Button(index) => geometry.buttons.get(index).copied(),
        };
        picked.unwrap_or(Rect::ZERO)
    }

    pub(super) fn frame_order(&self) -> i32 {
        self.placement.order()
    }

    pub(super) fn control_order(&self) -> i32 {
        self.placement.order() + CONTROL_RUNG
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
fn button_rects(row: Rect, metrics: &FormMetrics) -> Vec<Rect> {
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
            fields: vec![FieldHeight::Row; field_count],
            button_count: 2,
            button_width: 10,
            has_strip: false,
        }
    }

    #[test]
    fn rows_stack_without_overlapping_and_stay_inside_the_frame() {
        let geometry = form_geometry(
            Rect::new(0, 0, 100, 40),
            &metrics(3),
            &FormPlacement::Overlay,
        );
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

    fn hosted(rect: Rect) -> FormPlacement {
        FormPlacement::Host {
            layout: std::sync::Arc::new(move |_| rect),
            order: 0,
        }
    }

    /// A hosted form takes the rect its host hands over rather than
    /// sizing itself, and everything it draws stays inside it.
    #[test]
    fn a_hosted_form_fills_the_rect_it_is_given() {
        let host = Rect::new(40, 2, 60, 24);

        let geometry = form_geometry(Rect::new(0, 0, 200, 50), &metrics(3), &hosted(host));

        assert_eq!(geometry.frame, host);
        let placed = geometry
            .fields
            .iter()
            .chain(geometry.buttons.iter())
            .chain(std::iter::once(&geometry.message));
        for rect in placed {
            assert!(
                rect.x >= host.x
                    && rect.right() <= host.right()
                    && rect.y >= host.y
                    && rect.bottom() <= host.bottom(),
                "{rect:?} escaped its host {host:?}"
            );
        }
    }

    /// The slack in a tall host falls between the fields and the chrome,
    /// so the buttons stay on the bottom row where the eye expects them.
    #[test]
    fn hosted_chrome_is_pinned_to_the_bottom() {
        let host = Rect::new(0, 0, 60, 30);

        let geometry = form_geometry(host, &metrics(2), &hosted(host));

        assert_eq!(geometry.fields[0].y, host.y + 1, "fields start at the top");
        assert_eq!(
            geometry.buttons[0].y,
            host.bottom() - 2,
            "buttons sit on the last inner row"
        );
        assert_eq!(geometry.message.y, geometry.buttons[0].y - 1);
        assert!(
            geometry.message.y - geometry.fields[1].y > 1,
            "the slack falls between the fields and the message"
        );
    }

    /// The composer's body: five header rows and a text area that takes
    /// everything the reading column has left.
    #[test]
    fn a_filling_field_takes_the_rows_the_others_leave() {
        let host = Rect::new(0, 0, 60, 30);
        let metrics = FormMetrics {
            fields: vec![FieldHeight::Row, FieldHeight::Row, FieldHeight::Fill],
            ..metrics(0)
        };

        let geometry = form_geometry(host, &metrics, &hosted(host));

        let [first, second, body] = geometry.fields[..] else {
            panic!("expected three fields, got {:?}", geometry.fields);
        };
        assert_eq!(first.height, 1);
        assert_eq!(second.height, 1);
        assert_eq!(body.y, second.bottom(), "the body starts under the headers");
        assert_eq!(
            body.bottom(),
            geometry.message.y - 1,
            "and runs to the blank row above the message"
        );
    }

    #[test]
    fn a_frame_with_nothing_left_gives_the_filling_field_no_rows() {
        let host = Rect::new(0, 0, 60, 6);
        let metrics = FormMetrics {
            fields: vec![FieldHeight::Row, FieldHeight::Fill],
            ..metrics(0)
        };

        let geometry = form_geometry(host, &metrics, &hosted(host));

        assert_eq!(geometry.fields[0].height, 1);
        assert_eq!(
            geometry.fields[1],
            Rect::ZERO,
            "a body with no room draws nothing rather than overrunning the chrome"
        );
        assert!(geometry.buttons[0].bottom() <= host.bottom());
    }

    /// A modal cannot ask its host how tall to be, so it picks a height
    /// for a filling field and sizes its frame around it.
    #[test]
    fn a_modal_sizes_itself_around_a_filling_field() {
        let metrics = FormMetrics {
            fields: vec![FieldHeight::Row, FieldHeight::Fill],
            ..metrics(0)
        };

        let geometry = form_geometry(Rect::new(0, 0, 100, 40), &metrics, &FormPlacement::Overlay);

        assert_eq!(geometry.fields[1].height, MODAL_FILL_ROWS);
        assert!(geometry.fields[1].bottom() < geometry.message.y);
    }

    #[test]
    fn buttons_are_right_aligned_and_uniform() {
        let geometry = form_geometry(
            Rect::new(0, 0, 100, 40),
            &metrics(2),
            &FormPlacement::Overlay,
        );
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
            fields: vec![FieldHeight::Row],
            button_count: 2,
            button_width: 60,
            has_strip: false,
        };
        let geometry = form_geometry(Rect::new(0, 0, 60, 30), &wide, &FormPlacement::Overlay);
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
                let geometry = form_geometry(area, &metrics(4), &FormPlacement::Overlay);
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
        let plain = form_geometry(area, &metrics(2), &FormPlacement::Overlay);
        let stripped = form_geometry(
            area,
            &FormMetrics {
                has_strip: true,
                ..metrics(2)
            },
            &FormPlacement::Overlay,
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
