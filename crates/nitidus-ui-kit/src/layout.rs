//! Shell layout: the persistent chrome regions every screen lives inside.

use plurimus::LayoutFn;
use ratatui::layout::{Constraint, Layout, Rect};
use std::sync::Arc;

/// Three rows: the comfy-tabs strip renders each tab as a bordered box.
pub const TAB_BAR_HEIGHT: u16 = 3;
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

/// How wide a column asks to be when the budget allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnWidth {
    /// This many cells, shrinking toward `min_width` under pressure.
    Fixed(u16),
    /// An equal share of whatever the fixed columns leave.
    Fill,
}

/// One column of a miller layout. A column that cannot be given
/// `min_width` collapses rather than being drawn uselessly narrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    pub preferred: ColumnWidth,
    pub min_width: u16,
    /// Higher survives longer; the lowest collapses first.
    pub priority: u8,
}

/// A one-column rule sits between neighbouring panes, so the eye can
/// tell where one ends and the next begins.
pub const COLUMN_GUTTER: u16 = 1;

/// Where the columns landed, plus the gutters between them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnLayout {
    /// One entry per spec; `None` marks a column the terminal was too
    /// narrow to hold.
    pub columns: Vec<Option<Rect>>,
    /// The rule between each surviving pair, left to right.
    pub separators: Vec<Rect>,
}

/// Divides the content region into columns, left to right.
///
/// Columns are dropped one at a time, lowest priority first, until the
/// survivors' minimums and the gutters between them fit. The last column
/// standing is kept whatever its minimum, because some pane has to own
/// the region.
pub fn split_columns(area: Rect, specs: &[ColumnSpec]) -> ColumnLayout {
    let content = split_shell(area).content;
    let mut kept: Vec<usize> = (0..specs.len()).collect();
    while kept.len() > 1 && minimum_total(specs, &kept) > usable_width(content.width, kept.len()) {
        let weakest = kept
            .iter()
            .copied()
            .min_by_key(|&index| (specs[index].priority, std::cmp::Reverse(index)))
            .unwrap_or(0);
        kept.retain(|&index| index != weakest);
    }
    let widths = allocate(usable_width(content.width, kept.len()), specs, &kept);
    let mut columns = vec![None; specs.len()];
    let mut separators = Vec::new();
    let mut x = content.x;
    for (slot, &index) in kept.iter().enumerate() {
        if slot > 0 {
            separators.push(Rect {
                x,
                y: content.y,
                width: COLUMN_GUTTER,
                height: content.height,
            });
            x = x.saturating_add(COLUMN_GUTTER);
        }
        let width = widths[slot];
        columns[index] = Some(Rect {
            x,
            y: content.y,
            width,
            height: content.height,
        });
        x = x.saturating_add(width);
    }
    ColumnLayout {
        columns,
        separators,
    }
}

/// What the columns themselves get once the gutters are reserved.
fn usable_width(total: u16, kept: usize) -> u16 {
    let gutters = (kept.saturating_sub(1) as u16).saturating_mul(COLUMN_GUTTER);
    total.saturating_sub(gutters)
}

pub fn column_layout(specs: Vec<ColumnSpec>, column: usize) -> LayoutFn {
    Arc::new(move |area| {
        split_columns(*area, &specs)
            .columns
            .get(column)
            .copied()
            .flatten()
            .unwrap_or(Rect::ZERO)
    })
}

fn minimum_total(specs: &[ColumnSpec], kept: &[usize]) -> u16 {
    kept.iter()
        .map(|&index| specs[index].min_width)
        .fold(0u16, u16::saturating_add)
}

/// Fixed columns are served first but never below the minimums the rest
/// still need; the fills divide what is left, the leftmost taking any
/// odd cell.
fn allocate(total: u16, specs: &[ColumnSpec], kept: &[usize]) -> Vec<u16> {
    let mut widths = vec![0u16; kept.len()];
    let mut remaining = total;
    let fill_slots: Vec<usize> = kept
        .iter()
        .enumerate()
        .filter(|&(_, &index)| specs[index].preferred == ColumnWidth::Fill)
        .map(|(slot, _)| slot)
        .collect();
    let mut unserved = minimum_total(specs, kept);
    for (slot, &index) in kept.iter().enumerate() {
        let spec = specs[index];
        unserved = unserved.saturating_sub(spec.min_width);
        let ColumnWidth::Fixed(preferred) = spec.preferred else {
            continue;
        };
        let ceiling = remaining.saturating_sub(unserved);
        let width = preferred.clamp(spec.min_width.min(ceiling), ceiling);
        widths[slot] = width;
        remaining -= width;
    }
    let Some(fill_count) = u16::try_from(fill_slots.len())
        .ok()
        .filter(|count| *count > 0)
    else {
        // Nothing wants the slack; the rightmost column absorbs it so
        // the columns still tile the region.
        if let Some(last) = widths.last_mut() {
            *last = last.saturating_add(remaining);
        }
        return widths;
    };
    let share = remaining / fill_count;
    let mut odd = remaining % fill_count;
    for &slot in &fill_slots {
        let extra = u16::from(odd > 0);
        odd = odd.saturating_sub(1);
        widths[slot] = share + extra;
    }
    widths
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

/// The content region less a vertical margin, centered and capped at
/// `max_width` columns — a line much wider than that is more tiring to
/// read than it is informative.
pub fn centered_capped(area: Rect, max_width: u16, vertical_margin: u16) -> Rect {
    let content = split_shell(area).content;
    let width = content.width.min(max_width);
    let height = content
        .height
        .saturating_sub(vertical_margin * 2)
        .max(1)
        .min(content.height);
    Rect {
        x: content.x + (content.width - width) / 2,
        y: content.y + (content.height - height) / 2,
        width,
        height,
    }
}

pub fn centered_capped_layout(max_width: u16, vertical_margin: u16) -> LayoutFn {
    Arc::new(move |area| centered_capped(*area, max_width, vertical_margin))
}

/// Bottom-right corner of the content region, sized as a fraction of
/// it. Used by surfaces that report rather than ask, so they sit out of
/// the way of the panes instead of over them.
pub fn corner_panel(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let content = split_shell(area).content;
    let width = scaled(content.width, width_pct);
    let height = scaled(content.height, height_pct);
    Rect {
        x: content.right().saturating_sub(width),
        y: content.bottom().saturating_sub(height),
        width,
        height,
    }
}

pub fn corner_panel_layout(width_pct: u16, height_pct: u16) -> LayoutFn {
    Arc::new(move |area| corner_panel(*area, width_pct, height_pct))
}

fn scaled(total: u16, percent: u16) -> u16 {
    ((u32::from(total) * u32::from(percent.min(100))) / 100) as u16
}

/// Like `centered_panel_layout`, but the height cap scales with the
/// terminal: the content height minus `vertical_margin` rows top and
/// bottom.
pub fn centered_tall_panel_layout(width_pct: u16, vertical_margin: u16) -> LayoutFn {
    Arc::new(move |area| {
        let content = split_shell(*area).content;
        let max_height = content.height.saturating_sub(vertical_margin * 2).max(1);
        centered_panel(*area, width_pct, max_height)
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn tall_panel_scales_with_the_terminal() {
        let layout = centered_tall_panel_layout(50, 1);
        let chrome = TAB_BAR_HEIGHT + STATUSLINE_HEIGHT;

        let short = layout(&Rect::new(0, 0, 80, 24));
        assert_eq!(short.height, 24 - chrome - 2, "content minus margins");

        let tall = layout(&Rect::new(0, 0, 80, 50));
        assert_eq!(tall.height, 50 - chrome - 2);
        assert!(tall.width >= 40, "width stays percentage-driven");
    }

    #[test]
    fn splits_standard_terminal() {
        let regions = split_shell(Rect::new(0, 0, 80, 24));
        assert_eq!(regions.tab_bar, Rect::new(0, 0, 80, TAB_BAR_HEIGHT));
        assert_eq!(
            regions.content,
            Rect::new(
                0,
                TAB_BAR_HEIGHT,
                80,
                24 - TAB_BAR_HEIGHT - STATUSLINE_HEIGHT
            )
        );
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

    fn fixed(width: u16, min: u16, priority: u8) -> ColumnSpec {
        ColumnSpec {
            preferred: ColumnWidth::Fixed(width),
            min_width: min,
            priority,
        }
    }

    fn fill(min: u16, priority: u8) -> ColumnSpec {
        ColumnSpec {
            preferred: ColumnWidth::Fill,
            min_width: min,
            priority,
        }
    }

    #[test]
    fn columns_tile_the_content_region_left_to_right() {
        let area = Rect::new(0, 0, 100, 30);
        let content = split_shell(area).content;
        let specs = [fixed(24, 15, 0), fill(15, 2)];

        let [left, right] = split_columns(area, &specs).columns[..] else {
            panic!("expected two columns");
        };
        let (left, right) = (left.unwrap(), right.unwrap());

        assert_eq!(left.width, 24);
        assert_eq!(left.x, content.x);
        assert_eq!(right.x, left.right() + COLUMN_GUTTER);
        assert_eq!(left.width + right.width + COLUMN_GUTTER, content.width);
        assert_eq!(left.height, content.height);
        assert_eq!(column_layout(specs.to_vec(), 0)(&area), left);
    }

    #[test]
    fn a_fixed_column_shrinks_to_its_minimum_before_anything_collapses() {
        let specs = [fixed(24, 15, 0), fill(15, 2)];
        // Two minimums plus the rule between them need exactly 31.
        let columns = split_columns(Rect::new(0, 0, 31, 20), &specs).columns;

        assert_eq!(columns[0].unwrap().width, 15, "shrunk, not collapsed");
        assert_eq!(columns[1].unwrap().width, 15);
    }

    #[test]
    fn the_lowest_priority_column_collapses_first() {
        let specs = [fixed(24, 15, 0), fill(15, 1), fill(15, 2)];

        let columns = split_columns(Rect::new(0, 0, 32, 20), &specs).columns;

        assert!(columns[0].is_none(), "priority 0 goes first");
        assert!(columns[1].is_some());
        assert!(columns[2].is_some());
    }

    #[test]
    fn the_last_column_standing_keeps_the_region_however_narrow() {
        let specs = [fixed(24, 15, 0), fill(15, 2)];

        let columns = split_columns(Rect::new(0, 0, 6, 20), &specs).columns;

        assert!(columns[0].is_none());
        assert_eq!(columns[1].unwrap().width, 6);
    }

    #[test]
    fn fills_divide_the_slack_and_the_leftmost_takes_the_odd_cell() {
        let specs = [fill(1, 1), fill(1, 1)];
        // Ten columns less the rule leaves nine to share.
        let columns = split_columns(Rect::new(0, 0, 10, 20), &specs).columns;

        assert_eq!(columns[0].unwrap().width, 5);
        assert_eq!(columns[1].unwrap().width, 4);
    }

    #[test]
    fn columns_and_gutters_tile_the_region_without_overlap_at_any_width() {
        let specs = [fixed(24, 15, 0), fill(15, 1), fill(15, 2)];
        for width in 0..140u16 {
            let area = Rect::new(0, 0, width, 20);
            let content = split_shell(area).content;
            let layout = split_columns(area, &specs);
            let live: Vec<Rect> = layout.columns.iter().copied().flatten().collect();
            assert_eq!(
                layout.separators.len(),
                live.len().saturating_sub(1),
                "one rule between each surviving pair, width {width}"
            );
            for (pair, gutter) in live.windows(2).zip(&layout.separators) {
                assert_eq!(gutter.x, pair[0].right(), "width {width}");
                assert_eq!(pair[1].x, gutter.right(), "width {width}");
            }
            if let Some(last) = live.last() {
                assert!(last.right() <= content.right(), "width {width}");
            }
        }
    }

    #[test]
    fn a_gutter_costs_the_columns_a_cell_each_seam() {
        let specs = [fill(1, 1), fill(1, 1)];

        let layout = split_columns(Rect::new(0, 0, 11, 20), &specs);

        let widths: Vec<u16> = layout.columns.iter().flatten().map(|c| c.width).collect();
        assert_eq!(widths.iter().sum::<u16>(), 11 - COLUMN_GUTTER);
        assert_eq!(layout.separators.len(), 1);
        assert_eq!(layout.separators[0].width, COLUMN_GUTTER);
    }

    #[test]
    fn a_corner_panel_hugs_the_bottom_right_of_the_content_region() {
        let area = Rect::new(0, 0, 100, 40);
        let content = split_shell(area).content;
        let panel = corner_panel(area, 50, 40);

        assert_eq!(panel.right(), content.right());
        assert_eq!(panel.bottom(), content.bottom());
        assert_eq!(panel.width, 50);
        assert!(panel.y >= content.y, "it must not climb into the tab bar");
        assert_eq!(corner_panel_layout(50, 40)(&area), panel);
    }

    #[test]
    fn a_corner_panel_stays_inside_a_tiny_terminal() {
        for height in 0..6 {
            let area = Rect::new(0, 0, 12, height);
            let panel = corner_panel(area, 50, 40);
            assert!(panel.right() <= area.right(), "{panel:?}");
            assert!(panel.bottom() <= area.bottom(), "{panel:?}");
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
