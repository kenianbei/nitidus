use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ThemeColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn darken(self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        Self {
            r: lerp(self.r, u8::MIN, amount),
            g: lerp(self.g, u8::MIN, amount),
            b: lerp(self.b, u8::MIN, amount),
        }
    }

    pub fn lighten(self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        Self {
            r: lerp(self.r, u8::MAX, amount),
            g: lerp(self.g, u8::MAX, amount),
            b: lerp(self.b, u8::MAX, amount),
        }
    }
}

fn lerp(from: u8, to: u8, t: f32) -> u8 {
    let value = f32::from(from) + (f32::from(to) - f32::from(from)) * t;
    value.round().clamp(0.0, 255.0) as u8
}

impl From<ThemeColor> for Color {
    fn from(color: ThemeColor) -> Self {
        Color::Rgb(color.r, color.g, color.b)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn darken_moves_toward_black() {
        let color = ThemeColor::new(100, 150, 200);
        let darker = color.darken(0.5);
        assert_eq!(darker, ThemeColor::new(50, 75, 100));
        assert_eq!(color.darken(1.0), ThemeColor::new(0, 0, 0));
        assert_eq!(color.darken(0.0), color);
    }

    #[test]
    fn lighten_moves_toward_white() {
        let color = ThemeColor::new(100, 150, 200);
        let lighter = color.lighten(0.5);
        assert!(lighter.r > color.r && lighter.g > color.g && lighter.b > color.b);
        assert_eq!(color.lighten(1.0), ThemeColor::new(255, 255, 255));
        assert_eq!(color.lighten(0.0), color);
    }

    #[test]
    fn amounts_are_clamped() {
        let color = ThemeColor::new(10, 20, 30);
        assert_eq!(color.darken(2.0), ThemeColor::new(0, 0, 0));
        assert_eq!(color.lighten(-1.0), color);
    }

    #[test]
    fn converts_to_ratatui_rgb() {
        assert_eq!(
            Color::from(ThemeColor::new(1, 2, 3)),
            Color::Rgb(1, 2, 3),
            "ThemeColor should convert to Color::Rgb losslessly"
        );
    }
}
