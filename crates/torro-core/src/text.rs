//! Checks on text from someone else (a client of the core, a font file on the computer) before a person
//! reads it: which characters are invisible yet change what is shown. One table for every caller, so
//! the approvals and the names of installed fonts never judge the same text apart.

/// A Unicode format character (general category Cf, Unicode 16): invisible, yet able to change
/// what a person reads. Bidi controls turn `Delete "gnihton"` around on screen, zero-width ones
/// join or split words unseen, tag characters carry hidden text. Rust's standard library has no
/// category lookup, so the ranges are listed here.
pub fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_characters_are_format_characters() {
        for c in ['\u{202E}', '\u{200B}', '\u{FEFF}', '\u{00AD}', '\u{E0041}'] {
            assert!(is_format(c), "{c:?}");
        }
        for c in ['a', ' ', 'ä', '\n', '\u{2028}'] {
            assert!(!is_format(c), "{c:?}");
        }
    }
}
