//! Configuration ids and record bytes the library's record policies read.

use xm1_saclac_profile_records::Index;

use crate::exposure::ports::Property;

/// Bits 0 and 1: the calculation may fill row data (0xf60be0).
pub const ROW_DATA: Property = Property(0x5256);

/// The class limits fixed for every record: the upper class in the low nibble, the lower class
/// above it (0xf5f710).
pub const FIXED_CLASS_LIMITS: Property = Property(0x4a67);

/// The class limits of a level's band, laid out as `FIXED_CLASS_LIMITS`; band 0 takes band 2's
/// (0xf5f620..0xf5f698).
pub const fn band_class_limits(band: u8) -> Property {
    match band {
        1..=10 => Property(0x4a3f + band as u32),
        _ => Property(0x4a41),
    }
}

/// The mode positions of the record's three 0xa8-byte elements from +0x16c (0x752928).
pub const FIRST_POSITION: Index<3> = Index::new(0).unwrap();
pub const SECOND_POSITION: Index<3> = Index::new(1).unwrap();

/// What 0xf60e28 reads of a mode position element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PositionTerms {
    pub selector: u32,
    pub terms: [u16; 4],
    pub enabled: [bool; 2],
}

/// The selector word +4, the halfwords +0xc, +0xe, +0x10 and +0x1a, and the enable words +0x44
/// and +0x48 (0xf60e64..0xf60e98).
pub fn position_terms(element: &[u8; 168]) -> PositionTerms {
    let half = |at: usize| u16::from_le_bytes([element[at], element[at + 1]]);
    let word = |at: usize| u32::from_le_bytes([element[at], element[at + 1], element[at + 2], element[at + 3]]);
    PositionTerms { selector: word(4), terms: [half(0xc), half(0xe), half(0x10), half(0x1a)], enabled: [word(0x44) != 0, word(0x48) != 0] }
}
