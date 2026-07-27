//! The mail tab's column budget: which panes share the content region,
//! how wide each asks to be, and which gives way first when the terminal
//! is too narrow to hold them all.
//!
//! One declaration, consumed by every pane's `WidgetLayout`, so the
//! columns cannot disagree about where they are.

use bevy::prelude::*;
use nitidus_ui_kit::layout::{self, ColumnSpec, ColumnWidth, column_layout};
use nitidus_ui_kit::theme::Theme;
use plurimus::{LayoutFn, Widget, WidgetLayout};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Paragraph;

use crate::shell::Tabs;
use crate::sidebar::SidebarState;

pub const SIDEBAR_WIDTH: u16 = 24;
/// Below this a list is too narrow to read a subject in, so the column
/// collapses rather than being drawn uselessly thin.
pub const MIN_PANE_WIDTH: u16 = 15;

/// Reading yields first — a message can always be opened full screen —
/// then folders; losing the tree costs a keystroke, losing the list
/// costs the mailbox.
const READING_PRIORITY: u8 = 0;
const FOLDERS_PRIORITY: u8 = 1;
const MESSAGES_PRIORITY: u8 = 2;

const VERTICAL_RULE: &str = "│";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailPane {
    Folders,
    Messages,
    Reading,
}

impl MailPane {
    fn column(self, sidebar_visible: bool) -> usize {
        let offset = usize::from(sidebar_visible);
        match self {
            MailPane::Folders => 0,
            MailPane::Messages => offset,
            MailPane::Reading => offset + 1,
        }
    }
}

/// What the three columns have to divide between them: whether the
/// folder tree is here at all, and whether the list wants a width of
/// its own or shares the region with the reading pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaneBudget {
    pub sidebar_visible: bool,
    pub list_width: Option<u16>,
}

impl PaneBudget {
    pub fn new(sidebar_visible: bool, list_width: Option<u16>) -> Self {
        Self {
            sidebar_visible,
            list_width,
        }
    }
}

/// A hidden sidebar leaves the budget entirely rather than collapsing to
/// zero width, so the message list gets the whole region.
fn columns(budget: PaneBudget) -> Vec<ColumnSpec> {
    let messages = ColumnSpec {
        preferred: budget
            .list_width
            .map_or(ColumnWidth::Fill, ColumnWidth::Fixed),
        min_width: MIN_PANE_WIDTH,
        priority: MESSAGES_PRIORITY,
    };
    let reading = ColumnSpec {
        preferred: ColumnWidth::Fill,
        min_width: MIN_PANE_WIDTH,
        priority: READING_PRIORITY,
    };
    if !budget.sidebar_visible {
        return vec![messages, reading];
    }
    vec![
        ColumnSpec {
            preferred: ColumnWidth::Fixed(SIDEBAR_WIDTH),
            min_width: MIN_PANE_WIDTH,
            priority: FOLDERS_PRIORITY,
        },
        messages,
        reading,
    ]
}

pub fn mail_layout(pane: MailPane, budget: PaneBudget) -> LayoutFn {
    column_layout(columns(budget), pane.column(budget.sidebar_visible))
}

/// Draws the rules between panes. One widget over the whole content
/// region, painting only the gutter columns the budget reserved — no
/// pane owns those cells, so nothing can be drawn over.
pub struct PaneRulesPlugin;

impl Plugin for PaneRulesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_rules);
        app.add_systems(Update, refresh_rules);
    }
}

#[derive(Component)]
struct PaneRules;

#[derive(Clone, Default)]
struct RulesWindow {
    budget: PaneBudget,
    active: bool,
    style: Style,
}

fn spawn_rules(mut commands: Commands) {
    commands.spawn((
        PaneRules,
        Widget::from_render_fn_with_state(render_rules, RulesWindow::default()),
        WidgetLayout::from(layout::content_layout()),
    ));
}

fn refresh_rules(
    (theme, config): (Res<Theme>, Res<crate::config::Config>),
    tabs: Res<Tabs>,
    sidebar: Res<SidebarState>,
    mut widgets: Query<&mut Widget, With<PaneRules>>,
) -> Result {
    if !(theme.is_changed() || config.is_changed() || tabs.is_changed() || sidebar.is_changed()) {
        return Ok(());
    }
    for mut widget in &mut widgets {
        widget.set_state(RulesWindow {
            budget: PaneBudget::new(sidebar.visible, config.ui.index.list_width()),
            active: !tabs.is_contacts(),
            style: theme.base.default.disabled.style(),
        })?;
    }
    Ok(())
}

fn render_rules(frame: &mut ratatui::Frame, area: Rect, state: &mut RulesWindow) -> Result {
    if !state.active {
        return Ok(());
    }
    // The widget's own rect is the content region, so the separators the
    // budget reports are already in its coordinates.
    for gutter in layout::split_columns(area, &columns(state.budget)).separators {
        let rule = vec![ratatui::text::Line::from(VERTICAL_RULE); usize::from(gutter.height)];
        frame.render_widget(Paragraph::new(rule).style(state.style), gutter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use nitidus_ui_kit::layout::split_shell;
    use ratatui::layout::Rect;

    use nitidus_ui_kit::layout::COLUMN_GUTTER;

    use super::*;

    const CARD_LIST_WIDTH: u16 = 36;

    /// The table layout's budget: both content panes are fills.
    fn sharing(sidebar_visible: bool) -> PaneBudget {
        PaneBudget::new(sidebar_visible, None)
    }

    fn rect(pane: MailPane, sidebar_visible: bool, area: Rect) -> Rect {
        mail_layout(pane, sharing(sidebar_visible))(&area)
    }

    #[test]
    fn a_wide_terminal_tiles_all_three_panes_left_to_right() {
        let area = Rect::new(0, 0, 120, 30);
        let content = split_shell(area).content;

        let folders = rect(MailPane::Folders, true, area);
        let messages = rect(MailPane::Messages, true, area);
        let reading = rect(MailPane::Reading, true, area);

        assert_eq!(folders.width, SIDEBAR_WIDTH);
        assert_eq!(folders.x, content.x);
        assert_eq!(messages.x, folders.right() + COLUMN_GUTTER);
        assert_eq!(reading.x, messages.right() + COLUMN_GUTTER);
        assert_eq!(reading.right(), content.right());
        assert_eq!(messages.width, reading.width, "the fills share evenly");
    }

    /// R1 A4: the reading pane goes before the folder tree does.
    #[test]
    fn reading_collapses_before_folders() {
        // Three minimums need 45 columns; 40 forces exactly one out.
        let area = Rect::new(0, 0, 40, 20);

        assert_eq!(
            rect(MailPane::Reading, true, area),
            Rect::ZERO,
            "reading yields first"
        );
        assert!(rect(MailPane::Folders, true, area).width > 0);
        assert!(rect(MailPane::Messages, true, area).width >= MIN_PANE_WIDTH);
    }

    #[test]
    fn a_terminal_too_narrow_for_two_keeps_only_the_message_list() {
        let area = Rect::new(0, 0, 20, 20);

        assert_eq!(rect(MailPane::Reading, true, area), Rect::ZERO);
        assert_eq!(rect(MailPane::Folders, true, area), Rect::ZERO);
        assert_eq!(rect(MailPane::Messages, true, area).width, 20);
    }

    #[test]
    fn hiding_the_sidebar_splits_the_region_between_list_and_reading() {
        let area = Rect::new(0, 0, 100, 30);
        let content = split_shell(area).content;

        let messages = rect(MailPane::Messages, false, area);
        let reading = rect(MailPane::Reading, false, area);

        assert_eq!(messages.x, content.x);
        assert_eq!(reading.x, messages.right() + COLUMN_GUTTER);
        assert_eq!(reading.right(), content.right());
    }

    #[test]
    fn a_rule_sits_in_every_seam_between_visible_panes() {
        let area = Rect::new(0, 0, 120, 30);

        let layout = layout::split_columns(area, &columns(sharing(true)));

        assert_eq!(layout.separators.len(), 2, "folders|messages|reading");
        for (pair, gutter) in layout
            .columns
            .iter()
            .flatten()
            .collect::<Vec<_>>()
            .windows(2)
            .zip(&layout.separators)
        {
            assert_eq!(gutter.x, pair[0].right());
            assert_eq!(pair[1].x, gutter.right());
            assert_eq!(gutter.height, pair[0].height, "rules run the full height");
        }
    }

    #[test]
    fn a_collapsed_pane_takes_its_rule_with_it() {
        // Only the message list survives at this width.
        let layout = layout::split_columns(Rect::new(0, 0, 20, 20), &columns(sharing(true)));

        assert_eq!(layout.columns.iter().flatten().count(), 1);
        assert!(layout.separators.is_empty(), "one pane needs no rules");
    }

    #[test]
    fn a_fixed_list_keeps_its_width_and_gives_the_rest_to_reading() {
        let area = Rect::new(0, 0, 120, 30);
        let content = split_shell(area).content;
        let budget = PaneBudget::new(true, Some(CARD_LIST_WIDTH));
        let pane = |pane| mail_layout(pane, budget)(&area);

        let messages = pane(MailPane::Messages);
        let reading = pane(MailPane::Reading);

        assert_eq!(messages.width, CARD_LIST_WIDTH);
        assert!(
            reading.width > messages.width,
            "the reading pane takes the slack: {reading:?}"
        );
        assert_eq!(reading.right(), content.right());
    }

    #[test]
    fn a_wider_terminal_grows_only_the_reading_pane() {
        let budget = PaneBudget::new(true, Some(CARD_LIST_WIDTH));
        let narrow = Rect::new(0, 0, 100, 30);
        let wide = Rect::new(0, 0, 160, 30);
        let pane = |pane, area: Rect| mail_layout(pane, budget)(&area);

        assert_eq!(
            pane(MailPane::Messages, narrow).width,
            pane(MailPane::Messages, wide).width,
            "a fixed list does not stretch"
        );
        assert!(pane(MailPane::Reading, wide).width > pane(MailPane::Reading, narrow).width);
    }

    #[test]
    fn a_list_narrower_than_the_minimum_is_clamped_not_honoured() {
        let area = Rect::new(0, 0, 120, 30);
        let budget = PaneBudget::new(true, Some(2));

        assert_eq!(
            mail_layout(MailPane::Messages, budget)(&area).width,
            MIN_PANE_WIDTH,
            "the pane budget floor wins over the configured width"
        );
    }

    #[test]
    fn columns_never_overlap_at_any_width() {
        for width in 0..140u16 {
            let area = Rect::new(0, 0, width, 20);
            let live: Vec<Rect> = [MailPane::Folders, MailPane::Messages, MailPane::Reading]
                .into_iter()
                .map(|pane| rect(pane, true, area))
                .filter(|rect| *rect != Rect::ZERO)
                .collect();
            for pair in live.windows(2) {
                assert_eq!(pair[1].x, pair[0].right() + COLUMN_GUTTER, "width {width}");
            }
            if let Some(last) = live.last() {
                assert!(last.right() <= width, "width {width}");
            }
        }
    }
}
