//! Small building blocks shared by the apps.
//!
//! Task K.1 adds toolbar buttons, sidebar rows and empty states here.

use iced::Color;

/// `#RRGGBB` (or `#RRGGBBAA`, alpha ignored) as a colour; `None` for anything else.
pub fn color_from_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
}

/// A collection's colour, or a neutral blue when the server reports none.
pub fn collection_color(color: Option<&str>) -> Color {
    color.and_then(color_from_hex).unwrap_or(Color::from_rgb8(0x7F, 0xB2, 0xFF))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(color_from_hex("#D50C0C"), Some(Color::from_rgb8(0xD5, 0x0C, 0x0C)));
        assert_eq!(color_from_hex("#0082c9FF"), Some(Color::from_rgb8(0x00, 0x82, 0xC9)));
        assert_eq!(color_from_hex("red"), None);
        assert_eq!(color_from_hex("#12345"), None);
    }
}
