//! The review screen: headers block, scrollable body preview, and a
//! cheat-sheet footer generated from the live compose keymap so
//! rebindings show up automatically.

use bevy::prelude::*;
use nitidus_ui_kit::layout;
use nitidus_ui_kit::theme::Theme;
use plurimus::{Widget, WidgetLayout};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{ComposeSession, ComposeState};
use crate::command::describe;
use crate::keymap::{CONTEXT_COMPOSE, Keymaps};
use crate::screen::Screen;
use crate::sidebar::SIDEBAR_WIDTH;

const CHEAT_ROWS: u16 = 2;

#[derive(Component)]
pub struct ComposeWidget;

#[derive(Clone, Default)]
pub(super) struct ComposeWindow {
    active: bool,
    lines: Vec<Line<'static>>,
    cheat: Vec<Line<'static>>,
    pub(super) scroll: usize,
    normal: Style,
    last_height: u16,
}

impl ComposeWindow {
    pub(super) fn viewport_rows(&self) -> u16 {
        self.last_height.saturating_sub(CHEAT_ROWS)
    }

    pub(super) fn line_count(&self) -> usize {
        self.lines.len()
    }
}

pub(super) fn spawn_compose(mut commands: Commands) {
    commands.spawn((
        ComposeWidget,
        Widget::from_render_fn_with_state(render_compose, ComposeWindow::default()),
        WidgetLayout::from(layout::main_layout(SIDEBAR_WIDTH)),
    ));
}

pub(super) fn refresh_compose(
    theme: Res<Theme>,
    compose: Res<ComposeState>,
    screen: Res<Screen>,
    keymaps: Res<Keymaps>,
    mut widgets: Query<&mut Widget, With<ComposeWidget>>,
) -> Result {
    if !(theme.is_changed() || compose.is_changed() || screen.is_changed()) {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let previous = widget.get_state::<ComposeWindow>()?;
    let last_height = previous.last_height;
    let scroll = previous.scroll;
    let mut window = ComposeWindow {
        active: *screen == Screen::Compose,
        normal: theme.base.default.normal.style(),
        cheat: cheat_lines(&keymaps, &theme),
        last_height,
        scroll,
        ..ComposeWindow::default()
    };
    if let Some(session) = compose.session() {
        window.lines = session_lines(session, &theme);
        window.scroll = scroll.min(window.lines.len().saturating_sub(1));
    }
    widget.set_state(window)?;
    Ok(())
}

fn session_lines(session: &ComposeSession, theme: &Theme) -> Vec<Line<'static>> {
    let name_style = theme.base.info.normal.style().add_modifier(Modifier::BOLD);
    let normal = theme.base.default.normal.style();
    let dimmed = theme.base.default.disabled.style();
    let mut lines = Vec::new();
    let mut header = |name: &str, value: &str, optional: bool| {
        if value.is_empty() && optional {
            lines.push(Line::from(vec![
                Span::styled(format!("{name}: "), name_style),
                Span::styled("(none)", dimmed),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("{name}: "), name_style),
                Span::styled(value.to_owned(), normal),
            ]));
        }
    };
    header("From", &session.from, false);
    header("To", &session.to, false);
    header("Cc", &session.cc, true);
    header("Bcc", &session.bcc, true);
    header("Subject", &session.subject, false);
    for path in &session.attachments {
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment");
        lines.push(Line::from(vec![
            Span::styled("Attach: ".to_owned(), name_style),
            Span::styled(format!("📎 {name}  {size} bytes"), normal),
        ]));
    }
    lines.push(Line::default());
    for body_line in &session.body {
        lines.push(Line::from(Span::styled(body_line.clone(), normal)));
    }
    lines
}

/// Motions clutter the footer (j/k/arrows are universal); the sheet
/// shows compose-specific operations only. `?` still lists everything.
const CHEAT_SKIP: &[&str] = &[
    ":next",
    ":prev",
    ":next-page",
    ":prev-page",
    ":first",
    ":last",
    ":help",
];

/// `Esc discard · e edit · …` built from the live compose bindings.
fn cheat_lines(keymaps: &Keymaps, theme: &Theme) -> Vec<Line<'static>> {
    let key_style = theme.base.info.normal.style().add_modifier(Modifier::BOLD);
    let text_style = theme.base.default.disabled.style();
    let mut spans = Vec::new();
    for row in keymaps.bindings(CONTEXT_COMPOSE) {
        if CHEAT_SKIP.contains(&row.command.as_str()) {
            continue;
        }
        let summary = describe(&row.command).unwrap_or_default();
        if summary.is_empty() {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::styled("  ·  ", text_style));
        }
        spans.push(Span::styled(row.keys.clone(), key_style));
        spans.push(Span::styled(format!(" {summary}"), text_style));
    }
    vec![Line::default(), Line::from(spans)]
}

fn render_compose(frame: &mut ratatui::Frame, area: Rect, state: &mut ComposeWindow) -> Result {
    state.last_height = area.height;
    if !state.active {
        return Ok(());
    }
    frame.render_widget(ratatui::widgets::Clear, area);
    let body_rows = area.height.saturating_sub(CHEAT_ROWS);
    let body_area = Rect {
        height: body_rows,
        ..area
    };
    let visible: Vec<Line<'static>> = state
        .lines
        .iter()
        .skip(state.scroll)
        .take(usize::from(body_rows))
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(visible).style(state.normal), body_area);
    let cheat_area = Rect {
        y: area.y + body_rows,
        height: area.height - body_rows,
        ..area
    };
    frame.render_widget(
        Paragraph::new(state.cheat.clone())
            .style(state.normal)
            .wrap(ratatui::widgets::Wrap { trim: true }),
        cheat_area,
    );
    Ok(())
}
