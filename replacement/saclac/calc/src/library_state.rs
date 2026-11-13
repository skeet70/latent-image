//! What saclac's start and stop set beside the library: calc's dispatch state and the adjustment
//! layer's first joint extras.

use crate::exposure::ports::StockMemory;
use crate::stock::diagnostics as stock;
use crate::stock::shot_dump::at;

/// Sets calc's dispatch state to 1 and marks the first joint block's extras unset (0xf4c384).
pub fn start(memory: &mut impl StockMemory) {
    memory.write_u32(stock::CALC_DISPATCH_STATE, 1);
    memory.fill(at(stock::JOINT_TABLES, stock::JOINT_EXTRAS), stock::JOINT_EXTRA_COUNT as usize, stock::JOINT_EXTRA_UNSET);
}

/// Sets calc's dispatch state to 0 (0xf4c3b8 through 0xf49504).
pub fn stop(memory: &mut impl StockMemory) { memory.write_u32(stock::CALC_DISPATCH_STATE, 0); }
