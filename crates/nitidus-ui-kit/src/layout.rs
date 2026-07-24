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
    fn layout_fns_agree_with_split() {
        let area = Rect::new(0, 0, 100, 30);
        assert_eq!(tab_bar_layout()(&area), split_shell(area).tab_bar);
        assert_eq!(content_layout()(&area), split_shell(area).content);
        assert_eq!(statusline_layout()(&area), split_shell(area).statusline);
    }
}
