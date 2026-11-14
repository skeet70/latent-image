//! The plan's stock tables and the configuration ids it reads (0xfa0130 and its helpers).

use crate::adjust::motion::plan::PlanKind;

/// The kind table 0xfa5e04: seven attribute bytes per kind.
const KIND_TABLE: [[u8; 7]; 28] = [
    [0, 0, 0, 1, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 0, 0, 0],
    [0, 0, 0, 1, 0, 0, 0],
    [0, 0, 0, 1, 0, 0, 0],
    [0, 0, 0, 1, 0, 0, 0],
    [0, 0, 1, 0, 0, 1, 0],
    [0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 1, 0, 0, 1],
    [0, 0, 0, 1, 0, 0, 0],
    [1, 1, 0, 0, 1, 0, 1],
    [1, 1, 0, 0, 1, 0, 1],
    [1, 1, 0, 0, 1, 0, 1],
    [1, 1, 1, 0, 1, 1, 1],
    [1, 1, 1, 0, 1, 0, 1],
    [1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 1, 0, 0],
    [1, 1, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 1, 1, 0],
    [1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 1, 0, 0, 0],
    [1, 1, 1, 0, 1, 0, 1],
];

/// A kind's attribute bytes; zeros above 27.
pub const fn kind_row(kind: PlanKind) -> [u8; 7] {
    if (kind.0 as usize) < KIND_TABLE.len() { KIND_TABLE[kind.0 as usize] } else { [0; 7] }
}

pub const fn direction_mask(scene: u8, style: u8) -> u32 {
    match scene {
        11..=13 => if style == 6 { 16 } else { 32 },
        18 | 21 | 25 => 128,
        5..=7 | 34 | 37 | 40 => 8,
        _ => 1,
    }
}
