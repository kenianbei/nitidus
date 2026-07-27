//! Drawing a form. Each control is its own entity with its own rect, so
//! these renderers only ever see the box they own — the frame draws
//! beneath them, the controls on top.

use nitidus_ui_kit::surface::{FrameChrome, draw_frame};
use nitidus_ui_kit::theme::{Theme, ThemeColorStates, ThemeColors};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::geometry::{LABEL_WIDTH, value_area};
use super::interaction::{Interaction, Visual};
use super::state::StepState;

#[derive(Clone)]
pub(super) struct FrameView {
    pub(super) title: String,
    surface: Style,
}

impl FrameView {
    pub(super) fn new(title: String, theme: &Theme) -> Self {
        Self {
            title,
            surface: theme.paper.default.normal.style(),
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
    Ok(())
}

/// The message row is its own entity so the frame beneath it can stay a
/// plain block; it reports validation failures without moving anything.
#[derive(Clone)]
pub(super) struct MessageView {
    pub(super) message: Option<String>,
    style: Style,
}

impl MessageView {
    pub(super) fn new(message: Option<String>, theme: &Theme) -> Self {
        Self {
            message,
            style: theme.paper.error.normal.style(),
        }
    }
}

pub(super) fn render_message(
    frame: &mut ratatui::Frame,
    area: Rect,
    view: &mut MessageView,
) -> bevy::prelude::Result {
    let Some(message) = &view.message else {
        return Ok(());
    };
    frame.render_widget(
        Paragraph::new(Line::styled(message.clone(), view.style)),
        area,
    );
    Ok(())
}

/// A select renders its choice between guillemets so it reads as
/// something you change rather than something you type into.
const SELECT_OPEN: &str = "\u{2039} ";
const SELECT_CLOSE: &str = " \u{203a}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FieldViewKind {
    Text { masked: bool },
    Select,
}

#[derive(Clone)]
pub(super) struct FieldView {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) detail: Option<String>,
    pub(super) cursor: usize,
    pub(super) kind: FieldViewKind,
    pub(super) focused: bool,
    pub(super) is_error: bool,
    pub(super) interaction: Interaction,
    states: ThemeColorStates,
    error: ThemeColors,
}

impl FieldView {
    pub(super) fn new(label: String, kind: FieldViewKind, theme: &Theme) -> Self {
        Self {
            label,
            value: String::new(),
            detail: None,
            cursor: 0,
            kind,
            focused: false,
            is_error: false,
            interaction: Interaction::default(),
            states: theme.paper.default,
            error: theme.paper.error.normal,
        }
    }

    fn is_masked(&self) -> bool {
        matches!(self.kind, FieldViewKind::Text { masked: true })
    }

    fn label_style(&self) -> Style {
        if self.is_error {
            self.error.style().add_modifier(Modifier::BOLD)
        } else {
            self.states.normal.style()
        }
    }
}

pub(super) fn render_field(
    frame: &mut ratatui::Frame,
    area: Rect,
    view: &mut FieldView,
) -> bevy::prelude::Result {
    let label = truncated(&view.label, LABEL_WIDTH.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(Line::styled(label, view.label_style())),
        Rect {
            width: LABEL_WIDTH.min(area.width),
            ..area
        },
    );
    let box_area = value_area(area);
    match view.kind {
        FieldViewKind::Select => render_select_value(frame, box_area, view),
        FieldViewKind::Text { .. } => render_text_value(frame, box_area, view),
    }
    Ok(())
}

fn render_text_value(frame: &mut ratatui::Frame, area: Rect, view: &FieldView) {
    let shown = if view.is_masked() {
        "*".repeat(view.value.chars().count())
    } else {
        view.value.clone()
    };
    let (visible, column) = windowed(&shown, view.cursor, area.width);
    let style = Visual::resolve(view.focused, view.interaction)
        .colors(&view.states)
        .style();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            padded(&visible, area.width),
            style,
        ))),
        area,
    );
    if view.focused {
        frame.set_cursor_position((area.x.saturating_add(column), area.y));
    }
}

/// The chosen option fills the value box; the detail trails it dimmed
/// and is dropped rather than wrapped when the row runs out of room.
fn render_select_value(frame: &mut ratatui::Frame, area: Rect, view: &FieldView) {
    let style = Visual::resolve(view.focused, view.interaction)
        .colors(&view.states)
        .style();
    let choice = format!("{SELECT_OPEN}{}{SELECT_CLOSE}", view.value);
    let mut spans = vec![Span::styled(choice.clone(), style)];
    let used = choice.chars().count();
    if let Some(detail) = &view.detail {
        let room = usize::from(area.width).saturating_sub(used + 2);
        if room > 0 {
            let text = format!("  {}", truncated(detail, room as u16));
            spans.push(Span::styled(text, view.states.disabled.style()));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// One step in the strip. Its own entity, so an unreached step can
/// carry `UiDisabled` and refuse both focus and the pointer the same way
/// any other control does.
#[derive(Clone)]
pub(super) struct StepView {
    pub(super) title: String,
    pub(super) state: StepState,
    pub(super) interaction: Interaction,
    states: ThemeColorStates,
}

impl StepView {
    pub(super) fn new(title: String, state: StepState, theme: &Theme) -> Self {
        Self {
            title,
            state,
            interaction: Interaction::default(),
            states: theme.paper.default,
        }
    }
}

pub(super) fn render_step(
    frame: &mut ratatui::Frame,
    area: Rect,
    view: &mut StepView,
) -> bevy::prelude::Result {
    let style = match view.state {
        StepState::Current => view.states.selected.style().add_modifier(Modifier::BOLD),
        StepState::Unreached => view.states.disabled.style(),
        StepState::Reached => Visual::resolve(false, view.interaction)
            .colors(&view.states)
            .style(),
    };
    let width = usize::from(area.width);
    let title = format!("{:^width$}", truncated(&view.title, area.width));
    frame.render_widget(Paragraph::new(Line::from(Span::styled(title, style))), area);
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
    let label = format!("{:^width$}", truncated(&view.label, area.width));
    frame.render_widget(Paragraph::new(Line::from(Span::styled(label, style))), area);
    Ok(())
}

fn truncated(text: &str, width: u16) -> String {
    text.chars().take(usize::from(width)).collect()
}

fn padded(text: &str, width: u16) -> String {
    let padding = usize::from(width).saturating_sub(text.chars().count());
    format!("{text}{}", " ".repeat(padding))
}

/// Scrolls a value so the cursor stays inside the box, keeping the tail
/// visible once the text outgrows it.
fn windowed(value: &str, cursor: usize, width: u16) -> (String, u16) {
    let width = usize::from(width);
    if width == 0 {
        return (String::new(), 0);
    }
    let start = cursor.saturating_sub(width.saturating_sub(1));
    let visible = value.chars().skip(start).take(width).collect();
    (visible, (cursor - start) as u16)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_short_value_shows_from_the_start() {
        assert_eq!(windowed("abc", 3, 10), ("abc".to_owned(), 3));
        assert_eq!(windowed("abc", 0, 10), ("abc".to_owned(), 0));
    }

    #[test]
    fn a_long_value_scrolls_to_keep_the_cursor_visible() {
        let (visible, column) = windowed("abcdefghij", 10, 4);
        assert_eq!(visible, "hij", "the tail follows the cursor");
        assert_eq!(column, 3);
        assert!(column < 4, "the cursor stays inside the box");
    }

    #[test]
    fn a_zero_width_box_renders_nothing_without_panicking() {
        assert_eq!(windowed("abc", 2, 0), (String::new(), 0));
    }

    #[test]
    fn padding_fills_the_box_and_truncation_respects_char_boundaries() {
        assert_eq!(padded("ab", 5), "ab   ");
        assert_eq!(padded("abcdef", 3), "abcdef", "never truncates");
        assert_eq!(truncated("héllo", 3), "hél");
    }
}
