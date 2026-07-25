//! Shell layout: the persistent chrome regions every screen lives inside.

use plurimus::LayoutFn;
use ratatui::layout::{Constraint, Layout, Rect};
use std::sync::Arc;

pub const TAB_BAR_HEIGHT: u16 = 1;
pub const STATUSLINE_HEIGHT: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellRegions {
    pub tab_bar: Rect,
    pub content: Rect,
    pub statusline: Rect,
}

pub fn split_shell(area: Rect) -> ShellRegions {
    let [tab_bar, content, statusline] = Layout::vertical([
        Constraint::Length(TAB_BAR_HEIGHT),
        Constraint::Fill(1),
        Constraint::Length(STATUSLINE_HEIGHT),
    ])
    .areas(area);
    ShellRegions {
        tab_bar,
        content,
        statusline,
    }
}

pub fn tab_bar_layout() -> LayoutFn {
    Arc::new(|area| split_shell(*area).tab_bar)
}

pub fn content_layout() -> LayoutFn {
    Arc::new(|area| split_shell(*area).content)
}

pub fn statusline_layout() -> LayoutFn {
    Arc::new(|area| split_shell(*area).statusline)
}

/// Left column of the content region for a folder sidebar; clamps so a
/// narrow terminal always leaves the main column at least half.
pub fn sidebar_split(area: Rect, sidebar_width: u16) -> (Rect, Rect) {
    let content = split_shell(area).content;
    let width = sidebar_width.min(content.width / 2);
    let [sidebar, main] =
        Layout::horizontal([Constraint::Length(width), Constraint::Fill(1)]).areas(content);
    (sidebar, main)
}

pub fn sidebar_layout(sidebar_width: u16) -> LayoutFn {
    Arc::new(move |area| sidebar_split(*area, sidebar_width).0)
}

/// The content region minus the sidebar column.
pub fn main_layout(sidebar_width: u16) -> LayoutFn {
    Arc::new(move |area| sidebar_split(*area, sidebar_width).1)
}

/// Bottom-anchored strip of the content region sitting directly above
/// the statusline, `rows` high (clamped to the content height).
pub fn bottom_panel(area: Rect, rows: u16) -> Rect {
    let shell = split_shell(area);
    let height = rows.min(shell.content.height);
    Rect {
        x: shell.content.x,
        y: shell.statusline.y.saturating_sub(height),
        width: shell.content.width,
        height,
    }
}

pub fn bottom_panel_layout(rows: u16) -> LayoutFn {
    Arc::new(move |area| bottom_panel(*area, rows))
}

/// Floating rect for modal panels: centered inside the shell's content
/// region at `width_pct` of its width, up to `max_height` rows.
pub fn centered_panel(area: Rect, width_pct: u16, max_height: u16) -> Rect {
    let content = split_shell(area).content;
    let width = (u32::from(content.width) * u32::from(width_pct.min(100)) / 100) as u16;
    let width = width.clamp(content.width.min(20), content.width);
    let height = max_height.min(content.height);
    Rect {
        x: content.x + (content.width - width) / 2,
        y: content.y + (content.height - height) / 2,
        width,
        height,
    }
}

pub fn centered_panel_layout(width_pct: u16, max_height: u16) -> LayoutFn {
    Arc::new(move |area| centered_panel(*area, width_pct, max_height))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn splits_standard_terminal() {
        let regions = split_shell(Rect::new(0, 0, 80, 24));
        assert_eq!(regions.tab_bar, Rect::new(0, 0, 80, 1));
        assert_eq!(regions.content, Rect::new(0, 1, 80, 22));
        assert_eq!(regions.statusline, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn regions_cover_area_without_overlap() {
        let area = Rect::new(0, 0, 120, 40);
        let regions = split_shell(area);
        let total_height =
            regions.tab_bar.height + regions.content.height + regions.statusline.height;
        assert_eq!(total_height, area.height);
        assert_eq!(regions.content.y, regions.tab_bar.bottom());
        assert_eq!(regions.statusline.y, regions.content.bottom());
    }

    #[test]
    fn degenerate_heights_do_not_panic() {
        for height in 0..3 {
            let regions = split_shell(Rect::new(0, 0, 80, height));
            let total = regions.tab_bar.height + regions.content.height + regions.statusline.height;
            assert!(
                total <= height,
                "regions must not exceed a {height}-row terminal"
            );
        }
    }

    #[test]
    fn centered_panel_centers_and_clamps() {
        let area = Rect::new(0, 0, 100, 40);
        let panel = centered_panel(area, 50, 12);
        assert_eq!(panel.width, 50);
        assert_eq!(panel.height, 12);
        assert_eq!(panel.x, 25);
        assert!(panel.y > 1 && panel.bottom() < 39, "{panel:?}");

        let tiny = centered_panel(Rect::new(0, 0, 10, 5), 50, 12);
        assert!(tiny.width <= 10);
        assert!(tiny.height <= 3, "{tiny:?}");
        assert_eq!(centered_panel_layout(50, 12)(&area), panel);
    }

    #[test]
    fn sidebar_split_partitions_the_content_region() {
        let area = Rect::new(0, 0, 100, 30);
        let content = split_shell(area).content;
        let (sidebar, main) = sidebar_split(area, 24);
        assert_eq!(sidebar.width, 24);
        assert_eq!(sidebar.x, content.x);
        assert_eq!(main.x, sidebar.right());
        assert_eq!(sidebar.width + main.width, content.width);
        assert_eq!(sidebar.height, content.height);
        assert_eq!(sidebar_layout(24)(&area), sidebar);
        assert_eq!(main_layout(24)(&area), main);
    }

    #[test]
    fn sidebar_never_takes_more_than_half_a_narrow_terminal() {
        let (sidebar, main) = sidebar_split(Rect::new(0, 0, 30, 20), 24);
        assert_eq!(sidebar.width, 15);
        assert!(main.width >= 15);
    }

    #[test]
    fn layout_fns_agree_with_split() {
        let area = Rect::new(0, 0, 100, 30);
        assert_eq!(tab_bar_layout()(&area), split_shell(area).tab_bar);
        assert_eq!(content_layout()(&area), split_shell(area).content);
        assert_eq!(statusline_layout()(&area), split_shell(area).statusline);
    }
}
