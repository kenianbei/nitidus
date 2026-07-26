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

use super::{ComposeSession, ComposeState, InlineEditor};
use crate::command::describe;
use crate::keymap::{CONTEXT_COMPOSE, CONTEXT_EDITOR, Keymaps};
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
    /// The live editor, cloned in each refresh. `TextArea` is plain data,
    /// so the widget can own a snapshot rather than reach into the world
    /// mid-render.
    editor: Option<EditorView>,
}

#[derive(Clone)]
struct EditorView {
    text: super::inline::SharedArea,
    /// Body line index → the style it is drawn in.
    styles: Vec<Option<Style>>,
    header: Vec<Line<'static>>,
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
    editor: Res<InlineEditor>,
    screen: Res<Screen>,
    keymaps: Res<Keymaps>,
    mut widgets: Query<&mut Widget, With<ComposeWidget>>,
) -> Result {
    if !(theme.is_changed() || compose.is_changed() || editor.is_changed() || screen.is_changed()) {
        return Ok(());
    }
    let Ok(mut widget) = widgets.single_mut() else {
        return Ok(());
    };
    let previous = widget.get_state::<ComposeWindow>()?;
    let last_height = previous.last_height;
    let scroll = previous.scroll;
    let editing = editor.is_active();
    let mut window = ComposeWindow {
        active: *screen == Screen::Compose,
        normal: theme.base.default.normal.style(),
        cheat: cheat_lines(
            &keymaps,
            &theme,
            if editing {
                CONTEXT_EDITOR
            } else {
                CONTEXT_COMPOSE
            },
        ),
        last_height,
        scroll,
        ..ComposeWindow::default()
    };
    if let Some(session) = compose.session() {
        window.lines = session_lines(session, &theme);
        window.scroll = scroll.min(window.lines.len().saturating_sub(1));
    }
    if let (Some(text), Some(lines), Some(session)) =
        (editor.shared(), editor.lines(), compose.session())
    {
        window.editor = Some(EditorView {
            text,
            styles: body_styles(&lines, &theme),
            header: header_lines(session, &theme),
        });
    }
    widget.set_state(window)?;
    Ok(())
}

/// Classifies each body line the same way the pager does, so a quote
/// looks like a quote on both sides of the app.
fn body_styles(lines: &[String], theme: &Theme) -> Vec<Option<Style>> {
    crate::pager::body::classify_lines(lines)
        .into_iter()
        .map(|kind| super::style::line_style(kind, theme))
        .collect()
}

fn session_lines(session: &ComposeSession, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = header_lines(session, theme);
    let normal = theme.base.default.normal.style();
    for body_line in &session.body {
        lines.push(Line::from(Span::styled(body_line.clone(), normal)));
    }
    lines
}

/// The headers block: everything above the body, shared by the review
/// screen and the editor.
fn header_lines(session: &ComposeSession, theme: &Theme) -> Vec<Line<'static>> {
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
    lines
}

/// Motions clutter the footer (arrows, Home/End and paging are
/// universal), as do the delete keys any editor already has; the sheet
/// shows what you cannot guess. `?` still lists everything.
const CHEAT_SKIP: &[&str] = &[
    ":next",
    ":prev",
    ":next-page",
    ":prev-page",
    ":first",
    ":last",
    ":help",
    ":editor-left",
    ":editor-right",
    ":editor-up",
    ":editor-down",
    ":editor-word-back",
    ":editor-word-forward",
    ":editor-line-start",
    ":editor-line-end",
    ":editor-paragraph-back",
    ":editor-paragraph-forward",
    ":editor-page-up",
    ":editor-page-down",
    ":editor-top",
    ":editor-bottom",
    ":editor-delete-word-back",
    ":editor-delete-word-forward",
    ":editor-delete-line-end",
];

/// The way out leads the sheet. Bindings are otherwise listed by key, and
/// the footer has two rows, so an alphabetically unlucky exit would wrap
/// off the end — exactly the hint a reader needs most.
const CHEAT_LEAD: &str = ":editor-done";

/// `Esc discard · e edit · …` built from the live bindings of whichever
/// context currently owns the keyboard.
fn cheat_lines(keymaps: &Keymaps, theme: &Theme, context: &str) -> Vec<Line<'static>> {
    let key_style = theme.base.info.normal.style().add_modifier(Modifier::BOLD);
    let text_style = theme.base.default.disabled.style();
    let mut rows = keymaps.bindings(context);
    rows.sort_by_key(|row| row.command != CHEAT_LEAD);
    let mut spans = Vec::new();
    for row in rows {
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

/// Headers stay pinned above the text area, which takes the rest. The
/// line-styling pass runs after the widget draws, over the same buffer.
fn render_editor(frame: &mut ratatui::Frame, area: Rect, state: &mut ComposeWindow) {
    let Some(view) = state.editor.as_ref() else {
        return;
    };
    let header_rows = u16::try_from(view.header.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let header_area = Rect {
        height: header_rows,
        ..area
    };
    frame.render_widget(
        Paragraph::new(view.header.clone()).style(state.normal),
        header_area,
    );
    let text_area = Rect {
        y: area.y.saturating_add(header_rows),
        height: area.height.saturating_sub(header_rows),
        ..area
    };
    if text_area.height == 0 {
        return;
    }
    let text = super::inline::lock(&view.text);
    frame.render_widget(&*text, text_area);
    super::style::paint_lines(frame.buffer_mut(), text_area, &text, state.normal, |row| {
        view.styles.get(row).copied().flatten()
    });
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
    if state.editor.is_some() {
        render_editor(frame, body_area, state);
    } else {
        let visible: Vec<Line<'static>> = state
            .lines
            .iter()
            .skip(state.scroll)
            .take(usize::from(body_rows))
            .cloned()
            .collect();
        frame.render_widget(Paragraph::new(visible).style(state.normal), body_area);
    }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::config::RawKeymaps;
    use crate::keymap::{CONTEXT_EDITOR, Keymaps};

    fn sheet(context: &str) -> String {
        let keymaps = Keymaps::compile(&RawKeymaps::default()).unwrap();
        let theme = nitidus_ui_kit::theme::tailwind_dark();
        cheat_lines(&keymaps, &theme, context)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    /// Two rows at a typical width; a sheet much longer than this wraps
    /// off the end of the footer and takes its last hints with it.
    const CHEAT_BUDGET: usize = 400;

    #[test]
    fn the_editor_sheet_leads_with_the_way_out() {
        let sheet = sheet(CONTEXT_EDITOR);
        assert!(
            sheet.starts_with("Esc finish editing the body"),
            "leaving must be the first thing the footer says: {sheet:?}"
        );
    }

    #[test]
    fn both_sheets_fit_the_footer() {
        for context in [CONTEXT_COMPOSE, CONTEXT_EDITOR] {
            let sheet = sheet(context);
            assert!(
                sheet.len() < CHEAT_BUDGET,
                "the {context} sheet is {} chars, too long for two rows: {sheet:?}",
                sheet.len()
            );
        }
    }

    #[test]
    fn the_editor_sheet_keeps_what_cannot_be_guessed() {
        let sheet = sheet(CONTEXT_EDITOR);
        for expected in ["Ctrl-z", "Ctrl-w", "Ctrl-v", "Ctrl-p"] {
            assert!(
                sheet.contains(expected),
                "{expected} missing from {sheet:?}"
            );
        }
        assert!(
            !sheet.contains("Left move left"),
            "arrow motions are universal and only clutter: {sheet:?}"
        );
    }
}
