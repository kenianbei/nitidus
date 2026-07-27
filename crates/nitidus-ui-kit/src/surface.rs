//! Chrome shared by every modal surface: the cleared region, the
//! bordered frame, its title, and an optional key hint along the bottom
//! edge. Drawing it in one place is what makes a picker, a form, a
//! confirmation and the file browser read as the same kind of thing.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Widget};

pub struct FrameChrome<'a> {
    pub title: &'a str,
    /// Key hint drawn bottom-right; surfaces whose keys are obvious pass
    /// `None`.
    pub hint: Option<&'a str>,
    pub style: Style,
}

/// Clears `area`, draws the frame, and returns the region inside the
/// border for the surface's own content.
pub fn draw_frame(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    chrome: FrameChrome<'_>,
) -> Rect {
    Clear.render(area, buffer);
    let mut block = Block::bordered()
        .title(format!(" {} ", chrome.title))
        .style(chrome.style);
    if let Some(hint) = chrome.hint {
        block = block.title_bottom(Line::from(hint).right_aligned());
    }
    let inner = block.inner(area);
    block.render(area, buffer);
    inner
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn chrome<'a>(title: &'a str, hint: Option<&'a str>) -> FrameChrome<'a> {
        FrameChrome {
            title,
            hint,
            style: Style::default(),
        }
    }

    #[test]
    fn inner_region_sits_inside_the_border() {
        let area = Rect::new(2, 3, 20, 10);
        let mut buffer = ratatui::buffer::Buffer::empty(area);

        let inner = draw_frame(&mut buffer, area, chrome("Attach", None));

        assert_eq!(inner, Rect::new(3, 4, 18, 8));
    }

    #[test]
    fn the_title_is_padded_and_drawn_on_the_top_border() {
        let area = Rect::new(0, 0, 20, 5);
        let mut buffer = ratatui::buffer::Buffer::empty(area);

        draw_frame(&mut buffer, area, chrome("Keys", None));

        let top: String = (0..20).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(top.contains(" Keys "), "top border was {top:?}");
    }

    #[test]
    fn a_hint_lands_on_the_bottom_border_and_is_optional() {
        let area = Rect::new(0, 0, 24, 5);

        let mut with_hint = ratatui::buffer::Buffer::empty(area);
        draw_frame(&mut with_hint, area, chrome("Files", Some("Esc cancel")));
        let bottom: String = (0..24).map(|x| with_hint[(x, 4)].symbol()).collect();
        assert!(
            bottom.contains("Esc cancel"),
            "bottom border was {bottom:?}"
        );

        let mut without = ratatui::buffer::Buffer::empty(area);
        draw_frame(&mut without, area, chrome("Files", None));
        let plain: String = (0..24).map(|x| without[(x, 4)].symbol()).collect();
        assert!(!plain.contains("Esc"), "bottom border was {plain:?}");
    }

    #[test]
    fn a_frame_too_small_for_a_border_yields_an_empty_inner_region() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = ratatui::buffer::Buffer::empty(area);

        let inner = draw_frame(&mut buffer, area, chrome("x", None));

        assert!(
            inner.width == 0 || inner.height == 0,
            "nothing may be drawn inside a 1x1 frame, got {inner:?}"
        );
    }
}
