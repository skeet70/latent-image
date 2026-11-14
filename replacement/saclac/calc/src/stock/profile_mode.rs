//! Configuration ids and scene sets of a profile mode's step and its limit flags (id5 0xf5271c,
//! 0xf5ea70, 0xf5e72c, 0xf5e420), and the cell table the mode's caller copies.

use crate::adjust::profile_adjustment::halfword_offset;
use crate::exposure::ports::Property;
use crate::stock::calculation::Cell;

/// Bit 3 of `property::ADJUSTMENT_FLAGS` turns the limit flags off (0xf5eae8).
pub const FLAGS_OFF: u32 = 8;

/// The margin of a configured spread, in units of ten: when the second level lies above the
/// position, and otherwise (0xf5e448).
pub const SPREAD_MARGIN_ABOVE: Property = Property(0x657f);
pub const SPREAD_MARGIN: Property = Property(0x657e);
/// The margin of a bounded spread, in units of ten (0xf5e46c).
pub const BOUNDED_MARGIN: Property = Property(0x746c);
/// Nonzero: a bounded spread ends at the position (0xf5e480).
pub const BOUNDED_TO_POSITION: Property = Property(0x746b);

/// The rise of the second limit level, in units of ten, by scene (0xf5e94c..0xf5e980).
pub const fn second_rise(scene: u32) -> Property {
    Property(match scene {
        17 => 0x6581,
        30 => 0x6582,
        _ => 0x6580,
    })
}

/// Scenes whose limit levels stand on the base level of the record's lens step rather than the
/// limits' near step (0xf5e828).
pub const fn on_record_step(scene: u32) -> bool { matches!(scene, 46 | 47 | 54 | 56) }

/// Scenes that styles 2, 3 and 7 below the first level flag for the bank's own lower level
/// (0xf5e5ec).
pub const fn below_own_lower(scene: u32) -> bool { matches!(scene, 45 | 47 | 55 | 56) }

/// Armed scenes whose position below the first level flags the bank's own lower level
/// (0xf5e514, 0xf5e62c).
pub const fn armed_own_lower(scene: u32) -> bool { matches!(scene, 11 | 12 | 14 | 15) }

/// Armed scenes that styles 0 and 1 flag at or above the first level whenever the position is not
/// below it (0xf5e53c's table).
pub const fn armed_at_or_above(scene: u32) -> bool { matches!(scene, 4 | 9 | 20 | 23 | 27 | 36 | 39 | 42) }

/// The cell table row a profile mode reads (0xf5271c's first argument to 0xf526fc).
const MODE_ROW: u32 = 7;

/// The bytes 0xf17abc copies from 0x4171783c for a profile mode's caller.
pub const CELL_TABLE_BYTES: usize = 0x218;

/// The cells a profile mode's caller copies: halfword cells, column byte first, by row, column
/// and index (0xf7a9ec).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellTable(pub [u8; CELL_TABLE_BYTES]);

impl CellTable {
    /// The cell a profile mode reads for `rate`: row 7, column `rate`, index 0 (0xf526fc).
    pub fn mode_cell(&self, rate: u32) -> Cell {
        let at = halfword_offset(MODE_ROW, rate, 0) as usize;
        Cell { column: self.0[at], row: self.0[at + 1] }
    }
}
